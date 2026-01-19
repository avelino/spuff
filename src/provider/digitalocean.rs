//! DigitalOcean provider implementation.
//!
//! This module implements the Provider trait for DigitalOcean's API.
//! It handles droplet (instance) and snapshot management.

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::config::{ImageSpec, InstanceRequest, ProviderTimeouts, ProviderType};
use super::error::{ProviderError, ProviderResult};
use super::registry::ProviderFactory;
use super::{InstanceStatus, Provider, ProviderInstance, Snapshot};

const DEFAULT_API_BASE: &str = "https://api.digitalocean.com/v2";

/// DigitalOcean provider implementation.
#[derive(Debug)]
pub struct DigitalOceanProvider {
    client: Client,
    token: String,
    base_url: String,
    timeouts: ProviderTimeouts,
}

impl DigitalOceanProvider {
    /// Create a new DigitalOcean provider with default settings.
    pub fn new(token: &str) -> ProviderResult<Self> {
        Self::with_config(token, DEFAULT_API_BASE, ProviderTimeouts::default())
    }

    /// Create a new provider with custom base URL (for testing).
    pub fn with_base_url(token: &str, base_url: &str) -> ProviderResult<Self> {
        Self::with_config(token, base_url, ProviderTimeouts::default())
    }

    /// Create a new provider with full configuration.
    pub fn with_config(token: &str, base_url: &str, timeouts: ProviderTimeouts) -> ProviderResult<Self> {
        if token.is_empty() {
            return Err(ProviderError::auth(
                "digitalocean",
                "API token is required. Set DIGITALOCEAN_TOKEN or configure via 'spuff init'",
            ));
        }

        let client = Client::builder()
            .timeout(timeouts.http_request)
            .build()
            .map_err(ProviderError::Network)?;

        Ok(Self {
            client,
            token: token.to_string(),
            base_url: base_url.to_string(),
            timeouts,
        })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Resolve ImageSpec to DigitalOcean image slug or ID.
    fn resolve_image(&self, spec: &ImageSpec) -> String {
        match spec {
            ImageSpec::Ubuntu(version) => {
                let version_slug = version.replace('.', "-");
                format!("ubuntu-{}-x64", version_slug)
            }
            ImageSpec::Debian(version) => {
                format!("debian-{}-x64", version)
            }
            ImageSpec::Custom(id) => id.clone(),
            ImageSpec::Snapshot(id) => id.clone(),
        }
    }

    /// Fetch SSH key IDs from the DigitalOcean account.
    async fn get_ssh_key_ids(&self) -> ProviderResult<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/account/keys", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            tracing::warn!(
                "Failed to fetch SSH keys from DigitalOcean (HTTP {}). \
                 Instance will be created without pre-configured SSH keys.",
                status
            );
            return Ok(vec![]);
        }

        let data: SshKeysResponse = response.json().await?;
        let keys: Vec<String> = data.ssh_keys.into_iter().map(|k| k.id.to_string()).collect();

        if keys.is_empty() {
            tracing::debug!("No SSH keys found in DigitalOcean account");
        } else {
            tracing::debug!("Found {} SSH key(s) in DigitalOcean account", keys.len());
        }

        Ok(keys)
    }

    /// Wait for an action to complete.
    async fn wait_for_action(&self, action_id: u64) -> ProviderResult<()> {
        let max_attempts = self.timeouts.action_complete_attempts();
        let delay = self.timeouts.poll_interval;
        let start = std::time::Instant::now();

        for attempt in 0..max_attempts {
            let response = self
                .client
                .get(format!("{}/actions/{}", self.base_url, action_id))
                .header("Authorization", self.auth_header())
                .send()
                .await?;

            if response.status().is_success() {
                let data: ActionResponse = response.json().await?;
                match data.action.status.as_str() {
                    "completed" => return Ok(()),
                    "errored" => {
                        return Err(ProviderError::api(
                            500,
                            format!("Action {} failed", action_id),
                        ));
                    }
                    status => {
                        tracing::debug!(
                            "Action {} status: {} (attempt {}/{})",
                            action_id,
                            status,
                            attempt + 1,
                            max_attempts
                        );
                    }
                }
            }
            tokio::time::sleep(delay).await;
        }

        Err(ProviderError::timeout(
            format!("wait for action {}", action_id),
            start.elapsed(),
        ))
    }
}

#[async_trait]
impl Provider for DigitalOceanProvider {
    fn name(&self) -> &'static str {
        "digitalocean"
    }

    async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance> {
        let ssh_keys = self.get_ssh_key_ids().await?;
        let image = self.resolve_image(&request.image);

        // Convert labels to tags (DO uses simple string tags)
        let tags: Vec<String> = request.labels.keys().cloned().collect();

        let api_request = CreateDropletRequest {
            name: request.name.clone(),
            region: request.region.clone(),
            size: request.size.clone(),
            image,
            ssh_keys,
            user_data: request.user_data.clone(),
            tags,
            monitoring: true,
        };

        let response = self
            .client
            .post(format!("{}/droplets", self.base_url))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();

            return Err(match status {
                401 => ProviderError::auth("digitalocean", "Invalid API token"),
                403 => ProviderError::auth("digitalocean", body),
                422 => ProviderError::invalid_config("request", body),
                429 => ProviderError::RateLimit {
                    retry_after: Some(Duration::from_secs(60)),
                },
                _ => ProviderError::api(status, format!("Failed to create droplet: {}", body)),
            });
        }

        let data: DropletResponse = response.json().await?;

        Ok(ProviderInstance {
            id: data.droplet.id.to_string(),
            ip: "0.0.0.0".parse().unwrap(),
            status: InstanceStatus::New,
            created_at: Utc::now(),
        })
    }

    async fn destroy_instance(&self, id: &str) -> ProviderResult<()> {
        let response = self
            .client
            .delete(format!("{}/droplets/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        // 404 is OK - instance already gone
        if !response.status().is_success() && response.status().as_u16() != 404 {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to destroy droplet: {}", body),
            ));
        }

        Ok(())
    }

    async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>> {
        let response = self
            .client
            .get(format!("{}/droplets/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status().as_u16() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to get droplet: {}", body),
            ));
        }

        let data: DropletResponse = response.json().await?;
        Ok(Some(data.droplet.to_provider_instance()))
    }

    async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>> {
        let response = self
            .client
            .get(format!("{}/droplets?tag_name=spuff", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to list droplets: {}", body),
            ));
        }

        let data: DropletsResponse = response.json().await?;
        Ok(data
            .droplets
            .into_iter()
            .map(|d| d.to_provider_instance())
            .collect())
    }

    async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance> {
        let max_attempts = self.timeouts.instance_ready_attempts();
        let delay = self.timeouts.poll_interval;
        let start = std::time::Instant::now();

        for attempt in 0..max_attempts {
            if let Some(instance) = self.get_instance(id).await? {
                if instance.status == InstanceStatus::Active && instance.ip.to_string() != "0.0.0.0"
                {
                    return Ok(instance);
                }
                tracing::debug!(
                    "Instance {} status: {} ip: {} (attempt {}/{})",
                    id,
                    instance.status,
                    instance.ip,
                    attempt + 1,
                    max_attempts
                );
            }
            tokio::time::sleep(delay).await;
        }

        Err(ProviderError::timeout("wait for instance ready", start.elapsed()))
    }

    async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot> {
        #[derive(Serialize)]
        struct SnapshotAction {
            #[serde(rename = "type")]
            action_type: String,
            name: String,
        }

        let request = SnapshotAction {
            action_type: "snapshot".to_string(),
            name: name.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/droplets/{}/actions", self.base_url, instance_id))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to create snapshot: {}", body),
            ));
        }

        let action: ActionResponse = response.json().await?;
        self.wait_for_action(action.action.id).await?;

        // Find the snapshot by name
        let snapshots = self.list_snapshots().await?;
        snapshots
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                ProviderError::not_found("snapshot", format!("name={}", name))
            })
    }

    async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>> {
        let response = self
            .client
            .get(format!("{}/snapshots?resource_type=droplet", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to list snapshots: {}", body),
            ));
        }

        let data: SnapshotsResponse = response.json().await?;

        Ok(data
            .snapshots
            .into_iter()
            .filter(|s| s.name.starts_with("spuff"))
            .map(|s| Snapshot {
                id: s.id,
                name: s.name,
                created_at: DateTime::parse_from_rfc3339(&s.created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok(),
                size_gb: Some(s.size_gigabytes),
            })
            .collect())
    }

    async fn delete_snapshot(&self, id: &str) -> ProviderResult<()> {
        let response = self
            .client
            .delete(format!("{}/snapshots/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        // 404 is OK - snapshot already gone
        if !response.status().is_success() && response.status().as_u16() != 404 {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::api(
                status,
                format!("Failed to delete snapshot: {}", body),
            ));
        }

        Ok(())
    }

    async fn get_ssh_keys(&self) -> ProviderResult<Vec<String>> {
        self.get_ssh_key_ids().await
    }
}

/// Factory for creating DigitalOcean providers.
pub struct DigitalOceanFactory;

impl ProviderFactory for DigitalOceanFactory {
    fn provider_type(&self) -> ProviderType {
        ProviderType::DigitalOcean
    }

    fn create(&self, token: &str, timeouts: ProviderTimeouts) -> ProviderResult<Box<dyn Provider>> {
        Ok(Box::new(DigitalOceanProvider::with_config(
            token,
            DEFAULT_API_BASE,
            timeouts,
        )?))
    }
}

// ============================================================================
// API Request/Response Types
// ============================================================================

#[derive(Debug, Serialize)]
struct CreateDropletRequest {
    name: String,
    region: String,
    size: String,
    image: String,
    ssh_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_data: Option<String>,
    tags: Vec<String>,
    monitoring: bool,
}

#[derive(Debug, Deserialize)]
struct DropletResponse {
    droplet: DropletData,
}

#[derive(Debug, Deserialize)]
struct DropletsResponse {
    droplets: Vec<DropletData>,
}

#[derive(Debug, Deserialize)]
struct DropletData {
    id: u64,
    status: String,
    created_at: String,
    networks: Networks,
}

#[derive(Debug, Deserialize)]
struct Networks {
    v4: Vec<NetworkV4>,
}

#[derive(Debug, Deserialize)]
struct NetworkV4 {
    ip_address: String,
    #[serde(rename = "type")]
    network_type: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotsResponse {
    snapshots: Vec<SnapshotData>,
}

#[derive(Debug, Deserialize)]
struct SnapshotData {
    id: String,
    name: String,
    created_at: String,
    size_gigabytes: f64,
}

#[derive(Debug, Deserialize)]
struct ActionResponse {
    action: ActionData,
}

#[derive(Debug, Deserialize)]
struct ActionData {
    id: u64,
    status: String,
}

#[derive(Debug, Deserialize)]
struct SshKeysResponse {
    ssh_keys: Vec<SshKeyData>,
}

#[derive(Debug, Deserialize)]
struct SshKeyData {
    id: u64,
    #[allow(dead_code)]
    fingerprint: String,
}

impl DropletData {
    fn to_provider_instance(&self) -> ProviderInstance {
        let ip = self.get_public_ip().unwrap_or_else(|| "0.0.0.0".parse().unwrap());

        let created_at = DateTime::parse_from_rfc3339(&self.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to parse droplet created_at '{}': {}. Using current time.",
                    self.created_at,
                    e
                );
                Utc::now()
            });

        let status = match self.status.as_str() {
            "new" => InstanceStatus::New,
            "active" => InstanceStatus::Active,
            "off" => InstanceStatus::Off,
            "archive" => InstanceStatus::Archive,
            s => InstanceStatus::Unknown(s.to_string()),
        };

        ProviderInstance {
            id: self.id.to_string(),
            ip,
            status,
            created_at,
        }
    }

    fn get_public_ip(&self) -> Option<IpAddr> {
        self.networks
            .v4
            .iter()
            .find(|n| n.network_type == "public")
            .and_then(|n| n.ip_address.parse().ok())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_provider_requires_token() {
        let result = DigitalOceanProvider::new("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProviderError::Authentication { .. }));
    }

    #[test]
    fn test_provider_creates_with_valid_token() {
        let result = DigitalOceanProvider::new("test-token");
        assert!(result.is_ok());
    }

    #[test]
    fn test_auth_header() {
        let provider = DigitalOceanProvider::new("my-secret-token").unwrap();
        assert_eq!(provider.auth_header(), "Bearer my-secret-token");
    }

    #[test]
    fn test_resolve_image_ubuntu() {
        let provider = DigitalOceanProvider::new("token").unwrap();
        assert_eq!(
            provider.resolve_image(&ImageSpec::ubuntu("24.04")),
            "ubuntu-24-04-x64"
        );
        assert_eq!(
            provider.resolve_image(&ImageSpec::ubuntu("22.04")),
            "ubuntu-22-04-x64"
        );
    }

    #[test]
    fn test_resolve_image_debian() {
        let provider = DigitalOceanProvider::new("token").unwrap();
        assert_eq!(
            provider.resolve_image(&ImageSpec::debian("12")),
            "debian-12-x64"
        );
    }

    #[test]
    fn test_resolve_image_custom() {
        let provider = DigitalOceanProvider::new("token").unwrap();
        assert_eq!(
            provider.resolve_image(&ImageSpec::custom("my-custom-image")),
            "my-custom-image"
        );
    }

    #[test]
    fn test_droplet_data_to_instance_status_mapping() {
        let test_cases = vec![
            ("new", InstanceStatus::New),
            ("active", InstanceStatus::Active),
            ("off", InstanceStatus::Off),
            ("archive", InstanceStatus::Archive),
            ("unknown_status", InstanceStatus::Unknown("unknown_status".to_string())),
        ];

        for (status_str, expected_status) in test_cases {
            let droplet = DropletData {
                id: 123,
                status: status_str.to_string(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
                networks: Networks { v4: vec![] },
            };

            let instance = droplet.to_provider_instance();
            assert_eq!(instance.status, expected_status);
        }
    }

    #[test]
    fn test_droplet_data_extracts_public_ip() {
        let droplet = DropletData {
            id: 456,
            status: "active".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            networks: Networks {
                v4: vec![
                    NetworkV4 {
                        ip_address: "10.0.0.1".to_string(),
                        network_type: "private".to_string(),
                    },
                    NetworkV4 {
                        ip_address: "192.168.1.100".to_string(),
                        network_type: "public".to_string(),
                    },
                ],
            },
        };

        let instance = droplet.to_provider_instance();
        assert_eq!(instance.ip.to_string(), "192.168.1.100");
    }

    #[test]
    fn test_droplet_data_no_public_ip_returns_fallback() {
        let droplet = DropletData {
            id: 789,
            status: "new".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            networks: Networks { v4: vec![] },
        };

        let instance = droplet.to_provider_instance();
        assert_eq!(instance.ip.to_string(), "0.0.0.0");
    }

    #[tokio::test]
    async fn test_create_instance_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_keys": [{"id": 123456, "fingerprint": "aa:bb:cc"}]
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/droplets"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "droplet": {
                    "id": 999888,
                    "status": "new",
                    "created_at": "2024-01-01T12:00:00Z",
                    "networks": {"v4": []}
                }
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let request = InstanceRequest::new("test-instance", "nyc1", "s-2vcpu-4gb")
            .with_image(ImageSpec::ubuntu("24.04"))
            .with_label("spuff", "true");

        let result = provider.create_instance(&request).await;
        assert!(result.is_ok());

        let instance = result.unwrap();
        assert_eq!(instance.id, "999888");
        assert_eq!(instance.status, InstanceStatus::New);
    }

    #[tokio::test]
    async fn test_create_instance_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_keys": []
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/droplets"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("bad-token", &mock_server.uri()).unwrap();
        let request = InstanceRequest::new("test", "nyc1", "s-1vcpu-1gb");

        let result = provider.create_instance(&request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProviderError::Authentication { .. }));
    }

    #[tokio::test]
    async fn test_get_instance_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/droplets/12345"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "droplet": {
                    "id": 12345,
                    "status": "active",
                    "created_at": "2024-01-01T00:00:00Z",
                    "networks": {
                        "v4": [{"ip_address": "1.2.3.4", "type": "public"}]
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.get_instance("12345").await;

        assert!(result.is_ok());
        let instance = result.unwrap();
        assert!(instance.is_some());

        let instance = instance.unwrap();
        assert_eq!(instance.id, "12345");
        assert_eq!(instance.status, InstanceStatus::Active);
        assert_eq!(instance.ip.to_string(), "1.2.3.4");
    }

    #[tokio::test]
    async fn test_get_instance_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/droplets/99999"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.get_instance("99999").await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_destroy_instance_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/droplets/11111"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.destroy_instance("11111").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_destroy_instance_already_gone() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/droplets/22222"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.destroy_instance("22222").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_instances() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/droplets.*"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "droplets": [
                    {
                        "id": 111,
                        "status": "active",
                        "created_at": "2024-01-01T00:00:00Z",
                        "networks": {"v4": [{"ip_address": "1.1.1.1", "type": "public"}]}
                    },
                    {
                        "id": 222,
                        "status": "off",
                        "created_at": "2024-01-02T00:00:00Z",
                        "networks": {"v4": [{"ip_address": "2.2.2.2", "type": "public"}]}
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.list_instances().await;

        assert!(result.is_ok());
        let instances = result.unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "111");
        assert_eq!(instances[1].id, "222");
    }

    #[tokio::test]
    async fn test_list_snapshots() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/snapshots.*"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "snapshots": [
                    {
                        "id": "snap-001",
                        "name": "spuff-backup-1",
                        "created_at": "2024-01-01T00:00:00Z",
                        "size_gigabytes": 10.5
                    },
                    {
                        "id": "snap-002",
                        "name": "spuff-backup-2",
                        "created_at": "2024-01-02T00:00:00Z",
                        "size_gigabytes": 20.0
                    },
                    {
                        "id": "snap-003",
                        "name": "other-snapshot",
                        "created_at": "2024-01-03T00:00:00Z",
                        "size_gigabytes": 5.0
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.list_snapshots().await;

        assert!(result.is_ok());
        let snapshots = result.unwrap();
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots.iter().all(|s| s.name.starts_with("spuff")));
    }

    #[tokio::test]
    async fn test_delete_snapshot_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/snapshots/snap-123"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.delete_snapshot("snap-123").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_ssh_keys() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_keys": [
                    {"id": 100, "fingerprint": "aa:bb:cc"},
                    {"id": 200, "fingerprint": "dd:ee:ff"},
                    {"id": 300, "fingerprint": "11:22:33"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.get_ssh_key_ids().await;

        assert!(result.is_ok());
        let keys = result.unwrap();
        assert_eq!(keys.len(), 3);
        assert_eq!(keys, vec!["100", "200", "300"]);
    }

    #[tokio::test]
    async fn test_get_ssh_keys_empty() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_keys": []
            })))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.get_ssh_key_ids().await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_ssh_keys_api_error_returns_empty() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::with_base_url("test-token", &mock_server.uri()).unwrap();
        let result = provider.get_ssh_key_ids().await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_factory_creates_provider() {
        let factory = DigitalOceanFactory;
        assert_eq!(factory.provider_type(), ProviderType::DigitalOcean);
        assert!(factory.is_implemented());

        let result = factory.create("test-token", ProviderTimeouts::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_empty_token_fails() {
        let factory = DigitalOceanFactory;
        let result = factory.create("", ProviderTimeouts::default());
        assert!(matches!(result, Err(ProviderError::Authentication { .. })));
    }
}
