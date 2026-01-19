# Testing Providers

This guide covers testing strategies for cloud provider implementations.

## Testing Levels

```
┌─────────────────────────────────────────────────────────────┐
│                    Integration Tests                         │
│              (Real API, real resources)                      │
│                    cargo test --ignored                      │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │
┌─────────────────────────────────────────────────────────────┐
│                      Unit Tests                              │
│               (Mocked API responses)                         │
│                       cargo test                             │
└─────────────────────────────────────────────────────────────┘
```

## Unit Tests with Mocked API

### Setup

Add test dependencies to `Cargo.toml`:

```toml
[dev-dependencies]
wiremock = "0.6"
tokio-test = "0.4"
serde_json = "1.0"
```

### Creating a Testable Provider

Add a constructor that accepts a custom base URL:

```rust
impl HetznerProvider {
    /// Creates a provider with production URL
    pub fn new(token: &str, timeouts: ProviderTimeouts) -> ProviderResult<Self> {
        Self::with_config(token, API_BASE, timeouts)
    }

    /// Creates a provider with custom base URL (for testing)
    pub fn with_config(
        token: &str,
        base_url: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Self> {
        // ... implementation
    }
}
```

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path, header, body_json};

    // Helper to create test config
    fn test_request() -> InstanceRequest {
        InstanceRequest {
            name: "spuff-test".to_string(),
            region: "fsn1".to_string(),
            size: "cx11".to_string(),
            image: ImageSpec::Ubuntu("24.04".to_string()),
            user_data: None,
            labels: HashMap::new(),
        }
    }

    // Helper to create mock provider
    async fn create_mock_provider(mock_server: &MockServer) -> HetznerProvider {
        HetznerProvider::with_config(
            "test-token",
            &mock_server.uri(),
            ProviderTimeouts::default(),
        )
        .unwrap()
    }
}
```

---

## Testing Each Method

### Testing create_instance

```rust
#[tokio::test]
async fn test_create_instance_success() {
    let mock_server = MockServer::start().await;

    // Setup mock
    Mock::given(method("POST"))
        .and(path("/servers"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "initializing",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": null,
                    "ipv6": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            },
            "action": { "id": 1, "status": "running", "progress": 0 }
        })))
        .expect(1) // Verify called exactly once
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.create_instance(&test_request()).await;

    assert!(result.is_ok());
    let instance = result.unwrap();
    assert_eq!(instance.id, "123");
    assert_eq!(instance.status, InstanceStatus::New);
}

#[tokio::test]
async fn test_create_instance_api_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/servers"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "error": {
                "code": "invalid_input",
                "message": "Invalid server type"
            }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.create_instance(&test_request()).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProviderError::InvalidConfig { .. }));
}

#[tokio::test]
async fn test_create_instance_network_error() {
    // Use an invalid URL to simulate network error
    let provider = HetznerProvider::with_config(
        "test-token",
        "http://localhost:99999",
        ProviderTimeouts::default(),
    ).unwrap();

    let result = provider.create_instance(&test_request()).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProviderError::Network(_)));
}
```

### Testing destroy_instance

```rust
#[tokio::test]
async fn test_destroy_instance_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.destroy_instance("123").await;

    assert!(result.is_ok());
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

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.destroy_instance("999").await;

    // Idempotent: 404 should return Ok
    assert!(result.is_ok());
}
```

### Testing get_instance

```rust
#[tokio::test]
async fn test_get_instance_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "running",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": { "ip": "1.2.3.4" },
                    "ipv6": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.get_instance("123").await;

    assert!(result.is_ok());
    let instance = result.unwrap();
    assert!(instance.is_some());
    let instance = instance.unwrap();
    assert_eq!(instance.ip.to_string(), "1.2.3.4");
    assert_eq!(instance.status, InstanceStatus::Active);
}

#[tokio::test]
async fn test_get_instance_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": { "code": "not_found", "message": "Server not found" }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.get_instance("999").await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}
```

### Testing wait_ready

```rust
#[tokio::test]
async fn test_wait_ready_immediate() {
    let mock_server = MockServer::start().await;

    // Instance is already ready
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "running",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": { "ip": "1.2.3.4" },
                    "ipv6": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.wait_ready("123").await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().ip.to_string(), "1.2.3.4");
}

#[tokio::test]
async fn test_wait_ready_multiple_polls() {
    let mock_server = MockServer::start().await;

    // First call: still starting, no IP
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "initializing",
                "created": "2024-01-01T00:00:00Z",
                "public_net": { "ipv4": null, "ipv6": null },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            }
        })))
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;

    // Subsequent calls: ready with IP
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "running",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": { "ip": "1.2.3.4" },
                    "ipv6": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            }
        })))
        .mount(&mock_server)
        .await;

    // Use fast timeouts for testing
    let timeouts = ProviderTimeouts {
        poll_interval: Duration::from_millis(10),
        instance_ready: Duration::from_secs(5),
        ..Default::default()
    };

    let provider = HetznerProvider::with_config(
        "test-token",
        &mock_server.uri(),
        timeouts,
    ).unwrap();

    let result = provider.wait_ready("123").await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().ip.to_string(), "1.2.3.4");
}

#[tokio::test]
async fn test_wait_ready_timeout() {
    let mock_server = MockServer::start().await;

    // Always returns not ready
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "initializing",
                "created": "2024-01-01T00:00:00Z",
                "public_net": { "ipv4": null, "ipv6": null },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": {}
            }
        })))
        .mount(&mock_server)
        .await;

    // Very short timeout for testing
    let timeouts = ProviderTimeouts {
        poll_interval: Duration::from_millis(10),
        instance_ready: Duration::from_millis(50),
        ..Default::default()
    };

    let provider = HetznerProvider::with_config(
        "test-token",
        &mock_server.uri(),
        timeouts,
    ).unwrap();

    let result = provider.wait_ready("123").await;

    assert!(matches!(result.unwrap_err(), ProviderError::Timeout { .. }));
}
```

### Testing list_instances

```rust
#[tokio::test]
async fn test_list_instances() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers"))
        .and(wiremock::matchers::query_param("label_selector", "managed-by=spuff"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "servers": [
                {
                    "id": 1,
                    "name": "spuff-one",
                    "status": "running",
                    "created": "2024-01-01T00:00:00Z",
                    "public_net": { "ipv4": { "ip": "1.1.1.1" }, "ipv6": null },
                    "server_type": { "name": "cx11" },
                    "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                    "labels": { "managed-by": "spuff" }
                },
                {
                    "id": 2,
                    "name": "spuff-two",
                    "status": "running",
                    "created": "2024-01-01T00:00:00Z",
                    "public_net": { "ipv4": { "ip": "2.2.2.2" }, "ipv6": null },
                    "server_type": { "name": "cx21" },
                    "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                    "labels": { "managed-by": "spuff" }
                }
            ]
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.list_instances().await;

    assert!(result.is_ok());
    let instances = result.unwrap();
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[0].id, "1");
    assert_eq!(instances[1].id, "2");
}

#[tokio::test]
async fn test_list_instances_empty() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "servers": []
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.list_instances().await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}
```

### Testing Authentication Errors

```rust
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

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.list_instances().await;

    assert!(matches!(
        result.unwrap_err(),
        ProviderError::Authentication { .. }
    ));
}

#[tokio::test]
async fn test_rate_limit_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": { "code": "rate_limit_exceeded", "message": "Too many requests" }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.list_instances().await;

    let err = result.unwrap_err();
    assert!(matches!(err, ProviderError::RateLimit { .. }));
    assert!(err.is_retryable());
    assert!(err.retry_after().is_some());
}
```

---

## Integration Tests

Integration tests use real API credentials and create real resources.

### Setup

```rust
// tests/integration/provider_test.rs

use spuff::provider::{Provider, InstanceRequest, ImageSpec, create_provider};
use std::env;

fn skip_if_no_credentials() -> bool {
    if env::var("HETZNER_TOKEN").is_err() {
        eprintln!("Skipping: HETZNER_TOKEN not set");
        return true;
    }
    false
}

fn create_test_provider() -> Box<dyn Provider> {
    let token = env::var("HETZNER_TOKEN").expect("HETZNER_TOKEN required");
    let registry = ProviderRegistry::with_defaults();
    registry.create_by_name("hetzner", &token, ProviderTimeouts::default())
        .expect("Failed to create provider")
}
```

### Full Lifecycle Test

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_full_instance_lifecycle() {
    if skip_if_no_credentials() {
        return;
    }

    let provider = create_test_provider();

    // Create instance
    let request = InstanceRequest {
        name: format!("spuff-test-{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
        region: "fsn1".to_string(),
        size: "cx11".to_string(),
        image: ImageSpec::Ubuntu("24.04".to_string()),
        user_data: None,
        labels: HashMap::from([
            ("managed-by".to_string(), "spuff".to_string()),
            ("test".to_string(), "true".to_string()),
        ]),
    };

    println!("Creating instance: {}", request.name);
    let instance = provider.create_instance(&request).await
        .expect("Failed to create instance");

    println!("Instance ID: {}", instance.id);

    // Wait for ready
    println!("Waiting for instance to be ready...");
    let ready = provider.wait_ready(&instance.id).await
        .expect("Instance never became ready");

    println!("Instance ready at IP: {}", ready.ip);
    assert!(!ready.ip.is_unspecified());
    assert_eq!(ready.status, InstanceStatus::Active);

    // List instances
    let instances = provider.list_instances().await
        .expect("Failed to list instances");
    assert!(instances.iter().any(|i| i.id == instance.id));

    // Create snapshot (if supported)
    if provider.supports_snapshots() {
        println!("Creating snapshot...");
        let snapshot = provider.create_snapshot(&instance.id, "test-snapshot").await
            .expect("Failed to create snapshot");

        println!("Snapshot ID: {}", snapshot.id);

        // List snapshots
        let snapshots = provider.list_snapshots().await
            .expect("Failed to list snapshots");
        assert!(snapshots.iter().any(|s| s.id == snapshot.id));

        // Delete snapshot
        println!("Deleting snapshot...");
        provider.delete_snapshot(&snapshot.id).await
            .expect("Failed to delete snapshot");
    }

    // Cleanup: Destroy instance
    println!("Destroying instance...");
    provider.destroy_instance(&instance.id).await
        .expect("Failed to destroy instance");

    println!("Test complete!");
}
```

### Running Integration Tests

```bash
# Set credentials
export HETZNER_TOKEN="your-api-token"

# Run integration tests
cargo test -- --ignored

# Run specific integration test
cargo test test_full_instance_lifecycle -- --ignored

# Run with output
cargo test test_full_instance_lifecycle -- --ignored --nocapture
```

---

## Test Utilities

### Fixtures Module

Create reusable test fixtures:

```rust
// tests/fixtures/mod.rs

pub mod responses {
    use serde_json::json;

    pub fn server_running(id: u64, ip: &str) -> serde_json::Value {
        json!({
            "server": {
                "id": id,
                "name": format!("spuff-test-{}", id),
                "status": "running",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": { "ip": ip },
                    "ipv6": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": { "managed-by": "spuff" }
            }
        })
    }

    pub fn server_starting(id: u64) -> serde_json::Value {
        json!({
            "server": {
                "id": id,
                "name": format!("spuff-test-{}", id),
                "status": "initializing",
                "created": "2024-01-01T00:00:00Z",
                "public_net": { "ipv4": null, "ipv6": null },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14", "location": { "name": "fsn1" } },
                "labels": { "managed-by": "spuff" }
            }
        })
    }

    pub fn error_not_found() -> serde_json::Value {
        json!({
            "error": { "code": "not_found", "message": "Resource not found" }
        })
    }

    pub fn error_unauthorized() -> serde_json::Value {
        json!({
            "error": { "code": "unauthorized", "message": "Invalid token" }
        })
    }

    pub fn error_rate_limit() -> serde_json::Value {
        json!({
            "error": { "code": "rate_limit_exceeded", "message": "Too many requests" }
        })
    }
}
```

### Test Coverage

Check test coverage:

```bash
# Install cargo-tarpaulin
cargo install cargo-tarpaulin

# Run with coverage
cargo tarpaulin --out Html

# Open coverage report
open tarpaulin-report.html
```

---

## CI Integration

### GitHub Actions

```yaml
# .github/workflows/test.yml
name: Tests

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all

  integration-tests:
    runs-on: ubuntu-latest
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test -- --ignored
        env:
          DIGITALOCEAN_TOKEN: ${{ secrets.DIGITALOCEAN_TOKEN }}
          HETZNER_TOKEN: ${{ secrets.HETZNER_TOKEN }}
```

---

## Test Checklist

Before submitting provider tests:

- [ ] All Provider trait methods have unit tests
- [ ] Happy path tested for each method
- [ ] Error cases tested (API errors, network errors)
- [ ] Edge cases tested (not found, already exists)
- [ ] Idempotency tested for destroy/delete operations
- [ ] Timeout handling tested
- [ ] Rate limit handling tested
- [ ] Authentication error handling tested
- [ ] Integration test for full lifecycle (optional but recommended)
- [ ] Mock server used for unit tests
- [ ] Tests are deterministic (no flaky tests)
- [ ] Cleanup happens even on test failure
