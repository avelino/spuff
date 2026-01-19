//! Provider configuration types.
//!
//! This module contains configuration structures that are provider-agnostic,
//! allowing the same configuration to work across different cloud providers.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Configuration for creating a new instance.
///
/// This struct contains provider-agnostic configuration. Each provider
/// implementation is responsible for mapping these values to their
/// specific API requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRequest {
    /// Instance name (will be prefixed/suffixed as needed by provider)
    pub name: String,

    /// Region/datacenter location
    pub region: String,

    /// Instance size/type identifier
    pub size: String,

    /// Base image specification (provider resolves to actual image ID)
    pub image: ImageSpec,

    /// Cloud-init or user-data script
    pub user_data: Option<String>,

    /// Labels/tags for the instance
    pub labels: HashMap<String, String>,
}

impl InstanceRequest {
    /// Create a new instance request with required fields
    pub fn new(name: impl Into<String>, region: impl Into<String>, size: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            region: region.into(),
            size: size.into(),
            image: ImageSpec::default(),
            user_data: None,
            labels: HashMap::new(),
        }
    }

    /// Set the image specification
    pub fn with_image(mut self, image: ImageSpec) -> Self {
        self.image = image;
        self
    }

    /// Set user data (cloud-init script)
    pub fn with_user_data(mut self, user_data: impl Into<String>) -> Self {
        self.user_data = Some(user_data.into());
        self
    }

    /// Add a label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add multiple labels
    #[allow(dead_code)]
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }
}

/// Image specification that providers resolve to their specific format.
///
/// This allows expressing images in a provider-agnostic way while still
/// supporting provider-specific image IDs when needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageSpec {
    /// Ubuntu LTS version (e.g., "24.04")
    Ubuntu(String),

    /// Debian version (e.g., "12")
    Debian(String),

    /// Provider-specific image ID (e.g., "ami-xxx" for AWS)
    Custom(String),

    /// Snapshot ID to restore from
    Snapshot(String),
}

impl Default for ImageSpec {
    fn default() -> Self {
        Self::Ubuntu("24.04".to_string())
    }
}

impl ImageSpec {
    /// Create Ubuntu image spec
    pub fn ubuntu(version: impl Into<String>) -> Self {
        Self::Ubuntu(version.into())
    }

    /// Create Debian image spec
    #[allow(dead_code)]
    pub fn debian(version: impl Into<String>) -> Self {
        Self::Debian(version.into())
    }

    /// Create custom/provider-specific image spec
    pub fn custom(id: impl Into<String>) -> Self {
        Self::Custom(id.into())
    }

    /// Create snapshot image spec
    pub fn snapshot(id: impl Into<String>) -> Self {
        Self::Snapshot(id.into())
    }
}

/// Timeout configuration for provider operations.
///
/// These timeouts control how long various operations wait before failing.
/// Different providers may have different typical response times.
#[derive(Debug, Clone)]
pub struct ProviderTimeouts {
    /// How long to wait for an instance to become ready
    pub instance_ready: Duration,

    /// How long to wait for an async action to complete (e.g., snapshot)
    pub action_complete: Duration,

    /// Interval between status poll requests
    pub poll_interval: Duration,

    /// HTTP request timeout
    pub http_request: Duration,

    /// SSH connection timeout
    #[allow(dead_code)]
    pub ssh_connect: Duration,

    /// Cloud-init completion timeout
    #[allow(dead_code)]
    pub cloud_init: Duration,
}

impl Default for ProviderTimeouts {
    fn default() -> Self {
        Self {
            instance_ready: Duration::from_secs(300),   // 5 minutes
            action_complete: Duration::from_secs(600),  // 10 minutes
            poll_interval: Duration::from_secs(5),
            http_request: Duration::from_secs(30),
            ssh_connect: Duration::from_secs(300),      // 5 minutes
            cloud_init: Duration::from_secs(600),       // 10 minutes
        }
    }
}

impl ProviderTimeouts {
    /// Calculate max attempts for an operation given poll interval
    pub fn max_attempts(&self, operation_timeout: Duration) -> usize {
        (operation_timeout.as_secs() / self.poll_interval.as_secs().max(1)) as usize
    }

    /// Get instance ready max attempts
    pub fn instance_ready_attempts(&self) -> usize {
        self.max_attempts(self.instance_ready)
    }

    /// Get action complete max attempts
    pub fn action_complete_attempts(&self) -> usize {
        self.max_attempts(self.action_complete)
    }
}

/// Supported cloud providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    DigitalOcean,
    Hetzner,
    Aws,
}

impl ProviderType {
    /// Get all supported provider types
    pub fn all() -> &'static [ProviderType] {
        &[Self::DigitalOcean, Self::Hetzner, Self::Aws]
    }

    /// Get provider name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DigitalOcean => "digitalocean",
            Self::Hetzner => "hetzner",
            Self::Aws => "aws",
        }
    }

    /// Get list of supported provider names
    pub fn supported_names() -> Vec<String> {
        Self::all().iter().map(|p| p.as_str().to_string()).collect()
    }

    /// Parse provider type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "digitalocean" | "do" => Some(Self::DigitalOcean),
            "hetzner" | "hcloud" => Some(Self::Hetzner),
            "aws" | "ec2" => Some(Self::Aws),
            _ => None,
        }
    }

    /// Get the environment variable name for the API token
    #[allow(dead_code)]
    pub fn token_env_var(&self) -> &'static str {
        match self {
            Self::DigitalOcean => "DIGITALOCEAN_TOKEN",
            Self::Hetzner => "HETZNER_TOKEN",
            Self::Aws => "AWS_ACCESS_KEY_ID",
        }
    }

    /// Check if this provider is implemented
    pub fn is_implemented(&self) -> bool {
        matches!(self, Self::DigitalOcean)
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or_else(|| {
            format!(
                "Unknown provider '{}'. Supported: {:?}",
                s,
                Self::supported_names()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_request_builder() {
        let req = InstanceRequest::new("test", "nyc1", "s-2vcpu-4gb")
            .with_image(ImageSpec::ubuntu("24.04"))
            .with_user_data("#cloud-config")
            .with_label("env", "dev")
            .with_label("team", "platform");

        assert_eq!(req.name, "test");
        assert_eq!(req.region, "nyc1");
        assert_eq!(req.labels.get("env"), Some(&"dev".to_string()));
        assert_eq!(req.labels.get("team"), Some(&"platform".to_string()));
    }

    #[test]
    fn test_image_spec_default() {
        let spec = ImageSpec::default();
        assert!(matches!(spec, ImageSpec::Ubuntu(v) if v == "24.04"));
    }

    #[test]
    fn test_provider_timeouts_max_attempts() {
        let timeouts = ProviderTimeouts::default();
        // 300 seconds / 5 second interval = 60 attempts
        assert_eq!(timeouts.instance_ready_attempts(), 60);
        // 600 seconds / 5 second interval = 120 attempts
        assert_eq!(timeouts.action_complete_attempts(), 120);
    }

    #[test]
    fn test_provider_type_from_str() {
        assert_eq!(
            ProviderType::from_str("digitalocean"),
            Some(ProviderType::DigitalOcean)
        );
        assert_eq!(ProviderType::from_str("do"), Some(ProviderType::DigitalOcean));
        assert_eq!(ProviderType::from_str("hetzner"), Some(ProviderType::Hetzner));
        assert_eq!(ProviderType::from_str("aws"), Some(ProviderType::Aws));
        assert_eq!(ProviderType::from_str("unknown"), None);
    }

    #[test]
    fn test_provider_type_token_env_var() {
        assert_eq!(
            ProviderType::DigitalOcean.token_env_var(),
            "DIGITALOCEAN_TOKEN"
        );
        assert_eq!(ProviderType::Hetzner.token_env_var(), "HETZNER_TOKEN");
        assert_eq!(ProviderType::Aws.token_env_var(), "AWS_ACCESS_KEY_ID");
    }

    #[test]
    fn test_provider_type_is_implemented() {
        assert!(ProviderType::DigitalOcean.is_implemented());
        assert!(!ProviderType::Hetzner.is_implemented());
        assert!(!ProviderType::Aws.is_implemented());
    }
}
