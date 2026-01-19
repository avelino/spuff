use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SpuffError};
use crate::provider::ProviderType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_token: String,
    pub region: String,
    pub size: String,
    pub idle_timeout: String,
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dotfiles: Option<String>,
    pub ssh_key_path: String,
    #[serde(default = "default_ssh_user")]
    pub ssh_user: String,
    #[serde(default)]
    pub tailscale_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_authkey: Option<String>,
    /// Authentication token for the spuff-agent API.
    /// When set, the agent requires this token in the X-Spuff-Token header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_token: Option<String>,
}

fn default_ssh_user() -> String {
    "dev".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: "digitalocean".to_string(),
            api_token: String::new(),
            region: "nyc1".to_string(),
            size: "s-2vcpu-4gb".to_string(),
            idle_timeout: "2h".to_string(),
            environment: "devbox".to_string(),
            dotfiles: None,
            ssh_key_path: shellexpand::tilde("~/.ssh/id_ed25519").to_string(),
            ssh_user: "dev".to_string(),
            tailscale_enabled: false,
            tailscale_authkey: None,
            agent_token: None,
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .map_err(|_| SpuffError::Config("HOME environment variable not set".to_string()))?;
        Ok(PathBuf::from(home).join(".config").join("spuff"))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.yaml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            return Err(SpuffError::Config(format!(
                "Config file not found: {}. Run 'spuff init' first.",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(&path)?;
        let config: AppConfig = serde_yaml::from_str(&content)
            .map_err(|e| SpuffError::Config(format!("Invalid config: {}", e)))?;

        let mut config = config;

        // Load API token from environment if not in config
        if config.api_token.is_empty() {
            if let Ok(token) = std::env::var("SPUFF_API_TOKEN") {
                config.api_token = token;
            } else {
                let env_var = match config.provider.as_str() {
                    "digitalocean" => "DIGITALOCEAN_TOKEN",
                    "hetzner" => "HETZNER_TOKEN",
                    "aws" => "AWS_ACCESS_KEY_ID",
                    _ => "SPUFF_API_TOKEN",
                };

                if let Ok(token) = std::env::var(env_var) {
                    config.api_token = token;
                }
            }
        }

        // Load agent token from environment if not in config
        if config.agent_token.is_none() {
            if let Ok(token) = std::env::var("SPUFF_AGENT_TOKEN") {
                config.agent_token = Some(token);
            }
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_yaml::to_string(self)
            .map_err(|e| SpuffError::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&path, content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    pub fn parse_idle_timeout(&self) -> std::time::Duration {
        parse_duration(&self.idle_timeout).unwrap_or(std::time::Duration::from_secs(7200))
    }

    /// Validate the configuration.
    ///
    /// Returns an error if the configuration is invalid (e.g., unknown provider).
    pub fn validate(&self) -> Result<()> {
        // Validate provider
        if ProviderType::from_str(&self.provider).is_none() {
            return Err(SpuffError::Config(format!(
                "Unknown provider '{}'. Supported providers: {:?}",
                self.provider,
                ProviderType::supported_names()
            )));
        }

        // Validate idle timeout is parseable
        if parse_duration(&self.idle_timeout).is_none() {
            return Err(SpuffError::Config(format!(
                "Invalid idle_timeout '{}'. Use format like '2h', '30m', or '3600'",
                self.idle_timeout
            )));
        }

        // Validate SSH key path exists
        let ssh_path = shellexpand::tilde(&self.ssh_key_path);
        if !std::path::Path::new(ssh_path.as_ref()).exists() {
            return Err(SpuffError::Config(format!(
                "SSH key not found at '{}'. Generate one with: ssh-keygen -t ed25519",
                self.ssh_key_path
            )));
        }

        Ok(())
    }

    /// Get the provider type enum.
    pub fn provider_type(&self) -> Option<ProviderType> {
        ProviderType::from_str(&self.provider)
    }

    /// Check if the configured provider is implemented.
    pub fn is_provider_implemented(&self) -> bool {
        self.provider_type()
            .map(|p| p.is_implemented())
            .unwrap_or(false)
    }
}

fn parse_duration(s: &str) -> Option<std::time::Duration> {
    let s = s.trim().to_lowercase();

    if let Some(hours) = s.strip_suffix('h') {
        hours.parse::<u64>().ok().map(|h| std::time::Duration::from_secs(h * 3600))
    } else if let Some(minutes) = s.strip_suffix('m') {
        minutes.parse::<u64>().ok().map(|m| std::time::Duration::from_secs(m * 60))
    } else if let Some(seconds) = s.strip_suffix('s') {
        seconds.parse::<u64>().ok().map(std::time::Duration::from_secs)
    } else {
        s.parse::<u64>().ok().map(std::time::Duration::from_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("2h"), Some(std::time::Duration::from_secs(7200)));
        assert_eq!(parse_duration("1h"), Some(std::time::Duration::from_secs(3600)));
        assert_eq!(parse_duration("24H"), Some(std::time::Duration::from_secs(86400)));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("30m"), Some(std::time::Duration::from_secs(1800)));
        assert_eq!(parse_duration("1m"), Some(std::time::Duration::from_secs(60)));
        assert_eq!(parse_duration("90M"), Some(std::time::Duration::from_secs(5400)));
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("60s"), Some(std::time::Duration::from_secs(60)));
        assert_eq!(parse_duration("3600S"), Some(std::time::Duration::from_secs(3600)));
    }

    #[test]
    fn test_parse_duration_raw_seconds() {
        assert_eq!(parse_duration("7200"), Some(std::time::Duration::from_secs(7200)));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration("invalid"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("2x"), None);
    }

    #[test]
    fn test_parse_duration_whitespace() {
        assert_eq!(parse_duration("  2h  "), Some(std::time::Duration::from_secs(7200)));
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.provider, "digitalocean");
        assert_eq!(config.region, "nyc1");
        assert_eq!(config.size, "s-2vcpu-4gb");
        assert_eq!(config.idle_timeout, "2h");
        assert_eq!(config.environment, "devbox");
        assert_eq!(config.ssh_user, "dev");
        assert!(!config.tailscale_enabled);
        assert!(config.agent_token.is_none());
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig {
            provider: "digitalocean".to_string(),
            api_token: "".to_string(),
            region: "nyc1".to_string(),
            size: "s-2vcpu-4gb".to_string(),
            idle_timeout: "2h".to_string(),
            environment: "devbox".to_string(),
            dotfiles: Some("https://github.com/user/dotfiles".to_string()),
            ssh_key_path: "/home/user/.ssh/id_ed25519".to_string(),
            ssh_user: "root".to_string(),
            tailscale_enabled: false,
            tailscale_authkey: None,
            agent_token: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("provider: digitalocean"));
        assert!(yaml.contains("region: nyc1"));
        assert!(yaml.contains("dotfiles: https://github.com/user/dotfiles"));
        // api_token should not appear when empty
        assert!(!yaml.contains("api_token"));
        // agent_token should not appear when None
        assert!(!yaml.contains("agent_token"));
    }

    #[test]
    fn test_config_deserialization() {
        let yaml = r#"
provider: hetzner
region: fsn1
size: cx21
idle_timeout: 4h
environment: nix
ssh_key_path: /home/user/.ssh/id_rsa
ssh_user: admin
tailscale_enabled: true
tailscale_authkey: tskey-xxx
"#;

        let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.provider, "hetzner");
        assert_eq!(config.region, "fsn1");
        assert_eq!(config.size, "cx21");
        assert_eq!(config.idle_timeout, "4h");
        assert_eq!(config.environment, "nix");
        assert_eq!(config.ssh_user, "admin");
        assert!(config.tailscale_enabled);
        assert_eq!(config.tailscale_authkey, Some("tskey-xxx".to_string()));
    }

    #[test]
    fn test_parse_idle_timeout() {
        let config = AppConfig {
            idle_timeout: "2h".to_string(),
            ..Default::default()
        };
        assert_eq!(config.parse_idle_timeout().as_secs(), 7200);

        let config = AppConfig {
            idle_timeout: "30m".to_string(),
            ..Default::default()
        };
        assert_eq!(config.parse_idle_timeout().as_secs(), 1800);

        // Invalid falls back to 7200 (2h)
        let config = AppConfig {
            idle_timeout: "invalid".to_string(),
            ..Default::default()
        };
        assert_eq!(config.parse_idle_timeout().as_secs(), 7200);
    }

    // Note: Tests that modify HOME env var are marked #[ignore] to avoid
    // interference when running in parallel. Run with `cargo test -- --ignored`
    // to execute them.

    #[test]
    #[ignore]
    fn test_config_save_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join(".config").join("spuff");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.yaml");

        // Set HOME to temp dir for this test
        std::env::set_var("HOME", temp_dir.path());
        // Clear env tokens to avoid interference from parallel tests
        std::env::remove_var("DIGITALOCEAN_TOKEN");
        std::env::remove_var("SPUFF_API_TOKEN");

        let config = AppConfig {
            provider: "digitalocean".to_string(),
            api_token: "test-token".to_string(),
            region: "ams3".to_string(),
            size: "s-1vcpu-1gb".to_string(),
            idle_timeout: "1h".to_string(),
            environment: "devbox".to_string(),
            dotfiles: None,
            ssh_key_path: "/tmp/test-key".to_string(),
            ssh_user: "testuser".to_string(),
            tailscale_enabled: false,
            tailscale_authkey: None,
            agent_token: None,
        };

        config.save().unwrap();

        // Verify file exists
        assert!(config_path.exists());

        // Load and verify
        let loaded = AppConfig::load().unwrap();
        assert_eq!(loaded.provider, "digitalocean");
        assert_eq!(loaded.api_token, "test-token");
        assert_eq!(loaded.region, "ams3");
        assert_eq!(loaded.ssh_user, "testuser");
    }

    #[test]
    #[ignore]
    fn test_config_load_with_env_token() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join(".config").join("spuff");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.yaml");

        std::env::set_var("HOME", temp_dir.path());

        // Create config without token
        let yaml = r#"
provider: digitalocean
region: nyc1
size: s-2vcpu-4gb
idle_timeout: 2h
environment: devbox
ssh_key_path: /tmp/test-key
ssh_user: root
"#;
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        // Set env token
        std::env::set_var("DIGITALOCEAN_TOKEN", "env-token-123");

        let loaded = AppConfig::load().unwrap();
        assert_eq!(loaded.api_token, "env-token-123");

        // Cleanup
        std::env::remove_var("DIGITALOCEAN_TOKEN");
    }

    #[test]
    fn test_provider_type_methods() {
        let config = AppConfig {
            provider: "digitalocean".to_string(),
            ..Default::default()
        };
        assert!(config.provider_type().is_some());
        assert!(config.is_provider_implemented());

        let config = AppConfig {
            provider: "hetzner".to_string(),
            ..Default::default()
        };
        assert!(config.provider_type().is_some());
        assert!(!config.is_provider_implemented());

        let config = AppConfig {
            provider: "unknown".to_string(),
            ..Default::default()
        };
        assert!(config.provider_type().is_none());
        assert!(!config.is_provider_implemented());
    }

    #[test]
    fn test_validate_invalid_provider() {
        let config = AppConfig {
            provider: "invalid_provider".to_string(),
            ..Default::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown provider"));
    }

    #[test]
    fn test_validate_invalid_idle_timeout() {
        let config = AppConfig {
            provider: "digitalocean".to_string(),
            idle_timeout: "invalid".to_string(),
            ..Default::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid idle_timeout"));
    }
}
