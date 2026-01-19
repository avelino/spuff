# Creating a New Provider

This guide teaches you step-by-step how to implement support for a new cloud provider in spuff. We'll use Hetzner Cloud as an example, but the concepts apply to any provider.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Architecture Overview](#architecture-overview)
3. [Step 1: Add the ProviderType](#step-1-add-the-providertype)
4. [Step 2: Create the Provider File](#step-2-create-the-provider-file)
5. [Step 3: Define API Structures](#step-3-define-api-structures)
6. [Step 4: Implement the Provider](#step-4-implement-the-provider)
7. [Step 5: Implement the Factory](#step-5-implement-the-factory)
8. [Step 6: Register in the Registry](#step-6-register-in-the-registry)
9. [Step 7: Write Tests](#step-7-write-tests)
10. [Final Checklist](#final-checklist)
11. [Common Pitfalls](#common-pitfalls)

---

## Prerequisites

Before starting, you need:

- Experience with Rust and async programming (`async`/`await`)
- Familiarity with the provider's API you're implementing
- API credentials for testing
- Read the [Provider README](README.md) to understand the architecture

---

## Architecture Overview

The system uses two main traits:

```
┌─────────────────────────────────────────────────────────────┐
│                     ProviderRegistry                         │
│                                                              │
│   HashMap<ProviderType, Arc<dyn ProviderFactory>>           │
│                                                              │
│   DigitalOceanFactory ──┐                                   │
│   HetznerFactory ───────┼──► create_by_name("hetzner")      │
│   AwsFactory ───────────┘           │                       │
│                                     ▼                       │
│                          Box<dyn Provider>                   │
└─────────────────────────────────────────────────────────────┘
```

**You need to implement:**

1. `ProviderFactory` - Creates instances of your provider
2. `Provider` - Implements cloud operations

---

## Step 1: Add the ProviderType

First, add the new provider to the `ProviderType` enum in `src/provider/config.rs`:

```rust
// src/provider/config.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    DigitalOcean,
    Hetzner,     // ← Add here
    Aws,
}

impl ProviderType {
    /// Returns whether the provider is implemented and ready to use
    pub fn is_implemented(&self) -> bool {
        matches!(self,
            Self::DigitalOcean
            | Self::Hetzner  // ← Add here when implemented
        )
    }

    /// Provider name as string (used in configs and CLI)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DigitalOcean => "digitalocean",
            Self::Hetzner => "hetzner",  // ← Add here
            Self::Aws => "aws",
        }
    }

    /// Environment variable for the API token
    pub fn token_env_var(&self) -> &'static str {
        match self {
            Self::DigitalOcean => "DIGITALOCEAN_TOKEN",
            Self::Hetzner => "HETZNER_TOKEN",  // ← Add here
            Self::Aws => "AWS_ACCESS_KEY_ID",
        }
    }
}
```

---

## Step 2: Create the Provider File

Create a new file for the provider:

```bash
touch src/provider/hetzner.rs
```

Initial file structure:

```rust
// src/provider/hetzner.rs

//! Hetzner Cloud Provider
//!
//! Provider implementation for Hetzner Cloud.
//! API Documentation: https://docs.hetzner.cloud/

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Instant;

use super::{
    ImageSpec, InstanceRequest, InstanceStatus, Provider, ProviderInstance,
    ProviderResult, Snapshot,
};
use super::config::ProviderTimeouts;
use super::error::ProviderError;
use super::registry::{ProviderFactory, ProviderType};

/// Hetzner Cloud API base URL
const API_BASE: &str = "https://api.hetzner.cloud/v1";

// =============================================================================
// Provider Struct
// =============================================================================

/// Hetzner Cloud provider
pub struct HetznerProvider {
    client: Client,
    token: String,
    base_url: String,
    timeouts: ProviderTimeouts,
}

// =============================================================================
// Factory Struct
// =============================================================================

/// Factory for creating HetznerProvider instances
pub struct HetznerFactory;

// ... implementations below
```

**Don't forget to declare the module in `src/provider/mod.rs`:**

```rust
// src/provider/mod.rs

mod config;
mod digitalocean;
mod error;
mod hetzner;      // ← Add here
mod registry;

pub use hetzner::{HetznerFactory, HetznerProvider};  // ← And here
```

---

## Step 3: Define API Structures

Define types for serializing/deserializing API responses. Hetzner uses JSON, so we use `serde`:

```rust
// src/provider/hetzner.rs (continued)

// =============================================================================
// API Request/Response Types
// =============================================================================

/// Request to create a server
#[derive(Debug, Serialize)]
struct CreateServerRequest<'a> {
    name: &'a str,
    server_type: &'a str,
    location: &'a str,
    image: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_data: Option<&'a str>,
    labels: std::collections::HashMap<String, String>,
    start_after_create: bool,
}

/// Response when creating a server
#[derive(Debug, Deserialize)]
struct CreateServerResponse {
    server: Server,
    action: Action,
}

/// Response when getting a server
#[derive(Debug, Deserialize)]
struct GetServerResponse {
    server: Server,
}

/// Response when listing servers
#[derive(Debug, Deserialize)]
struct ListServersResponse {
    servers: Vec<Server>,
}

/// Server representation in the API
#[derive(Debug, Deserialize)]
struct Server {
    id: u64,
    name: String,
    status: String,
    created: DateTime<Utc>,
    public_net: PublicNet,
    server_type: ServerType,
    datacenter: Datacenter,
    labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PublicNet {
    ipv4: Option<Ipv4Net>,
    ipv6: Option<Ipv6Net>,
}

#[derive(Debug, Deserialize)]
struct Ipv4Net {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct Ipv6Net {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct ServerType {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Datacenter {
    name: String,
    location: Location,
}

#[derive(Debug, Deserialize)]
struct Location {
    name: String,
}

/// Async API action
#[derive(Debug, Deserialize)]
struct Action {
    id: u64,
    status: String,
    progress: u32,
}

/// Action response
#[derive(Debug, Deserialize)]
struct ActionResponse {
    action: Action,
}

/// Request to create an image (snapshot)
#[derive(Debug, Serialize)]
struct CreateImageRequest<'a> {
    description: &'a str,
    r#type: &'a str,
    labels: std::collections::HashMap<String, String>,
}

/// Response when creating an image
#[derive(Debug, Deserialize)]
struct CreateImageResponse {
    image: Image,
    action: Action,
}

/// Response when listing images
#[derive(Debug, Deserialize)]
struct ListImagesResponse {
    images: Vec<Image>,
}

/// Image representation in the API
#[derive(Debug, Deserialize)]
struct Image {
    id: u64,
    description: Option<String>,
    image_size: Option<f64>,
    created: DateTime<Utc>,
}

/// API error response
#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    code: String,
    message: String,
}
```

**Tip:** You don't need to map all API fields. Only use what you actually need. Unknown fields are ignored by serde by default.

---

## Step 4: Implement the Provider

Now implement the `Provider` trait. I'll detail each method:

### 4.1 Constructor and Helpers

```rust
// src/provider/hetzner.rs (continued)

// =============================================================================
// Provider Implementation
// =============================================================================

impl HetznerProvider {
    /// Creates a new provider with default configuration
    pub fn new(token: &str, timeouts: ProviderTimeouts) -> ProviderResult<Self> {
        Self::with_config(token, API_BASE, timeouts)
    }

    /// Creates a provider with custom base URL (useful for tests)
    pub fn with_config(
        token: &str,
        base_url: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Self> {
        if token.is_empty() {
            return Err(ProviderError::invalid_config(
                "token",
                "API token cannot be empty",
            ));
        }

        let client = Client::builder()
            .timeout(timeouts.http_request)
            .build()
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self {
            client,
            token: token.to_string(),
            base_url: base_url.to_string(),
            timeouts,
        })
    }

    /// Makes an authenticated request to the API
    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<&impl Serialize>,
    ) -> ProviderResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);

        let mut request = self
            .client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json");

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await?;
        let status = response.status();

        // Handle specific HTTP errors
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(self.map_api_error(status.as_u16(), &body));
        }

        response.json().await.map_err(|e| ProviderError::Other {
            message: format!("Failed to parse response: {}", e),
        })
    }

    /// Maps API errors to ProviderError
    fn map_api_error(&self, status: u16, body: &str) -> ProviderError {
        // Try to parse structured API error
        if let Ok(api_error) = serde_json::from_str::<ApiError>(body) {
            match api_error.error.code.as_str() {
                "unauthorized" | "forbidden" => {
                    return ProviderError::auth("hetzner", api_error.error.message);
                }
                "rate_limit_exceeded" => {
                    return ProviderError::RateLimit {
                        retry_after: Some(std::time::Duration::from_secs(60)),
                    };
                }
                "not_found" => {
                    return ProviderError::NotFound {
                        resource_type: "resource".to_string(),
                        id: "unknown".to_string(),
                    };
                }
                _ => {}
            }
        }

        // Fallback based on HTTP status
        match status {
            401 | 403 => ProviderError::auth("hetzner", body.to_string()),
            404 => ProviderError::NotFound {
                resource_type: "resource".to_string(),
                id: "unknown".to_string(),
            },
            422 => ProviderError::InvalidConfig {
                field: "request".to_string(),
                message: body.to_string(),
            },
            429 => ProviderError::RateLimit {
                retry_after: Some(std::time::Duration::from_secs(60)),
            },
            _ => ProviderError::api(status, body.to_string()),
        }
    }

    /// Converts ImageSpec to Hetzner image slug
    fn resolve_image(&self, spec: &ImageSpec) -> String {
        match spec {
            ImageSpec::Ubuntu(version) => {
                // Hetzner uses slugs like "ubuntu-24.04"
                format!("ubuntu-{}", version)
            }
            ImageSpec::Debian(version) => {
                format!("debian-{}", version)
            }
            ImageSpec::Custom(id) => id.clone(),
            ImageSpec::Snapshot(id) => id.clone(),
        }
    }

    /// Maps Hetzner status to InstanceStatus
    fn map_status(status: &str) -> InstanceStatus {
        match status {
            "initializing" | "starting" => InstanceStatus::New,
            "running" => InstanceStatus::Active,
            "stopping" | "off" => InstanceStatus::Off,
            other => InstanceStatus::Unknown(other.to_string()),
        }
    }

    /// Extracts public IP from response
    fn extract_public_ip(public_net: &PublicNet) -> IpAddr {
        // Prefer IPv4, fallback to IPv6
        if let Some(ipv4) = &public_net.ipv4 {
            if let Ok(ip) = ipv4.ip.parse() {
                return ip;
            }
        }
        if let Some(ipv6) = &public_net.ipv6 {
            if let Ok(ip) = ipv6.ip.parse() {
                return ip;
            }
        }
        // Fallback: IP not assigned yet
        "0.0.0.0".parse().unwrap()
    }

    /// Waits for an async action to complete
    async fn wait_for_action(&self, action_id: u64) -> ProviderResult<()> {
        let start = Instant::now();
        let max_attempts = self.timeouts.action_complete_attempts();

        for _attempt in 0..max_attempts {
            if start.elapsed() > self.timeouts.action_complete {
                return Err(ProviderError::timeout(
                    "action_complete",
                    start.elapsed(),
                ));
            }

            let response: ActionResponse = self
                .request(
                    reqwest::Method::GET,
                    &format!("/actions/{}", action_id),
                    None::<&()>,
                )
                .await?;

            match response.action.status.as_str() {
                "success" => return Ok(()),
                "error" => {
                    return Err(ProviderError::api(
                        500,
                        format!("Action {} failed", action_id),
                    ));
                }
                _ => {
                    // "running" or "pending" - keep waiting
                    tokio::time::sleep(self.timeouts.poll_interval).await;
                }
            }
        }

        Err(ProviderError::timeout("action_complete", start.elapsed()))
    }
}
```

### 4.2 Implement the Provider Trait

```rust
// src/provider/hetzner.rs (continued)

#[async_trait]
impl Provider for HetznerProvider {
    /// Provider name for logs and identification
    fn name(&self) -> &'static str {
        "hetzner"
    }

    /// Creates a new instance
    ///
    /// IMPORTANT: This method returns immediately after the API accepts the request.
    /// The instance may still be creating. Use `wait_ready()` to wait.
    async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance> {
        // Build labels (Hetzner uses key-value, not array)
        let mut labels = request.labels.clone();
        labels.insert("managed-by".to_string(), "spuff".to_string());

        let api_request = CreateServerRequest {
            name: &request.name,
            server_type: &request.size,
            location: &request.region,
            image: &self.resolve_image(&request.image),
            user_data: request.user_data.as_deref(),
            labels,
            start_after_create: true,
        };

        let response: CreateServerResponse = self
            .request(reqwest::Method::POST, "/servers", Some(&api_request))
            .await?;

        Ok(ProviderInstance {
            id: response.server.id.to_string(),
            ip: Self::extract_public_ip(&response.server.public_net),
            status: Self::map_status(&response.server.status),
            created_at: response.server.created,
        })
    }

    /// Destroys an instance
    ///
    /// IMPORTANT: This method is idempotent. Calling on a non-existent instance
    /// returns Ok(()) instead of an error.
    async fn destroy_instance(&self, id: &str) -> ProviderResult<()> {
        match self
            .request::<serde_json::Value>(
                reqwest::Method::DELETE,
                &format!("/servers/{}", id),
                None::<&()>,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(ProviderError::NotFound { .. }) => Ok(()), // Idempotent
            Err(e) => Err(e),
        }
    }

    /// Gets instance details
    ///
    /// Returns None if the instance doesn't exist.
    async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>> {
        match self
            .request::<GetServerResponse>(
                reqwest::Method::GET,
                &format!("/servers/{}", id),
                None::<&()>,
            )
            .await
        {
            Ok(response) => Ok(Some(ProviderInstance {
                id: response.server.id.to_string(),
                ip: Self::extract_public_ip(&response.server.public_net),
                status: Self::map_status(&response.server.status),
                created_at: response.server.created,
            })),
            Err(ProviderError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Lists all spuff-managed instances
    async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>> {
        // Filter by managed-by=spuff label
        let response: ListServersResponse = self
            .request(
                reqwest::Method::GET,
                "/servers?label_selector=managed-by=spuff",
                None::<&()>,
            )
            .await?;

        Ok(response
            .servers
            .into_iter()
            .map(|server| ProviderInstance {
                id: server.id.to_string(),
                ip: Self::extract_public_ip(&server.public_net),
                status: Self::map_status(&server.status),
                created_at: server.created,
            })
            .collect())
    }

    /// Waits for instance to be ready (running + public IP)
    ///
    /// This method polls until:
    /// 1. Status is "Active"
    /// 2. Public IP is assigned (not 0.0.0.0)
    ///
    /// Default timeout: 5 minutes (configurable via ProviderTimeouts)
    async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance> {
        let start = Instant::now();
        let max_attempts = self.timeouts.instance_ready_attempts();

        for _attempt in 0..max_attempts {
            if start.elapsed() > self.timeouts.instance_ready {
                return Err(ProviderError::timeout("wait_ready", start.elapsed()));
            }

            if let Some(instance) = self.get_instance(id).await? {
                // Check if running AND has IP
                let has_ip = !instance.ip.is_unspecified();
                let is_active = matches!(instance.status, InstanceStatus::Active);

                if is_active && has_ip {
                    return Ok(instance);
                }
            }

            tokio::time::sleep(self.timeouts.poll_interval).await;
        }

        Err(ProviderError::timeout("wait_ready", start.elapsed()))
    }

    /// Creates a snapshot of the instance
    ///
    /// In Hetzner, snapshots are called "images" of type "snapshot".
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot> {
        let mut labels = std::collections::HashMap::new();
        labels.insert("managed-by".to_string(), "spuff".to_string());

        let api_request = CreateImageRequest {
            description: name,
            r#type: "snapshot",
            labels,
        };

        let response: CreateImageResponse = self
            .request(
                reqwest::Method::POST,
                &format!("/servers/{}/actions/create_image", instance_id),
                Some(&api_request),
            )
            .await?;

        // Wait for creation action to complete
        self.wait_for_action(response.action.id).await?;

        Ok(Snapshot {
            id: response.image.id.to_string(),
            name: name.to_string(),
            created_at: Some(response.image.created),
        })
    }

    /// Lists all spuff-managed snapshots
    async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>> {
        let response: ListImagesResponse = self
            .request(
                reqwest::Method::GET,
                "/images?type=snapshot&label_selector=managed-by=spuff",
                None::<&()>,
            )
            .await?;

        Ok(response
            .images
            .into_iter()
            .map(|image| Snapshot {
                id: image.id.to_string(),
                name: image.description.unwrap_or_default(),
                created_at: Some(image.created),
            })
            .collect())
    }

    /// Deletes a snapshot
    ///
    /// IMPORTANT: Idempotent - deleting a non-existent snapshot returns Ok(())
    async fn delete_snapshot(&self, id: &str) -> ProviderResult<()> {
        match self
            .request::<serde_json::Value>(
                reqwest::Method::DELETE,
                &format!("/images/{}", id),
                None::<&()>,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(ProviderError::NotFound { .. }) => Ok(()), // Idempotent
            Err(e) => Err(e),
        }
    }

    /// Returns whether the provider supports snapshots
    fn supports_snapshots(&self) -> bool {
        true
    }
}
```

---

## Step 5: Implement the Factory

The Factory is simple - it knows how to create provider instances:

```rust
// src/provider/hetzner.rs (continued)

// =============================================================================
// Factory Implementation
// =============================================================================

impl ProviderFactory for HetznerFactory {
    /// Type of provider this factory creates
    fn provider_type(&self) -> ProviderType {
        ProviderType::Hetzner
    }

    /// Creates a new provider instance
    fn create(
        &self,
        token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>> {
        Ok(Box::new(HetznerProvider::new(token, timeouts)?))
    }
}
```

---

## Step 6: Register in the Registry

Add the factory to the registry in `src/provider/registry.rs`:

```rust
// src/provider/registry.rs

use super::hetzner::HetznerFactory;  // ← Add import

impl ProviderRegistry {
    /// Creates a registry with all default providers registered
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register all implemented providers
        registry.register(DigitalOceanFactory);
        registry.register(HetznerFactory);  // ← Add here
        // registry.register(AwsFactory);  // When implemented

        registry
    }
}
```

---

## Step 7: Write Tests

See [testing-providers.md](testing-providers.md) for the complete testing guide. Here's a basic example:

```rust
// src/provider/hetzner.rs (continued)

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path, header};

    async fn create_test_provider(mock_server: &MockServer) -> HetznerProvider {
        HetznerProvider::with_config(
            "test-token",
            &mock_server.uri(),
            ProviderTimeouts::default(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_create_instance_success() {
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
                    "public_net": { "ipv4": null, "ipv6": null },
                    "server_type": { "name": "cx11" },
                    "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                    "labels": {}
                },
                "action": { "id": 1, "status": "running", "progress": 0 }
            })))
            .mount(&mock_server)
            .await;

        let provider = create_test_provider(&mock_server).await;
        let request = InstanceRequest {
            name: "spuff-test".to_string(),
            region: "fsn1".to_string(),
            size: "cx11".to_string(),
            image: ImageSpec::Ubuntu("24.04".to_string()),
            user_data: None,
            labels: std::collections::HashMap::new(),
        };

        let result = provider.create_instance(&request).await;
        assert!(result.is_ok());

        let instance = result.unwrap();
        assert_eq!(instance.id, "123");
        assert!(matches!(instance.status, InstanceStatus::New));
    }

    #[tokio::test]
    async fn test_destroy_instance_not_found_is_ok() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/servers/999"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": { "code": "not_found", "message": "Server not found" }
            })))
            .mount(&mock_server)
            .await;

        let provider = create_test_provider(&mock_server).await;

        // Should return Ok even with 404 (idempotent)
        let result = provider.destroy_instance("999").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_authentication_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": { "code": "unauthorized", "message": "Invalid token" }
            })))
            .mount(&mock_server)
            .await;

        let provider = create_test_provider(&mock_server).await;
        let result = provider.list_instances().await;

        assert!(matches!(
            result,
            Err(ProviderError::Authentication { .. })
        ));
    }
}
```

---

## Final Checklist

Before submitting your PR, verify:

### Code

- [ ] All `Provider` trait methods implemented
- [ ] `ProviderFactory` implemented and registered
- [ ] `ProviderType` added to enum
- [ ] Module declared in `mod.rs`
- [ ] Error handling using `ProviderError`
- [ ] `destroy_instance` and `delete_snapshot` are idempotent
- [ ] Timeouts respected in `wait_ready` and long operations

### Tests

- [ ] Unit tests with mock server
- [ ] Success case test for each method
- [ ] Error tests (authentication, not found, rate limit)
- [ ] Idempotency test for destroy/delete

### Documentation

- [ ] Docstrings on all public methods
- [ ] Example configuration added
- [ ] README updated with new provider

---

## Common Pitfalls

### 1. Image Mapping

Each provider uses different conventions for identifying images:

| Provider | Format | Example |
|----------|--------|---------|
| DigitalOcean | Slug | `ubuntu-24-04-x64` |
| Hetzner | Slug | `ubuntu-24.04` |
| AWS | AMI ID | `ami-0c55b159cbfafe1f0` |

**Solution:** Implement `resolve_image()` correctly for each provider.

### 2. Public IP Extraction

Providers return IPs in different ways:

- Some in network array
- Some in specific field
- Some with separate IPv4 and IPv6

**Solution:** Always prefer IPv4 and handle the case when IP is not yet assigned (use `0.0.0.0`).

### 3. Labels vs Tags

- **DigitalOcean:** Tags are string array
- **Hetzner/AWS:** Labels are key-value pairs

**Solution:** Convert `HashMap<String, String>` to the format the provider expects.

### 4. Async Actions

Some operations (especially snapshots) are asynchronous:

1. API returns an action ID
2. You need to poll until complete

**Solution:** Implement `wait_for_action()` with timeout.

### 5. Rate Limiting

Each provider has different limits. Hetzner is more restrictive than DigitalOcean.

**Solution:**
- Return `ProviderError::RateLimit` with `retry_after`
- Respect the `poll_interval` from timeouts

### 6. User Data Encoding

- **DigitalOcean:** Accepts raw string
- **Hetzner:** Accepts raw string
- **AWS:** Requires base64

**Solution:** Do the necessary encoding in `create_instance()`.

---

## Need Help?

- Review the DigitalOcean implementation (`src/provider/digitalocean.rs`) as reference
- Open an issue to discuss before starting
- Consult the provider ADR to understand design decisions
