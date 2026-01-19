# Creating a Provider

This guide walks you through implementing a new cloud provider for spuff.

## Prerequisites

- Rust experience
- Familiarity with the cloud provider's API
- API credentials for testing

## Step 1: Create the Provider File

Create a new file in `src/provider/`:

```bash
touch src/provider/hetzner.rs
```

## Step 2: Basic Structure

Start with the basic structure:

```rust
// src/provider/hetzner.rs

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{Result, SpuffError};
use super::{Instance, InstanceConfig, InstanceStatus, Provider, Snapshot};

const API_BASE: &str = "https://api.hetzner.cloud/v1";

pub struct HetznerProvider {
    client: Client,
    token: String,
}

impl HetznerProvider {
    pub fn new(token: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| SpuffError::Provider(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            token: token.to_string(),
        })
    }

    /// Helper to make authenticated requests
    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<impl Serialize>,
    ) -> Result<T> {
        let url = format!("{}{}", API_BASE, endpoint);

        let mut req = self.client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json");

        if let Some(body) = body {
            req = req.json(&body);
        }

        let response = req.send().await
            .map_err(|e| SpuffError::Provider(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpuffError::Provider(format!(
                "API error {}: {}",
                status, body
            )));
        }

        response.json().await
            .map_err(|e| SpuffError::Provider(format!("Failed to parse response: {}", e)))
    }
}
```

## Step 3: Implement the Provider Trait

Implement each method of the `Provider` trait:

```rust
#[async_trait]
impl Provider for HetznerProvider {
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance> {
        // Map spuff config to Hetzner API request
        let request = CreateServerRequest {
            name: &config.name,
            server_type: &config.size,
            location: &config.region,
            image: &config.image,
            ssh_keys: &config.ssh_keys,
            user_data: config.user_data.as_deref(),
            labels: config.tags.iter()
                .map(|t| (t.clone(), "true".to_string()))
                .collect(),
        };

        let response: CreateServerResponse = self
            .request(reqwest::Method::POST, "/servers", Some(&request))
            .await?;

        Ok(Instance {
            id: response.server.id.to_string(),
            name: response.server.name,
            ip: response.server.public_net.ipv4.map(|n| n.ip).unwrap_or_default(),
            status: map_status(&response.server.status),
            region: config.region.clone(),
            size: config.size.clone(),
            created_at: response.server.created,
        })
    }

    async fn destroy_instance(&self, id: &str) -> Result<()> {
        self.request::<serde_json::Value>(
            reqwest::Method::DELETE,
            &format!("/servers/{}", id),
            None::<()>,
        ).await?;

        Ok(())
    }

    async fn get_instance(&self, id: &str) -> Result<Option<Instance>> {
        match self.request::<GetServerResponse>(
            reqwest::Method::GET,
            &format!("/servers/{}", id),
            None::<()>,
        ).await {
            Ok(response) => Ok(Some(map_server_to_instance(response.server))),
            Err(e) if e.to_string().contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn list_instances(&self) -> Result<Vec<Instance>> {
        let response: ListServersResponse = self
            .request(
                reqwest::Method::GET,
                "/servers?label_selector=spuff",
                None::<()>,
            )
            .await?;

        Ok(response.servers.into_iter().map(map_server_to_instance).collect())
    }

    async fn wait_ready(&self, id: &str) -> Result<Instance> {
        let timeout = std::time::Duration::from_secs(300);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(SpuffError::Timeout(
                    "Instance did not become ready in time".to_string()
                ));
            }

            if let Some(instance) = self.get_instance(id).await? {
                if instance.status == InstanceStatus::Running && !instance.ip.is_empty() {
                    return Ok(instance);
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot> {
        // Hetzner uses "images" for snapshots
        let request = CreateImageRequest {
            description: Some(name),
            labels: Some([("spuff".to_string(), "true".to_string())].into()),
            r#type: "snapshot",
        };

        let response: CreateImageResponse = self
            .request(
                reqwest::Method::POST,
                &format!("/servers/{}/actions/create_image", instance_id),
                Some(&request),
            )
            .await?;

        Ok(Snapshot {
            id: response.image.id.to_string(),
            name: name.to_string(),
            size_gb: response.image.image_size.unwrap_or(0) as u64,
            created_at: response.image.created,
            regions: vec![], // Hetzner snapshots are region-specific
        })
    }

    async fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        let response: ListImagesResponse = self
            .request(
                reqwest::Method::GET,
                "/images?type=snapshot&label_selector=spuff",
                None::<()>,
            )
            .await?;

        Ok(response.images.into_iter().map(|img| Snapshot {
            id: img.id.to_string(),
            name: img.description.unwrap_or_default(),
            size_gb: img.image_size.unwrap_or(0) as u64,
            created_at: img.created,
            regions: vec![],
        }).collect())
    }

    async fn delete_snapshot(&self, id: &str) -> Result<()> {
        self.request::<serde_json::Value>(
            reqwest::Method::DELETE,
            &format!("/images/{}", id),
            None::<()>,
        ).await?;

        Ok(())
    }
}
```

## Step 4: API Response Types

Define the API response types:

```rust
// Request/Response types for Hetzner API

#[derive(Serialize)]
struct CreateServerRequest<'a> {
    name: &'a str,
    server_type: &'a str,
    location: &'a str,
    image: &'a str,
    ssh_keys: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    user_data: Option<&'a str>,
    labels: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct CreateServerResponse {
    server: Server,
}

#[derive(Deserialize)]
struct GetServerResponse {
    server: Server,
}

#[derive(Deserialize)]
struct ListServersResponse {
    servers: Vec<Server>,
}

#[derive(Deserialize)]
struct Server {
    id: u64,
    name: String,
    status: String,
    created: chrono::DateTime<chrono::Utc>,
    public_net: PublicNet,
    server_type: ServerType,
    datacenter: Datacenter,
}

#[derive(Deserialize)]
struct PublicNet {
    ipv4: Option<Ipv4>,
}

#[derive(Deserialize)]
struct Ipv4 {
    ip: String,
}

// Helper function to map Hetzner status to spuff status
fn map_status(status: &str) -> InstanceStatus {
    match status {
        "running" => InstanceStatus::Running,
        "initializing" | "starting" => InstanceStatus::Starting,
        "stopping" | "off" => InstanceStatus::Stopped,
        _ => InstanceStatus::Unknown,
    }
}

fn map_server_to_instance(server: Server) -> Instance {
    Instance {
        id: server.id.to_string(),
        name: server.name,
        ip: server.public_net.ipv4.map(|n| n.ip).unwrap_or_default(),
        status: map_status(&server.status),
        region: server.datacenter.name,
        size: server.server_type.name,
        created_at: server.created,
    }
}
```

## Step 5: Register the Provider

Add your provider to `src/provider/mod.rs`:

```rust
mod digitalocean;
mod hetzner;  // Add this

pub use digitalocean::DigitalOceanProvider;
pub use hetzner::HetznerProvider;  // Add this

pub fn create_provider(config: &AppConfig) -> Result<Box<dyn Provider>> {
    match config.provider.as_str() {
        "digitalocean" => {
            let token = get_api_token("DIGITALOCEAN_TOKEN")?;
            Ok(Box::new(DigitalOceanProvider::new(&token)?))
        }
        "hetzner" => {  // Add this block
            let token = get_api_token("HETZNER_TOKEN")?;
            Ok(Box::new(HetznerProvider::new(&token)?))
        }
        _ => Err(SpuffError::Config(format!(
            "Unknown provider: {}",
            config.provider
        ))),
    }
}
```

## Step 6: Add Configuration Support

Update `src/config.rs` to recognize the new provider:

```rust
// In validate() function
fn validate(&self) -> Result<()> {
    match self.provider.as_str() {
        "digitalocean" | "hetzner" => Ok(()),  // Add hetzner
        _ => Err(SpuffError::Config(format!(
            "Unknown provider: {}. Supported: digitalocean, hetzner",
            self.provider
        ))),
    }
}
```

## Step 7: Update Documentation

1. Add to `docs/configuration.md`:
   - Provider name and status
   - Required environment variables
   - Supported regions and sizes

2. Add to `README.md`:
   - Feature table
   - Quick start section

## Step 8: Write Tests

Create tests in the same file or a separate test file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path, header};

    #[tokio::test]
    async fn test_create_instance() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "server": {
                    "id": 123,
                    "name": "spuff-test",
                    "status": "initializing",
                    "created": "2024-01-01T00:00:00Z",
                    "public_net": { "ipv4": null },
                    "server_type": { "name": "cx11" },
                    "datacenter": { "name": "fsn1-dc14" }
                }
            })))
            .mount(&mock_server)
            .await;

        // Create provider pointing to mock server
        let provider = HetznerProvider::new_with_base_url(
            "test-token",
            &mock_server.uri(),
        ).unwrap();

        let config = InstanceConfig {
            name: "spuff-test".to_string(),
            region: "fsn1".to_string(),
            size: "cx11".to_string(),
            image: "ubuntu-24.04".to_string(),
            ssh_keys: vec!["key-123".to_string()],
            user_data: None,
            tags: vec!["spuff".to_string()],
        };

        let instance = provider.create_instance(&config).await.unwrap();

        assert_eq!(instance.id, "123");
        assert_eq!(instance.name, "spuff-test");
    }
}
```

## Checklist

Before submitting your provider:

- [ ] All `Provider` trait methods implemented
- [ ] Error handling with meaningful messages
- [ ] Timeout handling for long operations
- [ ] Unit tests with mocked API
- [ ] Integration tests (optional, requires credentials)
- [ ] Documentation updated
- [ ] Configuration validation added
- [ ] Example configuration provided

## Common Pitfalls

1. **Rate limiting**: Add retry logic with exponential backoff
2. **Region/size mapping**: Cloud providers use different naming conventions
3. **SSH key format**: Some providers want key IDs, others want fingerprints
4. **Async polling**: Don't poll too frequently, respect API limits
5. **Error messages**: Include API response body in errors for debugging

## Getting Help

- Review the DigitalOcean implementation as reference
- Open an issue for questions
- Join the discussion before major implementation work
