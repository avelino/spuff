//! SSH agent protocol communication.
//!
//! Provides read-only access to SSH agent for checking if keys are loaded.
//!
//! Replaces: `ssh-add -l`
//!
//! NOTE: This module does NOT start the agent or add keys.
//! Users must have ssh-agent running and keys added before using spuff.

use std::path::Path;

use base64::Engine;
use russh_keys::PublicKeyBase64;
use sha2::{Digest, Sha256};
use tokio::net::UnixStream;

use crate::error::{Result, SpuffError};
use crate::ssh::keys::key_fingerprint;

/// Check if a specific key is loaded in the SSH agent.
///
/// Compares fingerprints to determine if the key is available.
pub async fn is_key_in_agent(key_path: impl AsRef<Path>) -> bool {
    let key_path = key_path.as_ref();

    // Get fingerprint of the key file
    let file_fingerprint = match key_fingerprint(key_path) {
        Ok(fp) => fp,
        Err(_) => return false,
    };

    // List keys in agent
    let agent_fingerprints = match list_agent_fingerprints().await {
        Ok(fps) => fps,
        Err(_) => return false,
    };

    // Check if any agent key matches
    agent_fingerprints.iter().any(|fp| fp == &file_fingerprint)
}

/// List fingerprints of all keys currently loaded in the SSH agent.
async fn list_agent_fingerprints() -> Result<Vec<String>> {
    let socket_path = std::env::var("SSH_AUTH_SOCK")
        .map_err(|_| SpuffError::Ssh("SSH_AUTH_SOCK not set. Is ssh-agent running?".to_string()))?;

    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to connect to SSH agent: {}", e)))?;

    let mut agent = russh_keys::agent::client::AgentClient::connect(stream);

    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to list agent keys: {}", e)))?;

    let fingerprints: Vec<String> = identities
        .iter()
        .map(|id| calculate_fingerprint(&id.public_key_bytes()))
        .collect();

    Ok(fingerprints)
}

/// Calculate SHA256 fingerprint from public key bytes.
fn calculate_fingerprint(key_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key_bytes);
    let hash = hasher.finalize();

    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(hash);
    format!("SHA256:{}", b64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_fingerprint() {
        let test_data = b"test public key data";
        let fp = calculate_fingerprint(test_data);
        assert!(fp.starts_with("SHA256:"));
    }
}
