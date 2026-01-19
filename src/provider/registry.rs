//! Provider registry for dynamic provider creation.
//!
//! This module implements the registry pattern, allowing providers to be
//! registered at runtime and created by name. This makes adding new providers
//! a matter of implementing the traits and registering the factory.

use std::collections::HashMap;
use std::sync::Arc;

use super::config::{ProviderTimeouts, ProviderType};
use super::error::{ProviderError, ProviderResult};
use super::Provider;

/// Factory trait for creating provider instances.
///
/// Each provider implements this trait to define how instances are created
/// from configuration.
pub trait ProviderFactory: Send + Sync {
    /// Get the provider type this factory creates
    fn provider_type(&self) -> ProviderType;

    /// Create a new provider instance
    fn create(&self, token: &str, timeouts: ProviderTimeouts) -> ProviderResult<Box<dyn Provider>>;

    /// Check if this provider is fully implemented
    fn is_implemented(&self) -> bool {
        self.provider_type().is_implemented()
    }
}

/// Registry for provider factories.
///
/// The registry maintains a collection of provider factories and allows
/// creating providers by name. This decouples the provider creation logic
/// from the calling code.
pub struct ProviderRegistry {
    factories: HashMap<ProviderType, Arc<dyn ProviderFactory>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Create a registry with all built-in providers registered
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }

    /// Register all default provider factories
    pub fn register_defaults(&mut self) {
        use super::digitalocean::DigitalOceanFactory;
        self.register(DigitalOceanFactory);
        // Future providers:
        // self.register(HetznerFactory);
        // self.register(AwsFactory);
    }

    /// Register a provider factory
    pub fn register<F: ProviderFactory + 'static>(&mut self, factory: F) {
        self.factories
            .insert(factory.provider_type(), Arc::new(factory));
    }

    /// Create a provider by type
    pub fn create(
        &self,
        provider_type: ProviderType,
        token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>> {
        let factory = self.factories.get(&provider_type).ok_or_else(|| {
            if provider_type.is_implemented() {
                ProviderError::Other {
                    message: format!("Provider {} not registered", provider_type),
                }
            } else {
                ProviderError::NotImplemented {
                    name: provider_type.to_string(),
                }
            }
        })?;

        if !factory.is_implemented() {
            return Err(ProviderError::NotImplemented {
                name: provider_type.to_string(),
            });
        }

        factory.create(token, timeouts)
    }

    /// Create a provider by name string
    pub fn create_by_name(
        &self,
        name: &str,
        token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>> {
        let provider_type = ProviderType::from_str(name).ok_or_else(|| {
            ProviderError::UnknownProvider {
                name: name.to_string(),
                supported: ProviderType::supported_names(),
            }
        })?;

        self.create(provider_type, token, timeouts)
    }

    /// Get list of registered provider types
    pub fn registered_providers(&self) -> Vec<ProviderType> {
        self.factories.keys().copied().collect()
    }

    /// Check if a provider type is registered
    pub fn is_registered(&self, provider_type: ProviderType) -> bool {
        self.factories.contains_key(&provider_type)
    }

    /// Get list of implemented (ready to use) providers
    pub fn implemented_providers(&self) -> Vec<ProviderType> {
        self.factories
            .iter()
            .filter(|(_, f)| f.is_implemented())
            .map(|(t, _)| *t)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_with_defaults() {
        let registry = ProviderRegistry::with_defaults();
        assert!(registry.is_registered(ProviderType::DigitalOcean));
    }

    #[test]
    fn test_create_by_name_unknown() {
        let registry = ProviderRegistry::with_defaults();
        let result = registry.create_by_name("unknown", "token", ProviderTimeouts::default());
        assert!(matches!(result, Err(ProviderError::UnknownProvider { .. })));
    }

    #[test]
    fn test_create_digitalocean() {
        let registry = ProviderRegistry::with_defaults();
        let result =
            registry.create_by_name("digitalocean", "test-token", ProviderTimeouts::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_digitalocean_empty_token() {
        let registry = ProviderRegistry::with_defaults();
        let result = registry.create_by_name("digitalocean", "", ProviderTimeouts::default());
        assert!(matches!(result, Err(ProviderError::Authentication { .. })));
    }

    #[test]
    fn test_implemented_providers() {
        let registry = ProviderRegistry::with_defaults();
        let implemented = registry.implemented_providers();
        assert!(implemented.contains(&ProviderType::DigitalOcean));
    }
}
