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
```

### Creating a Testable Provider

Add a constructor that accepts a custom base URL:

```rust
impl HetznerProvider {
    pub fn new(token: &str) -> Result<Self> {
        Self::new_with_base_url(token, API_BASE)
    }

    pub fn new_with_base_url(token: &str, base_url: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            token: token.to_string(),
            base_url: base_url.to_string(),
        })
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
    fn test_config() -> InstanceConfig {
        InstanceConfig {
            name: "spuff-test".to_string(),
            region: "fsn1".to_string(),
            size: "cx11".to_string(),
            image: "ubuntu-24.04".to_string(),
            ssh_keys: vec!["key-123".to_string()],
            user_data: None,
            tags: vec!["spuff".to_string()],
        }
    }

    // Helper to create mock provider
    async fn create_mock_provider(mock_server: &MockServer) -> HetznerProvider {
        HetznerProvider::new_with_base_url("test-token", &mock_server.uri()).unwrap()
    }
}
```

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
                    "ipv4": null
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14" }
            }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.create_instance(&test_config()).await;

    assert!(result.is_ok());
    let instance = result.unwrap();
    assert_eq!(instance.id, "123");
    assert_eq!(instance.name, "spuff-test");
    assert_eq!(instance.status, InstanceStatus::Starting);
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
    let result = provider.create_instance(&test_config()).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("422"));
}

#[tokio::test]
async fn test_create_instance_network_error() {
    // Use an invalid URL to simulate network error
    let provider = HetznerProvider::new_with_base_url(
        "test-token",
        "http://localhost:99999",
    ).unwrap();

    let result = provider.create_instance(&test_config()).await;

    assert!(result.is_err());
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
async fn test_destroy_instance_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/servers/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": { "code": "not_found" }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.destroy_instance("999").await;

    // Decide if 404 on destroy is an error or success
    // Many providers treat this as success (idempotent delete)
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
                    "ipv4": { "ip": "1.2.3.4" }
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14" }
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
    assert_eq!(instance.ip, "1.2.3.4");
    assert_eq!(instance.status, InstanceStatus::Running);
}

#[tokio::test]
async fn test_get_instance_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers/999"))
        .respond_with(ResponseTemplate::new(404))
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
                    "ipv4": { "ip": "1.2.3.4" }
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14" }
            }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.wait_ready("123").await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_ready_multiple_polls() {
    let mock_server = MockServer::start().await;

    // First call: still starting
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "status": "initializing",
                "public_net": { "ipv4": null },
                // ... other fields
            }
        })))
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;

    // Subsequent calls: ready
    Mock::given(method("GET"))
        .and(path("/servers/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "server": {
                "id": 123,
                "status": "running",
                "public_net": {
                    "ipv4": { "ip": "1.2.3.4" }
                },
                // ... other fields
            }
        })))
        .mount(&mock_server)
        .await;

    let provider = create_mock_provider(&mock_server).await;
    let result = provider.wait_ready("123").await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().ip, "1.2.3.4");
}
```

### Testing list_instances

```rust
#[tokio::test]
async fn test_list_instances() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/servers"))
        .and(wiremock::matchers::query_param("label_selector", "spuff"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "servers": [
                {
                    "id": 1,
                    "name": "spuff-one",
                    "status": "running",
                    // ...
                },
                {
                    "id": 2,
                    "name": "spuff-two",
                    "status": "running",
                    // ...
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
}
```

---

## Integration Tests

Integration tests use real API credentials and create real resources.

### Setup

```rust
// tests/integration/provider_test.rs

use spuff::provider::{Provider, InstanceConfig, create_provider};
use std::env;

fn skip_if_no_credentials() {
    if env::var("HETZNER_TOKEN").is_err() {
        eprintln!("Skipping: HETZNER_TOKEN not set");
        return;
    }
}

fn create_test_provider() -> Box<dyn Provider> {
    create_provider(&test_config()).expect("Failed to create provider")
}
```

### Full Lifecycle Test

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_full_instance_lifecycle() {
    skip_if_no_credentials();

    let provider = create_test_provider();

    // Create instance
    let config = InstanceConfig {
        name: format!("spuff-test-{}", uuid::Uuid::new_v4().to_string()[..8]),
        region: "fsn1".to_string(),
        size: "cx11".to_string(),
        image: "ubuntu-24.04".to_string(),
        ssh_keys: vec![], // Use account default
        user_data: None,
        tags: vec!["spuff".to_string(), "test".to_string()],
    };

    println!("Creating instance: {}", config.name);
    let instance = provider.create_instance(&config).await
        .expect("Failed to create instance");

    println!("Instance ID: {}", instance.id);

    // Wait for ready
    println!("Waiting for instance to be ready...");
    let ready = provider.wait_ready(&instance.id).await
        .expect("Instance never became ready");

    println!("Instance ready at IP: {}", ready.ip);
    assert!(!ready.ip.is_empty());

    // Create snapshot
    println!("Creating snapshot...");
    let snapshot = provider.create_snapshot(&instance.id, "test-snapshot").await
        .expect("Failed to create snapshot");

    println!("Snapshot ID: {}", snapshot.id);

    // List instances
    let instances = provider.list_instances().await
        .expect("Failed to list instances");
    assert!(instances.iter().any(|i| i.id == instance.id));

    // List snapshots
    let snapshots = provider.list_snapshots().await
        .expect("Failed to list snapshots");
    assert!(snapshots.iter().any(|s| s.id == snapshot.id));

    // Cleanup
    println!("Cleaning up...");

    provider.delete_snapshot(&snapshot.id).await
        .expect("Failed to delete snapshot");

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
    pub fn server_running() -> serde_json::Value {
        serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "running",
                "created": "2024-01-01T00:00:00Z",
                "public_net": {
                    "ipv4": { "ip": "1.2.3.4" }
                },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14" }
            }
        })
    }

    pub fn server_starting() -> serde_json::Value {
        serde_json::json!({
            "server": {
                "id": 123,
                "name": "spuff-test",
                "status": "initializing",
                "created": "2024-01-01T00:00:00Z",
                "public_net": { "ipv4": null },
                "server_type": { "name": "cx11" },
                "datacenter": { "name": "fsn1-dc14" }
            }
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

## Checklist

Before submitting provider tests:

- [ ] All Provider trait methods have unit tests
- [ ] Happy path tested
- [ ] Error cases tested (API errors, network errors)
- [ ] Edge cases tested (not found, already exists)
- [ ] Integration test for full lifecycle
- [ ] Mock server used for unit tests
- [ ] Tests are deterministic (no flaky tests)
- [ ] Cleanup happens even on test failure
