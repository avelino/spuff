# Provider API Reference

Complete reference for the Provider trait and associated types.

## Provider Trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Create a new cloud instance
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance>;

    /// Destroy an instance by ID
    async fn destroy_instance(&self, id: &str) -> Result<()>;

    /// Get instance details by ID
    async fn get_instance(&self, id: &str) -> Result<Option<Instance>>;

    /// List all spuff-managed instances
    async fn list_instances(&self) -> Result<Vec<Instance>>;

    /// Wait for instance to be ready (running + has IP)
    async fn wait_ready(&self, id: &str) -> Result<Instance>;

    /// Create a snapshot of an instance
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot>;

    /// List all spuff snapshots
    async fn list_snapshots(&self) -> Result<Vec<Snapshot>>;

    /// Delete a snapshot by ID
    async fn delete_snapshot(&self, id: &str) -> Result<()>;
}
```

---

## Types

### InstanceConfig

Configuration for creating a new instance.

```rust
pub struct InstanceConfig {
    /// Unique instance name (e.g., "spuff-a1b2c3d4")
    pub name: String,

    /// Region/datacenter identifier
    pub region: String,

    /// Instance size/type identifier
    pub size: String,

    /// Base image identifier (e.g., "ubuntu-24-04-x64")
    pub image: String,

    /// SSH key identifiers (IDs or fingerprints)
    pub ssh_keys: Vec<String>,

    /// Cloud-init user data (base64-encoded YAML)
    pub user_data: Option<String>,

    /// Tags for identifying spuff instances
    pub tags: Vec<String>,
}
```

**Notes:**

- `name` should be unique within the account
- `user_data` is base64-encoded cloud-init YAML
- `tags` should include "spuff" for filtering

### Instance

Represents a cloud instance.

```rust
pub struct Instance {
    /// Provider-specific instance ID
    pub id: String,

    /// Instance name
    pub name: String,

    /// Public IPv4 address
    pub ip: String,

    /// Current status
    pub status: InstanceStatus,

    /// Region where instance is running
    pub region: String,

    /// Instance size/type
    pub size: String,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}
```

### InstanceStatus

Enum representing instance states.

```rust
pub enum InstanceStatus {
    /// Instance is being created
    Starting,

    /// Instance is running and accessible
    Running,

    /// Instance is being stopped
    Stopping,

    /// Instance is stopped
    Stopped,

    /// Status cannot be determined
    Unknown,
}
```

**Status mapping guidelines:**

| Provider State | Spuff Status |
|----------------|--------------|
| new, initializing, starting | `Starting` |
| active, running | `Running` |
| stopping, shutting-down | `Stopping` |
| off, stopped, terminated | `Stopped` |
| (anything else) | `Unknown` |

### Snapshot

Represents a saved instance state.

```rust
pub struct Snapshot {
    /// Provider-specific snapshot ID
    pub id: String,

    /// Snapshot name/description
    pub name: String,

    /// Size in gigabytes
    pub size_gb: u64,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Regions where snapshot is available
    pub regions: Vec<String>,
}
```

---

## Method Specifications

### create_instance

Creates a new cloud instance with the specified configuration.

**Parameters:**

- `config: &InstanceConfig` - Instance configuration

**Returns:**

- `Result<Instance>` - Created instance (may not be ready yet)

**Behavior:**

1. Send API request to create instance
2. Return immediately with instance metadata
3. Instance may still be initializing
4. Use `wait_ready()` to wait for full availability

**Errors:**

- API authentication failure
- Invalid configuration (region, size, image)
- Quota exceeded
- Network errors

**Example:**

```rust
let config = InstanceConfig {
    name: "spuff-abc123".to_string(),
    region: "nyc1".to_string(),
    size: "s-2vcpu-4gb".to_string(),
    image: "ubuntu-24-04-x64".to_string(),
    ssh_keys: vec!["12345".to_string()],
    user_data: Some(base64_cloud_init),
    tags: vec!["spuff".to_string()],
};

let instance = provider.create_instance(&config).await?;
println!("Created instance: {}", instance.id);
```

---

### destroy_instance

Destroys an instance by ID.

**Parameters:**

- `id: &str` - Instance ID to destroy

**Returns:**

- `Result<()>` - Success or error

**Behavior:**

1. Send delete request to provider API
2. Do not wait for deletion to complete
3. Return success when deletion is initiated

**Errors:**

- Instance not found (may or may not be an error)
- API authentication failure
- Network errors

**Example:**

```rust
provider.destroy_instance("12345678").await?;
println!("Instance destruction initiated");
```

---

### get_instance

Gets instance details by ID.

**Parameters:**

- `id: &str` - Instance ID

**Returns:**

- `Result<Option<Instance>>` - Instance if found, None if not exists

**Behavior:**

1. Query provider API for instance
2. Return `None` if instance doesn't exist (404)
3. Return instance details if found

**Example:**

```rust
match provider.get_instance("12345678").await? {
    Some(instance) => println!("Found: {} ({})", instance.name, instance.ip),
    None => println!("Instance not found"),
}
```

---

### list_instances

Lists all spuff-managed instances.

**Returns:**

- `Result<Vec<Instance>>` - List of instances

**Behavior:**

1. Query provider API with spuff tag/label filter
2. Return all matching instances
3. Handle pagination if needed

**Filter requirements:**

- Only return instances tagged with "spuff"
- This prevents listing unrelated instances

**Example:**

```rust
let instances = provider.list_instances().await?;
for instance in instances {
    println!("{}: {} ({})", instance.name, instance.ip, instance.status);
}
```

---

### wait_ready

Waits for instance to be fully ready.

**Parameters:**

- `id: &str` - Instance ID

**Returns:**

- `Result<Instance>` - Ready instance with IP address

**Behavior:**

1. Poll `get_instance()` periodically
2. Check for `Running` status AND non-empty IP
3. Return when both conditions met
4. Timeout after 5 minutes

**Polling:**

- Poll every 5 seconds
- Respect provider rate limits
- Log progress if verbose mode enabled

**Example:**

```rust
let instance = provider.create_instance(&config).await?;
println!("Waiting for instance to be ready...");

let ready = provider.wait_ready(&instance.id).await?;
println!("Instance ready at {}", ready.ip);
```

---

### create_snapshot

Creates a snapshot of an instance.

**Parameters:**

- `instance_id: &str` - Instance to snapshot
- `name: &str` - Snapshot name/description

**Returns:**

- `Result<Snapshot>` - Created snapshot

**Behavior:**

1. Initiate snapshot creation
2. May need to wait for completion (provider-specific)
3. Tag snapshot with "spuff" for filtering
4. Return snapshot metadata

**Example:**

```rust
let snapshot = provider.create_snapshot("12345678", "pre-upgrade").await?;
println!("Created snapshot: {} ({}GB)", snapshot.id, snapshot.size_gb);
```

---

### list_snapshots

Lists all spuff snapshots.

**Returns:**

- `Result<Vec<Snapshot>>` - List of snapshots

**Behavior:**

1. Query provider API with spuff filter
2. Return only spuff-tagged snapshots
3. Handle pagination if needed

**Example:**

```rust
let snapshots = provider.list_snapshots().await?;
for snap in snapshots {
    println!("{}: {} ({}GB)", snap.id, snap.name, snap.size_gb);
}
```

---

### delete_snapshot

Deletes a snapshot by ID.

**Parameters:**

- `id: &str` - Snapshot ID

**Returns:**

- `Result<()>` - Success or error

**Example:**

```rust
provider.delete_snapshot("snap-12345").await?;
println!("Snapshot deleted");
```

---

## Error Handling

### Error Types

Providers should return `SpuffError` variants:

```rust
pub enum SpuffError {
    /// Provider API errors
    Provider(String),

    /// Configuration errors
    Config(String),

    /// Timeout errors
    Timeout(String),

    /// Network errors
    Network(String),
}
```

### Error Guidelines

1. **Include context**: Error messages should include what operation failed and why
2. **Include API response**: Include the API error body when available
3. **Don't panic**: Always return `Result`, never panic
4. **Retriable vs fatal**: Indicate if error is retriable

**Good error messages:**

```rust
Err(SpuffError::Provider(format!(
    "Failed to create instance '{}' in region '{}': {} - {}",
    config.name, config.region, status, response_body
)))
```

**Bad error messages:**

```rust
Err(SpuffError::Provider("API error".to_string()))
```

---

## Testing

### Mock Server Testing

Use `wiremock` for API mocking:

```rust
#[tokio::test]
async fn test_create_instance() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v2/droplets"))
        .respond_with(ResponseTemplate::new(202)
            .set_body_json(create_response()))
        .mount(&mock_server)
        .await;

    let provider = DigitalOceanProvider::new_with_base_url(
        "test-token",
        &mock_server.uri(),
    )?;

    let result = provider.create_instance(&config).await;
    assert!(result.is_ok());
}
```

### Integration Testing

For real API testing (requires credentials):

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn integration_test_lifecycle() {
    let provider = create_test_provider();

    // Create
    let instance = provider.create_instance(&test_config()).await?;

    // Wait ready
    let ready = provider.wait_ready(&instance.id).await?;
    assert!(!ready.ip.is_empty());

    // Destroy
    provider.destroy_instance(&instance.id).await?;
}
```
