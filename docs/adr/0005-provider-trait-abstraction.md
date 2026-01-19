# ADR-0005: Provider Trait for Cloud Abstraction

## Status

Accepted

## Date

2025-01

## Context

Spuff aims to support multiple cloud providers:

- DigitalOcean (current)
- Hetzner Cloud (planned)
- AWS EC2 (planned)
- Others (future)

Each provider has different:

- API endpoints and authentication
- Resource naming (droplets, servers, instances)
- Region and size identifiers
- Snapshot mechanisms
- Rate limits and quirks

### Requirements

- Support multiple cloud providers
- Consistent CLI experience regardless of provider
- Easy to add new providers
- Provider-specific features accessible when needed
- Clean separation of concerns

## Decision

We will use a **Rust trait** (`Provider`) as an abstraction layer over cloud providers.

### The Provider Trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    // Instance lifecycle
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance>;
    async fn destroy_instance(&self, id: &str) -> Result<()>;
    async fn get_instance(&self, id: &str) -> Result<Option<Instance>>;
    async fn list_instances(&self) -> Result<Vec<Instance>>;
    async fn wait_ready(&self, id: &str) -> Result<Instance>;

    // Snapshots
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot>;
    async fn list_snapshots(&self) -> Result<Vec<Snapshot>>;
    async fn delete_snapshot(&self, id: &str) -> Result<()>;
}
```

### Factory Pattern

```rust
pub fn create_provider(config: &AppConfig) -> Result<Box<dyn Provider>> {
    match config.provider.as_str() {
        "digitalocean" => Ok(Box::new(DigitalOceanProvider::new(&token)?)),
        "hetzner" => Ok(Box::new(HetznerProvider::new(&token)?)),
        _ => Err(SpuffError::Config("Unknown provider")),
    }
}
```

### Common Data Types

```rust
pub struct Instance {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub status: InstanceStatus,
    pub region: String,
    pub size: String,
    pub created_at: DateTime<Utc>,
}

pub enum InstanceStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Unknown,
}
```

## Consequences

### Positive

- **Extensibility**: New providers just implement the trait
- **Consistency**: CLI code doesn't change for different providers
- **Testability**: Mock providers for testing
- **Type safety**: Rust compiler enforces interface compliance
- **Documentation**: Trait documents the required API

### Negative

- **Lowest common denominator**: Trait methods must work across all providers
- **Feature gaps**: Provider-specific features harder to expose
- **Abstraction leak**: Some provider differences may leak through
- **Maintenance**: Must update all providers for trait changes

### Neutral

- Requires understanding of Rust traits and dynamic dispatch
- Some boxing overhead (negligible for network operations)

## Design Decisions

### Why `async_trait`?

Cloud API calls are async, and Rust traits don't natively support async methods. The `async_trait` crate provides this capability.

### Why `Box<dyn Provider>`?

Dynamic dispatch allows runtime provider selection based on configuration. The alternative (generics) would require compile-time provider choice.

### Why `Send + Sync`?

The provider may be used across async tasks. These bounds ensure thread safety.

### Method Granularity

Methods are designed to be:

- **Atomic**: Each method does one thing
- **Composable**: Higher-level operations built from primitives
- **Idempotent where possible**: Delete already-deleted resources shouldn't error

## Alternatives Considered

### Alternative 1: No Abstraction

Direct provider API calls throughout the codebase.

**Pros:**

- Full access to provider features
- No abstraction overhead

**Cons:**

- Code duplication
- Provider-specific code everywhere
- Hard to add new providers

**Why rejected:** Not scalable for multi-cloud support.

### Alternative 2: Enum-Based Dispatch

Use an enum with match statements:

```rust
enum Provider {
    DigitalOcean(DigitalOceanProvider),
    Hetzner(HetznerProvider),
}
```

**Pros:**

- No dynamic dispatch
- Exhaustive matching

**Cons:**

- Every match statement must handle all providers
- Adding provider touches many files

**Why rejected:** Too invasive when adding providers.

### Alternative 3: Generic Parameters

Use generic parameters instead of trait objects:

```rust
fn run<P: Provider>(provider: P, config: &AppConfig) -> Result<()>
```

**Pros:**

- No boxing overhead
- Monomorphization

**Cons:**

- Can't select provider at runtime
- Larger binary (code per provider)

**Why rejected:** Need runtime provider selection from config.

### Alternative 4: gRPC/Plugin System

Load providers as separate processes/plugins.

**Pros:**

- True isolation
- Dynamic loading

**Cons:**

- Massive complexity
- IPC overhead
- Deployment complexity

**Why rejected:** Overkill for this use case.

## Future Considerations

### Provider-Specific Extensions

For provider-specific features, we can:

1. **Downcast**: Cast `Box<dyn Provider>` to concrete type
2. **Extension traits**: Additional traits for specific capabilities
3. **Feature flags**: Optional trait methods with default implementations

### API Versioning

If trait changes significantly:

1. Create new trait version (e.g., `ProviderV2`)
2. Provide adapter from old to new
3. Deprecate old trait gradually

## References

- [Rust Trait Objects](https://doc.rust-lang.org/book/ch17-02-trait-objects.html)
- [async-trait Crate](https://github.com/dtolnay/async-trait)
- [Provider Pattern Discussion](docs/providers/README.md)
