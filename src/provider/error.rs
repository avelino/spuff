//! Provider-specific error types.
//!
//! This module provides structured error types for cloud provider operations,
//! enabling proper error handling and recovery strategies.

use std::time::Duration;

use thiserror::Error;

/// Errors that can occur during provider operations.
///
/// These errors are structured to allow proper handling and recovery.
/// For example, rate limit errors include retry information, and
/// authentication errors can trigger token refresh flows.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Authentication failed (invalid token, expired credentials, etc.)
    #[error("Authentication failed for {provider}: {message}")]
    Authentication { provider: String, message: String },

    /// Rate limit exceeded - includes optional retry-after duration
    #[error("Rate limit exceeded{}", .retry_after.map(|d| format!(", retry after {:?}", d)).unwrap_or_default())]
    RateLimit { retry_after: Option<Duration> },

    /// Resource not found (instance, snapshot, etc.)
    #[error("{resource_type} not found: {id}")]
    NotFound { resource_type: String, id: String },

    /// Quota or limit exceeded
    #[error("Quota exceeded for {resource}: {message}")]
    #[allow(dead_code)]
    QuotaExceeded { resource: String, message: String },

    /// Invalid configuration or request parameters
    #[error("Invalid configuration for {field}: {message}")]
    InvalidConfig { field: String, message: String },

    /// Feature not supported by this provider
    #[error("Feature not supported: {feature}")]
    #[allow(dead_code)]
    NotSupported { feature: String },

    /// Operation timed out
    #[error("Operation timed out after {elapsed:?}: {operation}")]
    Timeout {
        operation: String,
        elapsed: Duration,
    },

    /// Network or HTTP error
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// API returned an error response
    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    /// Provider is not yet implemented
    #[error("Provider '{name}' is not yet implemented")]
    NotImplemented { name: String },

    /// Unknown provider name
    #[error("Unknown provider: {name}. Supported providers: {supported:?}")]
    UnknownProvider {
        name: String,
        supported: Vec<String>,
    },

    /// Generic provider error for edge cases
    #[error("{message}")]
    Other { message: String },
}

impl ProviderError {
    /// Create an authentication error
    pub fn auth(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Authentication {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// Create a not found error
    pub fn not_found(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Create an API error from status code and message
    pub fn api(status: u16, message: impl Into<String>) -> Self {
        Self::Api {
            status,
            message: message.into(),
        }
    }

    /// Create a timeout error
    pub fn timeout(operation: impl Into<String>, elapsed: Duration) -> Self {
        Self::Timeout {
            operation: operation.into(),
            elapsed,
        }
    }

    /// Create a quota exceeded error
    #[allow(dead_code)]
    pub fn quota(resource: impl Into<String>, message: impl Into<String>) -> Self {
        Self::QuotaExceeded {
            resource: resource.into(),
            message: message.into(),
        }
    }

    /// Create an invalid config error
    pub fn invalid_config(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    #[allow(dead_code)]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit { .. } | Self::Timeout { .. } | Self::Network(_)
        )
    }

    /// Get retry delay if applicable
    #[allow(dead_code)]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit { retry_after } => *retry_after,
            Self::Timeout { .. } => Some(Duration::from_secs(5)),
            Self::Network(_) => Some(Duration::from_secs(2)),
            _ => None,
        }
    }
}

/// Result type alias for provider operations
pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_error_display() {
        let err = ProviderError::auth("digitalocean", "Invalid token");
        assert_eq!(
            err.to_string(),
            "Authentication failed for digitalocean: Invalid token"
        );
    }

    #[test]
    fn test_not_found_error_display() {
        let err = ProviderError::not_found("instance", "123456");
        assert_eq!(err.to_string(), "instance not found: 123456");
    }

    #[test]
    fn test_rate_limit_with_retry() {
        let err = ProviderError::RateLimit {
            retry_after: Some(Duration::from_secs(60)),
        };
        assert!(err.to_string().contains("retry after"));
        assert!(err.is_retryable());
        assert_eq!(err.retry_after(), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_rate_limit_without_retry() {
        let err = ProviderError::RateLimit { retry_after: None };
        assert_eq!(err.to_string(), "Rate limit exceeded");
    }

    #[test]
    fn test_api_error() {
        let err = ProviderError::api(401, "Unauthorized");
        assert_eq!(err.to_string(), "API error (401): Unauthorized");
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_timeout_error() {
        let err = ProviderError::timeout("wait_ready", Duration::from_secs(300));
        assert!(err.to_string().contains("timed out"));
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable() {
        assert!(ProviderError::RateLimit { retry_after: None }.is_retryable());
        assert!(ProviderError::timeout("test", Duration::from_secs(1)).is_retryable());
        assert!(!ProviderError::auth("test", "bad token").is_retryable());
        assert!(!ProviderError::not_found("instance", "123").is_retryable());
    }
}
