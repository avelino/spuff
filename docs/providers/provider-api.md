# Provider API Reference

Complete reference for the Provider trait and associated types.

## Provider Trait

The `Provider` trait defines the contract that all cloud providers must implement:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Returns the provider name for logging and identification
    fn name(&self) -> &'static str;

    /// Creates a new cloud instance
    async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance>;

    /// Destroys an instance by ID (must be idempotent)
    async fn destroy_instance(&self, id: &str) -> ProviderResult<()>;

    /// Gets instance details by ID, returns None if not found
    async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>>;

    /// Lists all spuff-managed instances
    async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>>;

    /// Waits for instance to be ready (running + has IP)
    async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance>;

    /// Creates a snapshot of an instance
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot>;

    /// Lists all spuff-managed snapshots
    async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>>;

    /// Deletes a snapshot by ID (must be idempotent)
    async fn delete_snapshot(&self, id: &str) -> ProviderResult<()>;

    /// Returns SSH key identifiers (optional, default returns empty)
    async fn get_ssh_keys(&self) -> ProviderResult<Vec<String>> { Ok(vec![]) }

    /// Returns whether this provider supports snapshots
    fn supports_snapshots(&self) -> bool { true }
}
```

---

## Core Types

### InstanceRequest

Configuration for creating a new instance. This is provider-agnostic - each provider translates it to their API format.

```rust
pub struct InstanceRequest {
    /// Unique instance name (e.g., "spuff-a1b2c3d4")
    pub name: String,

    /// Region/datacenter identifier (provider-specific)
    pub region: String,

    /// Instance size/type identifier (provider-specific)
    pub size: String,

    /// Base image specification
    pub image: ImageSpec,

    /// Cloud-init user data script (raw YAML, not base64)
    pub user_data: Option<String>,

    /// Labels/tags for identifying spuff instances
    pub labels: HashMap<String, String>,
}
```

**Notes:**
- `name` should be unique within the account
- `user_data` is raw cloud-init YAML - providers handle encoding if needed
- `labels` should include identifiers for filtering (e.g., `managed-by: spuff`)

### ImageSpec

Provider-agnostic image specification:

```rust
pub enum ImageSpec {
    /// Ubuntu version (e.g., "24.04")
    /// Provider maps to appropriate slug/ID
    Ubuntu(String),

    /// Debian version (e.g., "12")
    Debian(String),

    /// Provider-specific image ID/slug
    Custom(String),

    /// Snapshot ID to restore from
    Snapshot(String),
}
```

**Provider Mapping Examples:**

| ImageSpec | DigitalOcean | Hetzner | AWS |
|-----------|--------------|---------|-----|
| `Ubuntu("24.04")` | `ubuntu-24-04-x64` | `ubuntu-24.04` | `ami-xxx` (lookup) |
| `Debian("12")` | `debian-12-x64` | `debian-12` | `ami-xxx` (lookup) |
| `Custom(id)` | pass through | pass through | pass through |
| `Snapshot(id)` | pass through | pass through | pass through |

### ProviderInstance

Represents a cloud instance:

```rust
pub struct ProviderInstance {
    /// Provider-specific instance ID
    pub id: String,

    /// Public IP address (or 0.0.0.0 if not yet assigned)
    pub ip: IpAddr,

    /// Current instance status
    pub status: InstanceStatus,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}
```

### InstanceStatus

Enum representing instance states:

```rust
pub enum InstanceStatus {
    /// Instance is being created
    New,

    /// Instance is running and accessible
    Active,

    /// Instance is powered off
    Off,

    /// Instance is stopped/archived
    Archive,

    /// Provider-specific status not mapped
    Unknown(String),
}
```

**Status Mapping Guidelines:**

| Provider State | Spuff Status |
|----------------|--------------|
| new, initializing, starting, pending | `New` |
| active, running | `Active` |
| stopping, off, stopped | `Off` |
| archive, terminated | `Archive` |
| (anything else) | `Unknown(state)` |

### Snapshot

Represents a saved instance state:

```rust
pub struct Snapshot {
    /// Provider-specific snapshot ID
    pub id: String,

    /// Snapshot name/description
    pub name: String,

    /// Creation timestamp (optional - some providers don't return this)
    pub created_at: Option<DateTime<Utc>>,
}
```

### ProviderTimeouts

Configurable timeout values for provider operations:

```rust
pub struct ProviderTimeouts {
    /// Maximum time to wait for instance to be ready
    /// Default: 300 seconds (5 minutes)
    pub instance_ready: Duration,

    /// Maximum time to wait for an action to complete
    /// Default: 600 seconds (10 minutes)
    pub action_complete: Duration,

    /// Interval between polling requests
    /// Default: 5 seconds
    pub poll_interval: Duration,

    /// Timeout for individual HTTP requests
    /// Default: 30 seconds
    pub http_request: Duration,

    /// Timeout for SSH connection attempts
    /// Default: 300 seconds (5 minutes)
    pub ssh_connect: Duration,

    /// Timeout for cloud-init to complete
    /// Default: 600 seconds (10 minutes)
    pub cloud_init: Duration,
}
```

**Helper Methods:**

```rust
impl ProviderTimeouts {
    /// Returns max attempts for instance ready polling
    pub fn instance_ready_attempts(&self) -> u32 {
        (self.instance_ready.as_secs() / self.poll_interval.as_secs()) as u32
    }

    /// Returns max attempts for action complete polling
    pub fn action_complete_attempts(&self) -> u32 {
        (self.action_complete.as_secs() / self.poll_interval.as_secs()) as u32
    }
}
```

---

## Error Types

### ProviderError

Structured error types for proper handling and retry logic:

```rust
pub enum ProviderError {
    /// Authentication failed (invalid or expired token)
    Authentication {
        provider: String,
        message: String,
    },

    /// Rate limit exceeded - should retry after duration
    RateLimit {
        retry_after: Option<Duration>,
    },

    /// Resource not found
    NotFound {
        resource_type: String,
        id: String,
    },

    /// Quota/limit exceeded (e.g., max droplets)
    QuotaExceeded {
        resource: String,
        message: String,
    },

    /// Invalid configuration
    InvalidConfig {
        field: String,
        message: String,
    },

    /// Feature not supported by this provider
    NotSupported {
        feature: String,
    },

    /// Operation timed out
    Timeout {
        operation: String,
        elapsed: Duration,
    },

    /// Network/HTTP error
    Network(#[from] reqwest::Error),

    /// API error with status code
    Api {
        status: u16,
        message: String,
    },

    /// Provider not implemented yet
    NotImplemented {
        name: String,
    },

    /// Unknown provider name
    UnknownProvider {
        name: String,
        supported: Vec<String>,
    },

    /// Generic error
    Other {
        message: String,
    },
}
```

**Helper Constructors:**

```rust
ProviderError::auth(provider, message)       // Creates Authentication error
ProviderError::not_found(resource_type, id)  // Creates NotFound error
ProviderError::api(status_code, message)     // Creates Api error
ProviderError::timeout(operation, elapsed)   // Creates Timeout error
ProviderError::quota(resource, message)      // Creates QuotaExceeded error
ProviderError::invalid_config(field, message) // Creates InvalidConfig error
```

**Retry Logic:**

```rust
impl ProviderError {
    /// Returns true if this error is potentially retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self,
            Self::RateLimit { .. } |
            Self::Timeout { .. } |
            Self::Network(_)
        )
    }

    /// Returns duration to wait before retrying, if applicable
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit { retry_after } => *retry_after,
            Self::Timeout { .. } => Some(Duration::from_secs(5)),
            Self::Network(_) => Some(Duration::from_secs(1)),
            _ => None,
        }
    }
}
```

---

## Method Specifications

### name

Returns the provider name for logging and identification.

```rust
fn name(&self) -> &'static str;
```

**Returns:** Static string with provider name (e.g., `"digitalocean"`, `"hetzner"`)

---

### create_instance

Creates a new cloud instance with the specified configuration.

```rust
async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance>;
```

**Parameters:**
- `request: &InstanceRequest` - Instance configuration

**Returns:**
- `ProviderResult<ProviderInstance>` - Created instance (may not be ready yet)

**Behavior:**
1. Translate `InstanceRequest` to provider-specific API request
2. Resolve `ImageSpec` to provider-specific image ID/slug
3. Send API request to create instance
4. Return immediately with instance metadata
5. Instance may still be initializing - use `wait_ready()` to wait

**Error Cases:**
- `Authentication` - Invalid API token
- `InvalidConfig` - Invalid region, size, or image
- `QuotaExceeded` - Account limit reached
- `Api` - Other API errors

**Example:**

```rust
let request = InstanceRequest {
    name: "spuff-abc123".to_string(),
    region: "nyc1".to_string(),
    size: "s-2vcpu-4gb".to_string(),
    image: ImageSpec::Ubuntu("24.04".to_string()),
    user_data: Some(cloud_init_script),
    labels: HashMap::from([("managed-by".to_string(), "spuff".to_string())]),
};

let instance = provider.create_instance(&request).await?;
println!("Created instance: {} (status: {:?})", instance.id, instance.status);

// Wait for it to be ready
let ready = provider.wait_ready(&instance.id).await?;
println!("Instance ready at: {}", ready.ip);
```

---

### destroy_instance

Destroys an instance by ID.

```rust
async fn destroy_instance(&self, id: &str) -> ProviderResult<()>;
```

**Parameters:**
- `id: &str` - Instance ID to destroy

**Returns:**
- `ProviderResult<()>` - Success or error

**Behavior:**
1. Send delete request to provider API
2. Do not wait for deletion to complete
3. **MUST be idempotent**: Return `Ok(())` if instance doesn't exist (404)

**Example:**

```rust
// Safe to call multiple times
provider.destroy_instance("12345678").await?;
provider.destroy_instance("12345678").await?; // Still returns Ok(())
```

---

### get_instance

Gets instance details by ID.

```rust
async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>>;
```

**Parameters:**
- `id: &str` - Instance ID

**Returns:**
- `ProviderResult<Option<ProviderInstance>>` - Instance if found, `None` if not exists

**Behavior:**
1. Query provider API for instance
2. Return `None` if instance doesn't exist (404)
3. Return instance details if found

**Example:**

```rust
match provider.get_instance("12345678").await? {
    Some(instance) => println!("Found: {} at {}", instance.id, instance.ip),
    None => println!("Instance not found"),
}
```

---

### list_instances

Lists all spuff-managed instances.

```rust
async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>>;
```

**Returns:**
- `ProviderResult<Vec<ProviderInstance>>` - List of instances

**Behavior:**
1. Query provider API with spuff label/tag filter
2. Return only instances tagged with spuff identifiers
3. Handle pagination if needed

**Filter Requirements:**
- Only return instances with `managed-by: spuff` label (or equivalent)
- This prevents listing unrelated instances in the account

---

### wait_ready

Waits for instance to be fully ready.

```rust
async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance>;
```

**Parameters:**
- `id: &str` - Instance ID

**Returns:**
- `ProviderResult<ProviderInstance>` - Ready instance with IP address

**Behavior:**
1. Poll `get_instance()` periodically
2. Check for `Active` status AND non-unspecified IP
3. Return when both conditions are met
4. Timeout after `ProviderTimeouts::instance_ready`

**Ready Conditions:**
- `status == InstanceStatus::Active`
- `ip.is_unspecified() == false` (not 0.0.0.0)

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

```rust
async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot>;
```

**Parameters:**
- `instance_id: &str` - Instance to snapshot
- `name: &str` - Snapshot name/description

**Returns:**
- `ProviderResult<Snapshot>` - Created snapshot

**Behavior:**
1. Initiate snapshot creation
2. Wait for completion if the operation is async (using action polling)
3. Tag snapshot with spuff identifiers for filtering
4. Return snapshot metadata

**Note:** Some providers (like Hetzner) have async snapshot creation that returns an action ID. The implementation should wait for the action to complete before returning.

---

### list_snapshots

Lists all spuff-managed snapshots.

```rust
async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>>;
```

**Returns:**
- `ProviderResult<Vec<Snapshot>>` - List of snapshots

**Behavior:**
1. Query provider API with spuff filter
2. Return only spuff-tagged snapshots
3. Handle pagination if needed

---

### delete_snapshot

Deletes a snapshot by ID.

```rust
async fn delete_snapshot(&self, id: &str) -> ProviderResult<()>;
```

**Parameters:**
- `id: &str` - Snapshot ID

**Returns:**
- `ProviderResult<()>` - Success or error

**Behavior:**
- **MUST be idempotent**: Return `Ok(())` if snapshot doesn't exist (404)

---

## ProviderFactory Trait

The factory trait for creating provider instances:

```rust
pub trait ProviderFactory: Send + Sync {
    /// Returns the type of provider this factory creates
    fn provider_type(&self) -> ProviderType;

    /// Creates a new provider instance
    fn create(
        &self,
        token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>>;

    /// Returns whether this provider is implemented
    fn is_implemented(&self) -> bool {
        self.provider_type().is_implemented()
    }
}
```

---

## ProviderRegistry

Registry for managing provider factories:

```rust
pub struct ProviderRegistry {
    factories: HashMap<ProviderType, Arc<dyn ProviderFactory>>,
}

impl ProviderRegistry {
    /// Creates an empty registry
    pub fn new() -> Self;

    /// Creates a registry with all default providers registered
    pub fn with_defaults() -> Self;

    /// Registers a provider factory
    pub fn register<F: ProviderFactory + 'static>(&mut self, factory: F);

    /// Creates a provider by name
    pub fn create_by_name(
        &self,
        name: &str,
        token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>>;

    /// Returns list of registered provider types
    pub fn registered_providers(&self) -> Vec<ProviderType>;

    /// Returns list of implemented (ready to use) provider types
    pub fn implemented_providers(&self) -> Vec<ProviderType>;
}
```

**Usage:**

```rust
// Create registry with defaults
let registry = ProviderRegistry::with_defaults();

// List available providers
for provider_type in registry.implemented_providers() {
    println!("Available: {}", provider_type.as_str());
}

// Create a specific provider
let provider = registry.create_by_name(
    "digitalocean",
    &api_token,
    ProviderTimeouts::default(),
)?;
```

---

## Type Aliases

```rust
/// Result type for provider operations
pub type ProviderResult<T> = Result<T, ProviderError>;
```
