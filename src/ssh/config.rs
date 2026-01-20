//! SSH connection configuration.

use std::path::PathBuf;

/// SSH connection configuration.
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// SSH username.
    pub user: String,

    /// Path to the private key file.
    pub key_path: PathBuf,

    /// Host key verification policy.
    pub host_key_policy: HostKeyPolicy,
}

/// Host key verification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostKeyPolicy {
    /// Accept any host key (insecure, but matches OpenSSH StrictHostKeyChecking=no).
    #[default]
    AcceptAny,

    /// Accept new keys but reject changed keys (matches StrictHostKeyChecking=accept-new).
    AcceptNew,
}

impl SshConfig {
    /// Create a new SSH configuration.
    pub fn new(user: impl Into<String>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            user: user.into(),
            key_path: key_path.into(),
            host_key_policy: HostKeyPolicy::AcceptAny,
        }
    }
}

impl From<&crate::config::AppConfig> for SshConfig {
    fn from(app_config: &crate::config::AppConfig) -> Self {
        Self::new(&app_config.ssh_user, &app_config.ssh_key_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_config_new() {
        let config = SshConfig::new("root", "/path/to/key");

        assert_eq!(config.user, "root");
        assert_eq!(config.host_key_policy, HostKeyPolicy::AcceptAny);
    }
}
