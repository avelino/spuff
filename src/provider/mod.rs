//! Cloud provider abstraction layer.
//!
//! This module provides a unified interface for interacting with different
//! cloud providers. The `Provider` trait defines the contract that all
//! providers must implement, while the registry allows dynamic provider
//! creation.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │ ProviderRegistry│  ← Creates providers by name
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │  dyn Provider   │  ← Common interface
//! └────────┬────────┘
//!          │
//!    ┌─────┴─────┐
//!    ▼           ▼
//! ┌──────┐   ┌──────┐
//! │  DO  │   │ Hetz │  ← Implementations
//! └──────┘   └──────┘
//! ```
//!
//! # Adding a New Provider
//!
//! 1. Create a new module (e.g., `hetzner.rs`)
//! 2. Implement the `Provider` trait
//! 3. Implement the `ProviderFactory` trait
//! 4. Register in `ProviderRegistry::register_defaults()`

pub mod config;
pub mod digitalocean;
pub mod docker;
pub mod error;
pub mod registry;

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-export commonly used types
pub use config::{ImageSpec, InstanceRequest, ProviderTimeouts, ProviderType, VolumeMount};
pub use error::{ProviderError, ProviderResult};
pub use registry::ProviderRegistry;

use crate::config::AppConfig;

/// Legacy InstanceConfig for backward compatibility.
/// New code should use `InstanceRequest` instead.
#[deprecated(since = "0.2.0", note = "Use InstanceRequest instead")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub ssh_keys: Vec<String>,
    pub user_data: Option<String>,
    pub tags: Vec<String>,
}

#[allow(deprecated)]
impl From<InstanceConfig> for InstanceRequest {
    fn from(config: InstanceConfig) -> Self {
        let image = if config.image.starts_with("ami-") {
            ImageSpec::custom(&config.image)
        } else if config.image.contains("snapshot") || config.image.parse::<u64>().is_ok() {
            ImageSpec::snapshot(&config.image)
        } else {
            // Try to parse as Ubuntu version or use as custom
            ImageSpec::custom(&config.image)
        };

        let mut labels = std::collections::HashMap::new();
        for tag in config.tags {
            labels.insert(tag, "true".to_string());
        }

        InstanceRequest {
            name: config.name,
            region: config.region,
            size: config.size,
            image,
            user_data: config.user_data,
            labels,
            volumes: Vec::new(),
        }
    }
}

/// Instance returned by provider operations.
///
/// This represents an instance as seen by the cloud provider.
/// For local state tracking, see `crate::state::LocalInstance`.
#[derive(Debug, Clone)]
pub struct ProviderInstance {
    /// Provider-specific instance ID
    pub id: String,

    /// Public IP address (may be 0.0.0.0 if not yet assigned)
    pub ip: IpAddr,

    /// Current instance status
    pub status: InstanceStatus,

    /// When the instance was created
    pub created_at: DateTime<Utc>,
}

/// Instance lifecycle status.
#[derive(Debug, Clone, PartialEq)]
pub enum InstanceStatus {
    /// Instance is being created
    New,
    /// Instance is running and ready
    Active,
    /// Instance is powered off
    Off,
    /// Instance is archived/stopped (provider-specific)
    Archive,
    /// Unknown status from provider
    Unknown(String),
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceStatus::New => write!(f, "new"),
            InstanceStatus::Active => write!(f, "active"),
            InstanceStatus::Off => write!(f, "off"),
            InstanceStatus::Archive => write!(f, "archive"),
            InstanceStatus::Unknown(s) => write!(f, "{}", s),
        }
    }
}

/// Snapshot of an instance.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Provider-specific snapshot ID
    pub id: String,

    /// Snapshot name
    pub name: String,

    /// When the snapshot was created
    pub created_at: Option<DateTime<Utc>>,
}

/// Core trait that all cloud providers must implement.
///
/// This trait defines the contract for interacting with cloud providers.
/// Implementations should handle provider-specific API calls and map
/// responses to the common types defined in this module.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider name
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// Create a new instance.
    ///
    /// Returns immediately with instance ID. Use `wait_ready` to wait
    /// for the instance to be fully provisioned.
    async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance>;

    /// Destroy an instance.
    ///
    /// This operation is idempotent - destroying a non-existent instance
    /// should succeed silently.
    async fn destroy_instance(&self, id: &str) -> ProviderResult<()>;

    /// Get instance details by ID.
    ///
    /// Returns `None` if the instance doesn't exist.
    async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>>;

    /// List all instances with the "spuff" tag.
    #[allow(dead_code)]
    async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>>;

    /// Wait for an instance to be ready (active status + IP assigned).
    ///
    /// Returns the updated instance when ready, or error on timeout.
    async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance>;

    /// Create a snapshot of an instance.
    ///
    /// This may be an async operation - the method waits for completion.
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot>;

    /// List all snapshots with the "spuff" prefix.
    async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>>;

    /// Delete a snapshot.
    ///
    /// This operation is idempotent - deleting a non-existent snapshot
    /// should succeed silently.
    async fn delete_snapshot(&self, id: &str) -> ProviderResult<()>;

    // Optional methods with default implementations

    /// Get provider-specific SSH key IDs configured in the account.
    ///
    /// Default implementation returns empty list (use cloud-init for keys).
    #[allow(dead_code)]
    async fn get_ssh_keys(&self) -> ProviderResult<Vec<String>> {
        Ok(vec![])
    }

    /// Check if the provider supports snapshots.
    #[allow(dead_code)]
    fn supports_snapshots(&self) -> bool {
        true
    }
}

/// Create a provider from application configuration.
///
/// This is the main entry point for creating providers. It uses the
/// provider registry internally.
///
/// # Example
///
/// ```ignore
/// let config = AppConfig::load()?;
/// let provider = create_provider(&config)?;
/// let instance = provider.create_instance(&request).await?;
/// ```
pub fn create_provider(config: &AppConfig) -> crate::error::Result<Box<dyn Provider>> {
    let registry = ProviderRegistry::with_defaults();
    let timeouts = ProviderTimeouts::default();

    registry
        .create_by_name(&config.provider, &config.api_token, timeouts)
        .map_err(|e| crate::error::SpuffError::Provider(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_status_display() {
        assert_eq!(InstanceStatus::New.to_string(), "new");
        assert_eq!(InstanceStatus::Active.to_string(), "active");
        assert_eq!(InstanceStatus::Off.to_string(), "off");
        assert_eq!(InstanceStatus::Archive.to_string(), "archive");
        assert_eq!(
            InstanceStatus::Unknown("custom".to_string()).to_string(),
            "custom"
        );
    }

    #[allow(deprecated)]
    #[test]
    fn test_instance_config_to_request() {
        let config = InstanceConfig {
            name: "test".to_string(),
            region: "nyc1".to_string(),
            size: "s-2vcpu-4gb".to_string(),
            image: "ubuntu-24-04-x64".to_string(),
            ssh_keys: vec!["123".to_string()],
            user_data: Some("#cloud-config".to_string()),
            tags: vec!["spuff".to_string()],
        };

        let request: InstanceRequest = config.into();
        assert_eq!(request.name, "test");
        assert_eq!(request.region, "nyc1");
        assert!(request.labels.contains_key("spuff"));
    }
}
