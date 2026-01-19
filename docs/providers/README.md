# Provider System

Spuff uses a provider abstraction layer that allows it to work with multiple cloud providers. This document explains how the provider system works.

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                         spuff CLI                            │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                    Provider Trait                       │  │
│  │                                                         │  │
│  │  create_instance()  destroy_instance()  list_instances()│  │
│  │  get_instance()     wait_ready()        create_snapshot()│ │
│  │  list_snapshots()   delete_snapshot()                   │  │
│  └────────────────────────────────────────────────────────┘  │
│                              │                                │
│         ┌────────────────────┼────────────────────┐          │
│         │                    │                    │          │
│         ▼                    ▼                    ▼          │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐    │
│  │DigitalOcean │     │   Hetzner   │     │     AWS     │    │
│  │  Provider   │     │  Provider   │     │  Provider   │    │
│  └─────────────┘     └─────────────┘     └─────────────┘    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Architecture

### Provider Trait

The `Provider` trait (`src/provider/mod.rs`) defines the interface that all cloud providers must implement:

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

### Data Types

**InstanceConfig** - Configuration for creating an instance:

```rust
pub struct InstanceConfig {
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub ssh_keys: Vec<String>,
    pub user_data: Option<String>,
    pub tags: Vec<String>,
}
```

**Instance** - Represents a running instance:

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
```

**Snapshot** - Represents a saved instance state:

```rust
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub size_gb: u64,
    pub created_at: DateTime<Utc>,
    pub regions: Vec<String>,
}
```

## Current Providers

| Provider | Status | Location |
|----------|--------|----------|
| DigitalOcean | Stable | `src/provider/digitalocean.rs` |
| Hetzner | Planned | - |
| AWS EC2 | Planned | - |

## Provider Factory

Providers are instantiated through the factory function:

```rust
pub fn create_provider(config: &AppConfig) -> Result<Box<dyn Provider>> {
    match config.provider.as_str() {
        "digitalocean" => {
            let token = get_api_token("DIGITALOCEAN_TOKEN")?;
            Ok(Box::new(DigitalOceanProvider::new(&token)?))
        }
        "hetzner" => {
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

## Documentation

- [Creating a Provider](creating-a-provider.md) - Step-by-step guide
- [Provider API Reference](provider-api.md) - Detailed API documentation
- [Testing Providers](testing-providers.md) - How to test your provider

## Contributing a Provider

We welcome contributions for new cloud providers! Before starting:

1. Open an issue to discuss the provider
2. Review existing implementations (DigitalOcean is the reference)
3. Follow the [Creating a Provider](creating-a-provider.md) guide
4. Ensure comprehensive test coverage
5. Add documentation and examples
