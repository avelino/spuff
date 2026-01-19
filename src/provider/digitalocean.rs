use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{Instance, InstanceConfig, InstanceStatus, Provider, Snapshot};
use crate::error::{Result, SpuffError};

const DEFAULT_API_BASE: &str = "https://api.digitalocean.com/v2";

#[derive(Debug)]
pub struct DigitalOceanProvider {
    client: Client,
    token: String,
    base_url: String,
}

impl DigitalOceanProvider {
    pub fn new(token: &str) -> Result<Self> {
        Self::with_base_url(token, DEFAULT_API_BASE)
    }

    pub fn with_base_url(token: &str, base_url: &str) -> Result<Self> {
        if token.is_empty() {
            return Err(SpuffError::Provider(
                "DigitalOcean API token is required. Set DIGITALOCEAN_TOKEN or configure via 'spuff init'".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            token: token.to_string(),
            base_url: base_url.to_string(),
        })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

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

// Used for list_instances deserialization
#[allow(dead_code)]
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

// Used for single snapshot responses (e.g., after creating a snapshot)
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SnapshotResponse {
    snapshot: SnapshotData,
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
    fn to_instance(&self) -> Result<Instance> {
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

        Ok(Instance {
            id: self.id.to_string(),
            ip,
            status,
            created_at,
        })
    }

    fn get_public_ip(&self) -> Option<IpAddr> {
        self.networks
            .v4
            .iter()
            .find(|n| n.network_type == "public")
            .and_then(|n| n.ip_address.parse().ok())
    }
}

#[async_trait]
impl Provider for DigitalOceanProvider {
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance> {
        let ssh_keys = self.get_ssh_key_ids().await?;

        let request = CreateDropletRequest {
            name: config.name.clone(),
            region: config.region.clone(),
            size: config.size.clone(),
            image: config.image.clone(),
            ssh_keys,
            user_data: config.user_data.clone(),
            tags: config.tags.clone(),
            monitoring: true,
        };

        let response = self
            .client
            .post(format!("{}/droplets", self.base_url))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to create droplet: {} - {}",
                status, body
            )));
        }

        let data: DropletResponse = response.json().await?;

        Ok(Instance {
            id: data.droplet.id.to_string(),
            ip: "0.0.0.0".parse().unwrap(),
            status: InstanceStatus::New,
            created_at: Utc::now(),
        })
    }

    async fn destroy_instance(&self, id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/droplets/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() && response.status().as_u16() != 404 {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to destroy droplet: {} - {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn get_instance(&self, id: &str) -> Result<Option<Instance>> {
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
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to get droplet: {} - {}",
                status, body
            )));
        }

        let data: DropletResponse = response.json().await?;
        Ok(Some(data.droplet.to_instance()?))
    }

    async fn list_instances(&self) -> Result<Vec<Instance>> {
        let response = self
            .client
            .get(format!("{}/droplets?tag_name=spuff", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to list droplets: {} - {}",
                status, body
            )));
        }

        let data: DropletsResponse = response.json().await?;
        data.droplets
            .into_iter()
            .map(|d| d.to_instance())
            .collect()
    }

    async fn wait_ready(&self, id: &str) -> Result<Instance> {
        let max_attempts = 60;
        let delay = Duration::from_secs(5);

        for _ in 0..max_attempts {
            if let Some(instance) = self.get_instance(id).await? {
                if instance.status == InstanceStatus::Active
                    && instance.ip.to_string() != "0.0.0.0"
                {
                    return Ok(instance);
                }
            }
            tokio::time::sleep(delay).await;
        }

        Err(SpuffError::InstanceNotReady(
            "Timeout waiting for instance".to_string(),
        ))
    }

    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot> {
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
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to create snapshot: {} - {}",
                status, body
            )));
        }

        let action: ActionResponse = response.json().await?;

        self.wait_for_action(action.action.id).await?;

        let snapshots = self.list_snapshots().await?;
        snapshots
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| SpuffError::Provider("Snapshot not found after creation".to_string()))
    }

    async fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        let response = self
            .client
            .get(format!("{}/snapshots?resource_type=droplet", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to list snapshots: {} - {}",
                status, body
            )));
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

    async fn delete_snapshot(&self, id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/snapshots/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !response.status().is_success() && response.status().as_u16() != 404 {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "Failed to delete snapshot: {} - {}",
                status, body
            )));
        }

        Ok(())
    }
}

impl DigitalOceanProvider {
    /// Fetches SSH key IDs from the DigitalOcean account.
    ///
    /// Returns an empty list if fetching fails, allowing instance creation
    /// to continue (the user can still SSH using their local key).
    async fn get_ssh_key_ids(&self) -> Result<Vec<String>> {
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

    async fn wait_for_action(&self, action_id: u64) -> Result<()> {
        let max_attempts = 120;
        let delay = Duration::from_secs(5);

        for _ in 0..max_attempts {
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
                        return Err(SpuffError::Provider("Action failed".to_string()));
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(delay).await;
        }

        Err(SpuffError::Provider(
            "Timeout waiting for action".to_string(),
        ))
    }
}

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
        assert!(err.to_string().contains("API token is required"));
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

            let instance = droplet.to_instance().unwrap();
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

        let instance = droplet.to_instance().unwrap();
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

        let instance = droplet.to_instance().unwrap();
        assert_eq!(instance.ip.to_string(), "0.0.0.0");
    }

    #[tokio::test]
    async fn test_create_instance_success() {
        let mock_server = MockServer::start().await;

        // Mock SSH keys endpoint
        Mock::given(method("GET"))
            .and(path("/account/keys"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_keys": [
                    {"id": 123456, "fingerprint": "aa:bb:cc"}
                ]
            })))
            .mount(&mock_server)
            .await;

        // Mock create droplet endpoint
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
        let config = InstanceConfig {
            name: "test-instance".to_string(),
            region: "nyc1".to_string(),
            size: "s-2vcpu-4gb".to_string(),
            image: "ubuntu-24-04-x64".to_string(),
            ssh_keys: vec![],
            user_data: None,
            tags: vec!["spuff".to_string()],
        };

        let result = provider.create_instance(&config).await;
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
        let config = InstanceConfig {
            name: "test".to_string(),
            region: "nyc1".to_string(),
            size: "s-1vcpu-1gb".to_string(),
            image: "ubuntu".to_string(),
            ssh_keys: vec![],
            user_data: None,
            tags: vec![],
        };

        let result = provider.create_instance(&config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to create droplet"));
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
                        "v4": [{
                            "ip_address": "1.2.3.4",
                            "type": "public"
                        }]
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

        // Should succeed even if droplet doesn't exist
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
        // Only spuff-* snapshots should be returned
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

        // Should return empty vec on error, not fail
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
