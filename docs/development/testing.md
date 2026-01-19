# Testing Guide

This guide covers testing strategies, tools, and best practices for spuff.

## Test Organization

```
spuff/
├── src/
│   ├── provider/
│   │   └── digitalocean.rs  # Unit tests in #[cfg(test)] mod
│   └── ...
└── tests/
    └── integration/         # Integration tests
```

## Running Tests

### All Tests

```bash
# Run all unit tests
cargo test --all

# Run with output
cargo test --all -- --nocapture

# Run specific test
cargo test test_name

# Run tests for a specific crate
cargo test -p spuff
```

### Integration Tests

Integration tests require cloud credentials:

```bash
# Run integration tests (marked with #[ignore])
cargo test -- --ignored

# Run all tests including integration
cargo test --all -- --include-ignored
```

## Unit Testing

### Testing Provider Methods

Use `wiremock` for API mocking:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path};

    #[tokio::test]
    async fn test_create_instance() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v2/droplets"))
            .respond_with(ResponseTemplate::new(202)
                .set_body_json(create_droplet_response()))
            .mount(&mock_server)
            .await;

        let provider = DigitalOceanProvider::new_with_base_url(
            "test-token",
            &mock_server.uri(),
        ).unwrap();

        let result = provider.create_instance(&test_config()).await;
        assert!(result.is_ok());
    }
}
```

### Testing SSH Functions

Mock the SSH command:

```rust
#[cfg(test)]
mod tests {
    // Use test doubles or feature flags for SSH testing
    // Real SSH tests should be integration tests
}
```

### Testing Cloud-Init Generation

```rust
#[test]
fn test_cloud_init_generation() {
    let config = CloudInitConfig {
        username: "dev".to_string(),
        ssh_public_key: "ssh-ed25519 AAAA...".to_string(),
        // ...
    };

    let yaml = generate_cloud_init(&config).unwrap();

    assert!(yaml.contains("username: dev"));
    assert!(yaml.contains("ssh-ed25519"));
}
```

### Testing Configuration

```rust
#[test]
fn test_config_loading() {
    let yaml = r#"
provider: digitalocean
region: nyc1
size: s-2vcpu-4gb
"#;

    let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.provider, "digitalocean");
}

#[test]
fn test_config_validation() {
    let invalid_config = AppConfig {
        provider: "unknown".to_string(),
        // ...
    };

    let result = invalid_config.validate();
    assert!(result.is_err());
}
```

## Integration Testing

### Full Lifecycle Test

```rust
// tests/integration/lifecycle_test.rs

#[tokio::test]
#[ignore] // Requires credentials
async fn test_full_lifecycle() {
    let provider = create_test_provider();

    // Create
    let instance = provider.create_instance(&test_config()).await
        .expect("Failed to create");

    // Wait
    let ready = provider.wait_ready(&instance.id).await
        .expect("Failed to wait");
    assert!(!ready.ip.is_empty());

    // Destroy
    provider.destroy_instance(&instance.id).await
        .expect("Failed to destroy");
}
```

### SSH Integration Test

```rust
#[tokio::test]
#[ignore]
async fn test_ssh_connection() {
    // Create instance
    let instance = create_test_instance().await;

    // Test SSH
    let result = run_command(&instance.ip, &config, "echo hello").await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("hello"));

    // Cleanup
    destroy_test_instance(&instance.id).await;
}
```

## Test Utilities

### Fixtures

```rust
// tests/fixtures.rs

pub fn test_config() -> InstanceConfig {
    InstanceConfig {
        name: format!("spuff-test-{}", uuid::Uuid::new_v4()),
        region: "nyc1".to_string(),
        size: "s-1vcpu-1gb".to_string(),
        image: "ubuntu-24-04-x64".to_string(),
        ssh_keys: vec![],
        user_data: None,
        tags: vec!["spuff".to_string(), "test".to_string()],
    }
}

pub fn mock_droplet_response() -> serde_json::Value {
    serde_json::json!({
        "droplet": {
            "id": 12345,
            "name": "spuff-test",
            "status": "active",
            // ...
        }
    })
}
```

### Test Helpers

```rust
// tests/helpers.rs

pub async fn with_test_instance<F, Fut>(test: F)
where
    F: FnOnce(Instance) -> Fut,
    Fut: Future<Output = ()>,
{
    let provider = create_test_provider();
    let instance = provider.create_instance(&test_config()).await.unwrap();

    test(instance.clone()).await;

    // Always cleanup
    let _ = provider.destroy_instance(&instance.id).await;
}
```

## Mocking

### HTTP Mocking with wiremock

```rust
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path, header, body_json};

async fn setup_mock_server() -> MockServer {
    let server = MockServer::start().await;

    // Mock create droplet
    Mock::given(method("POST"))
        .and(path("/v2/droplets"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(202)
            .set_body_json(mock_droplet_response()))
        .mount(&server)
        .await;

    server
}
```

### Mocking Time

For timeout tests:

```rust
use tokio::time::{pause, advance};

#[tokio::test]
async fn test_timeout() {
    pause(); // Enable time control

    let start = tokio::time::Instant::now();

    // Advance time by 5 minutes
    advance(std::time::Duration::from_secs(300)).await;

    // Test timeout behavior
}
```

## Test Coverage

### Generate Coverage Report

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate HTML report
cargo tarpaulin --out Html

# Open report
open tarpaulin-report.html
```

### Coverage Goals

- Unit tests: 80%+ coverage
- Critical paths (provider, SSH): 90%+ coverage
- Integration tests for all happy paths

## Best Practices

### Test Naming

```rust
#[test]
fn test_create_instance_success() { }

#[test]
fn test_create_instance_invalid_region() { }

#[test]
fn test_create_instance_api_error() { }
```

### Test Structure (Arrange-Act-Assert)

```rust
#[test]
fn test_example() {
    // Arrange
    let config = test_config();
    let provider = MockProvider::new();

    // Act
    let result = provider.create_instance(&config);

    // Assert
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name, config.name);
}
```

### Avoiding Flaky Tests

1. Don't rely on timing
2. Use deterministic test data
3. Clean up resources in tests
4. Isolate tests from each other

### Test Documentation

```rust
/// Tests that creating an instance with an invalid region
/// returns a descriptive error message.
#[test]
fn test_create_instance_invalid_region() {
    // ...
}
```

## CI Integration

Tests run automatically on PR:

```yaml
# .github/workflows/test.yml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all
```

## Running Specific Test Categories

```bash
# Unit tests only
cargo test --lib

# Integration tests only
cargo test --test '*'

# Tests for specific module
cargo test provider::

# Tests matching pattern
cargo test ssh
```
