//! SSH client implementation using russh.
//!
//! Provides connection management and authentication.

use std::net::ToSocketAddrs;
use std::sync::Arc;

use async_trait::async_trait;
use russh::client::{self, Handle};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::error::{Result, SpuffError};
use crate::ssh::config::SshConfig;
use crate::ssh::exec::CommandOutput;
use crate::ssh::sftp::SftpClient;
use crate::ssh::tunnel::PortForward;

/// SSH client wrapper over russh.
pub struct SshClient {
    session: Arc<Mutex<Handle<ClientHandler>>>,
    host: String,
}

impl SshClient {
    /// Connect to an SSH server.
    pub async fn connect(host: &str, port: u16, config: &SshConfig) -> Result<Self> {
        let russh_config = Arc::new(client::Config {
            // No inactivity timeout - keep connection alive indefinitely
            inactivity_timeout: None,
            // Send keep-alive every 15 seconds
            keepalive_interval: Some(std::time::Duration::from_secs(15)),
            // Allow up to 4 missed keep-alives before disconnect (60 seconds)
            keepalive_max: 4,
            ..Default::default()
        });

        // Resolve hostname to IP
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .map_err(|e| SpuffError::Ssh(format!("Failed to resolve {}: {}", host, e)))?
            .next()
            .ok_or_else(|| SpuffError::Ssh(format!("No address found for {}", host)))?;

        let handler = ClientHandler {
            host_key_policy: config.host_key_policy,
        };

        let mut session = client::connect(russh_config, addr, handler)
            .await
            .map_err(|e| SpuffError::Ssh(format!("Connection failed: {}", e)))?;

        // Authenticate
        Self::authenticate(&mut session, config).await?;

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            host: host.to_string(),
        })
    }

    /// Authenticate with the SSH server.
    async fn authenticate(session: &mut Handle<ClientHandler>, config: &SshConfig) -> Result<()> {
        // First, try the spuff managed key (ed25519, no RSA SHA2 issues)
        if let Ok(()) = Self::auth_with_managed_key(session, config).await {
            tracing::debug!("Authenticated with spuff managed key");
            return Ok(());
        }

        // Try agent authentication (works for ed25519, may fail for RSA due to SHA2)
        let agent_result = Self::auth_with_agent(session, config).await;
        let agent_rejected_keys = matches!(&agent_result, Ok(false));

        match agent_result {
            Ok(true) => return Ok(()),
            Ok(false) => {
                tracing::debug!("Agent authentication: server rejected all keys");
            }
            Err(e) => {
                tracing::debug!("Agent authentication failed: {}", e);
            }
        }

        // Check if key has passphrase before trying to load it directly
        let has_passphrase = crate::ssh::key_has_passphrase(&config.key_path).unwrap_or(true);

        if has_passphrase {
            if agent_rejected_keys {
                return Err(SpuffError::Ssh(
                    "SSH authentication failed. The server rejected all keys.\n\n\
                     This may be due to RSA SHA2 algorithm negotiation issues.\n\
                     Spuff uses its own ed25519 key for authentication.\n\n\
                     If this is a new instance, try: spuff down && spuff up\n\
                     This will re-provision with the spuff managed key."
                        .to_string(),
                ));
            }

            return Err(SpuffError::Ssh(format!(
                "SSH key requires passphrase but is not loaded in the agent.\n\
                 Add your key to the agent first:\n\n  \
                 ssh-add {}\n\n\
                 On macOS with Keychain:\n\n  \
                 ssh-add --apple-use-keychain {}",
                config.key_path.display(),
                config.key_path.display()
            )));
        }

        // Try key file authentication (only for unencrypted keys)
        Self::auth_with_key_file(session, config).await
    }

    /// Authenticate using spuff's managed ed25519 key.
    async fn auth_with_managed_key(
        session: &mut Handle<ClientHandler>,
        config: &SshConfig,
    ) -> Result<()> {
        let managed_key = crate::ssh::managed_key::load_managed_private_key_russh()?;

        let auth_result = session
            .authenticate_publickey(&config.user, managed_key)
            .await
            .map_err(|e| SpuffError::Ssh(format!("Managed key auth failed: {}", e)))?;

        if auth_result {
            Ok(())
        } else {
            Err(SpuffError::Ssh(
                "Server rejected spuff managed key".to_string(),
            ))
        }
    }

    /// Authenticate using SSH agent.
    async fn auth_with_agent(
        session: &mut Handle<ClientHandler>,
        config: &SshConfig,
    ) -> Result<bool> {
        let socket_path = std::env::var("SSH_AUTH_SOCK")
            .map_err(|_| SpuffError::Ssh("SSH_AUTH_SOCK not set".to_string()))?;

        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to connect to agent: {}", e)))?;

        let mut agent = russh_keys::agent::client::AgentClient::connect(stream);

        let identities = agent
            .request_identities()
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to get agent identities: {}", e)))?;

        tracing::debug!("Agent has {} identities", identities.len());

        for identity in identities {
            tracing::debug!(
                "Trying agent key: comment='{}', algo='{}'",
                identity.comment(),
                identity.algorithm()
            );

            let auth_result = session
                .authenticate_publickey_with(&config.user, identity, &mut agent)
                .await;

            match auth_result {
                Ok(true) => return Ok(true),
                Ok(false) => continue,
                Err(e) => {
                    tracing::debug!("Agent auth error: {}", e);
                    continue;
                }
            }
        }

        Ok(false)
    }

    /// Authenticate using key file directly.
    async fn auth_with_key_file(
        session: &mut Handle<ClientHandler>,
        config: &SshConfig,
    ) -> Result<()> {
        let key = russh_keys::load_secret_key(&config.key_path, None)
            .map_err(|e| SpuffError::Ssh(format!("Failed to load key: {}", e)))?;

        let auth_result = session
            .authenticate_publickey(&config.user, Arc::new(key))
            .await
            .map_err(|e| SpuffError::Ssh(format!("Authentication failed: {}", e)))?;

        if auth_result {
            Ok(())
        } else {
            Err(SpuffError::Ssh(
                "Authentication failed. Key may require passphrase - use ssh-add first."
                    .to_string(),
            ))
        }
    }

    /// Execute a command on the remote host (non-interactive).
    pub async fn exec(&self, command: &str) -> Result<CommandOutput> {
        let session = self.session.lock().await;
        crate::ssh::exec::exec_command(&session, command).await
    }

    /// Start an interactive shell session with PTY.
    pub async fn shell(&self) -> Result<()> {
        let session = self.session.lock().await;
        crate::ssh::pty::interactive_shell(&session).await
    }

    /// Execute an interactive command with PTY.
    pub async fn exec_interactive(&self, command: &str) -> Result<i32> {
        let session = self.session.lock().await;
        crate::ssh::pty::exec_interactive(&session, command).await
    }

    /// Get an SFTP client for file transfers.
    pub async fn sftp(&self) -> Result<SftpClient> {
        let session = self.session.lock().await;
        SftpClient::new(&session).await
    }

    /// Create a local port forward.
    pub async fn forward_local_port(
        &self,
        local_port: u16,
        remote_port: u16,
    ) -> Result<PortForward> {
        crate::ssh::tunnel::create_local_forward(
            self.session.clone(),
            &self.host,
            local_port,
            remote_port,
        )
        .await
    }

    /// Create multiple port forwards.
    pub async fn forward_ports(&self, ports: &[u16]) -> Result<Vec<PortForward>> {
        let mut forwards = Vec::new();
        for &port in ports {
            let forward = self.forward_local_port(port, port).await?;
            forwards.push(forward);
        }
        Ok(forwards)
    }
}

/// Client handler for russh connection callbacks.
pub struct ClientHandler {
    pub host_key_policy: crate::ssh::config::HostKeyPolicy,
}

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = SpuffError;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh_keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        match self.host_key_policy {
            crate::ssh::config::HostKeyPolicy::AcceptAny => Ok(true),
            crate::ssh::config::HostKeyPolicy::AcceptNew => Ok(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_handler_accepts_keys() {
        let handler = ClientHandler {
            host_key_policy: crate::ssh::config::HostKeyPolicy::AcceptAny,
        };
        assert!(matches!(
            handler.host_key_policy,
            crate::ssh::config::HostKeyPolicy::AcceptAny
        ));
    }
}
