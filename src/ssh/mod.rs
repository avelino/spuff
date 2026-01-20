//! Pure Rust SSH implementation for spuff.
//!
//! This module replaces external SSH binaries (ssh, scp, ssh-keygen, ssh-add)
//! with native Rust libraries for a self-contained binary.
//!
//! ## Modules
//!
//! - [`keys`] - SSH key parsing, fingerprint calculation, passphrase detection
//! - [`agent`] - SSH agent protocol communication (read-only)
//! - [`client`] - SSH connection management
//! - [`exec`] - Remote command execution
//! - [`sftp`] - File transfer via SFTP
//! - [`tunnel`] - Port forwarding
//! - [`pty`] - Interactive terminal sessions

mod agent;
mod client;
pub mod config;
mod exec;
mod keys;
pub mod managed_key;
mod pty;
mod sftp;
mod tunnel;

// Re-exports for public API
pub use agent::is_key_in_agent;
pub use client::SshClient;
pub use config::SshConfig;
pub use keys::key_has_passphrase;

use std::time::Duration;
use tokio::net::TcpStream;

use crate::error::{Result, SpuffError};

/// Check if SSH agent is running and accessible.
///
/// Verifies that SSH_AUTH_SOCK environment variable is set and the socket exists.
pub fn is_ssh_agent_running() -> bool {
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        std::path::Path::new(&sock).exists()
    } else {
        false
    }
}

/// Wait for SSH port to be open (TCP level check).
///
/// This performs a simple TCP connection check without SSH protocol handshake.
pub async fn wait_for_ssh(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let addr = format!("{}:{}", host, port);

    while start.elapsed() < timeout {
        match TcpStream::connect(&addr).await {
            Ok(_) => return Ok(()),
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    Err(SpuffError::Ssh(format!(
        "Timeout waiting for SSH port on {}",
        addr
    )))
}

/// Wait for SSH login to actually work (user exists and key is configured).
///
/// Attempts SSH connection with authentication to verify the remote is ready.
pub async fn wait_for_ssh_login(host: &str, config: &SshConfig, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match SshClient::connect(host, 22, config).await {
            Ok(client) => {
                // Try a simple command to verify connection works
                match client.exec("echo ok").await {
                    Ok(output) if output.success => return Ok(()),
                    _ => {}
                }
            }
            Err(_) => {}
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Err(SpuffError::Ssh(format!(
        "Timeout waiting for SSH login as {}@{}. Make sure your SSH key is loaded in the agent.",
        config.user, host
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_for_ssh_timeout() {
        // Test against localhost on a port that's unlikely to be listening
        let result = wait_for_ssh("127.0.0.1", 59999, Duration::from_millis(100)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Timeout waiting for SSH"));
    }

    #[tokio::test]
    async fn test_wait_for_ssh_success() {
        // Start a TCP listener to simulate SSH port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a task to accept one connection
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let result = wait_for_ssh("127.0.0.1", port, Duration::from_secs(5)).await;
        assert!(result.is_ok());
    }
}
