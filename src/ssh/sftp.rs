//! SFTP file transfer implementation.
//!
//! Replaces `scp` binary with native SFTP over SSH.

use std::path::Path;

use russh::client::Handle;
use russh_sftp::client::SftpSession;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, SpuffError};
use crate::ssh::client::ClientHandler;

/// SFTP client for file transfers.
pub struct SftpClient {
    session: SftpSession,
}

impl SftpClient {
    /// Create a new SFTP client from an SSH session.
    pub async fn new(ssh_session: &Handle<ClientHandler>) -> Result<Self> {
        let channel = ssh_session
            .channel_open_session()
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to open SFTP channel: {}", e)))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to request SFTP subsystem: {}", e)))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to initialize SFTP: {}", e)))?;

        Ok(Self { session: sftp })
    }

    /// Upload a file to the remote host.
    ///
    /// Replaces: `scp local_path user@host:remote_path`
    pub async fn upload(&self, local_path: impl AsRef<Path>, remote_path: &str) -> Result<()> {
        let local_path = local_path.as_ref();

        // Read local file
        let content = tokio::fs::read(local_path).await.map_err(|e| {
            SpuffError::Ssh(format!(
                "Failed to read local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        // Create remote file
        let mut remote_file = self
            .session
            .create(remote_path)
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to create remote file: {}", e)))?;

        // Write content
        remote_file
            .write_all(&content)
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to write to remote file: {}", e)))?;

        // Ensure data is flushed
        remote_file
            .shutdown()
            .await
            .map_err(|e| SpuffError::Ssh(format!("Failed to close remote file: {}", e)))?;

        Ok(())
    }
}
