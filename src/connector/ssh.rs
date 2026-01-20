//! SSH connection utilities using pure Rust.
//!
//! This module provides SSH connectivity using the russh library,
//! eliminating the need for external SSH binaries.

use std::path::PathBuf;
use std::time::Duration;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::ssh::{SshClient, SshConfig};

/// Convert AppConfig to SshConfig for SSH operations.
fn app_config_to_ssh_config(config: &AppConfig) -> SshConfig {
    SshConfig {
        user: config.ssh_user.clone(),
        key_path: PathBuf::from(&config.ssh_key_path),
        host_key_policy: crate::ssh::config::HostKeyPolicy::AcceptNew,
    }
}

/// Wait for SSH port to be reachable (TCP connectivity check).
///
/// This is a pure Rust implementation using tokio TcpStream.
pub async fn wait_for_ssh(host: &str, port: u16, timeout: Duration) -> Result<()> {
    crate::ssh::wait_for_ssh(host, port, timeout).await
}

/// Wait for SSH login to actually work (user exists and key is configured).
///
/// Uses pure Rust SSH library (russh) instead of external ssh binary.
pub async fn wait_for_ssh_login(host: &str, config: &AppConfig, timeout: Duration) -> Result<()> {
    let ssh_config = app_config_to_ssh_config(config);
    crate::ssh::wait_for_ssh_login(host, &ssh_config, timeout).await
}

/// Connect to remote host via SSH (interactive shell).
///
/// Uses pure Rust SSH implementation with PTY support.
pub async fn connect_ssh(host: &str, config: &AppConfig) -> Result<()> {
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(host, 22, &ssh_config).await?;
    client.shell().await
}

/// Connect to remote host (SSH only, mosh support removed).
///
/// Note: mosh support has been removed. Use connect_ssh() directly.
pub async fn connect(host: &str, config: &AppConfig) -> Result<()> {
    connect_ssh(host, config).await
}

/// Execute a command interactively with TTY support.
///
/// Uses pure Rust SSH implementation with PTY allocation.
pub async fn exec_interactive(host: &str, config: &AppConfig, command: &str) -> Result<i32> {
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(host, 22, &ssh_config).await?;
    client.exec_interactive(command).await
}

/// Upload a file to the remote host via SFTP.
///
/// Uses pure Rust SFTP implementation (replaces scp binary).
pub async fn scp_upload(
    host: &str,
    config: &AppConfig,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(host, 22, &ssh_config).await?;
    let sftp = client.sftp().await?;
    sftp.upload(local_path, remote_path).await
}

/// Execute a non-interactive command and return stdout.
///
/// Uses pure Rust SSH implementation.
pub async fn run_command(host: &str, config: &AppConfig, command: &str) -> Result<String> {
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(host, 22, &ssh_config).await?;
    let output = client.exec(command).await?;

    if !output.success {
        // Check for passphrase-related errors
        if output.stderr.contains("Permission denied") || output.stderr.contains("passphrase") {
            return Err(SpuffError::Ssh(
                "SSH key requires passphrase. Run 'ssh-add' first to add your key to the agent."
                    .to_string(),
            ));
        }
        return Err(SpuffError::Ssh(format!("Command failed: {}", output.stderr)));
    }

    Ok(output.stdout)
}

// Note: The following functions have been removed as per the migration plan:
// - is_mosh_available() - mosh support removed
// - connect_mosh() - mosh support removed

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
