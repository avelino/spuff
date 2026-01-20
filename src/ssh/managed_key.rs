//! Managed SSH key for spuff.
//!
//! This module handles a spuff-managed ed25519 SSH key that avoids the RSA SHA2
//! authentication issues with older russh versions.
//!
//! The key is stored at `~/.config/spuff/ssh_key` and is created automatically
//! if it doesn't exist.

use std::path::PathBuf;
use std::sync::Arc;

use ssh_key::{Algorithm, LineEnding, PrivateKey};

use crate::error::{Result, SpuffError};

/// Get the path to the spuff managed SSH key.
pub fn get_managed_key_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| SpuffError::Ssh("Cannot determine config directory".to_string()))?;

    Ok(config_dir.join("spuff").join("ssh_key"))
}

/// Check if the managed key exists.
pub fn managed_key_exists() -> Result<bool> {
    let key_path = get_managed_key_path()?;
    Ok(key_path.exists())
}

/// Generate a new ed25519 SSH key for spuff.
///
/// Creates the key at `~/.config/spuff/ssh_key` with no passphrase.
/// This key is used to authenticate with spuff-provisioned instances.
pub fn generate_managed_key() -> Result<PathBuf> {
    let key_path = get_managed_key_path()?;

    // Create parent directory
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            SpuffError::Ssh(format!("Failed to create config directory: {}", e))
        })?;
    }

    // Generate ed25519 key
    let private_key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519)
        .map_err(|e| SpuffError::Ssh(format!("Failed to generate SSH key: {}", e)))?;

    // Write private key
    let private_openssh = private_key
        .to_openssh(LineEnding::LF)
        .map_err(|e| SpuffError::Ssh(format!("Failed to encode private key: {}", e)))?;

    std::fs::write(&key_path, private_openssh.as_bytes()).map_err(|e| {
        SpuffError::Ssh(format!("Failed to write private key: {}", e))
    })?;

    // Set permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| SpuffError::Ssh(format!("Failed to set key permissions: {}", e)))?;
    }

    // Write public key
    let public_key = private_key.public_key();
    let public_openssh = public_key
        .to_openssh()
        .map_err(|e| SpuffError::Ssh(format!("Failed to encode public key: {}", e)))?;

    let pub_key_path = format!("{}.pub", key_path.display());
    std::fs::write(&pub_key_path, format!("{}\n", public_openssh)).map_err(|e| {
        SpuffError::Ssh(format!("Failed to write public key: {}", e))
    })?;

    tracing::info!("Generated new spuff SSH key at {}", key_path.display());

    Ok(key_path)
}

/// Ensure the managed key exists, creating it if necessary.
///
/// Returns the path to the private key.
pub fn ensure_managed_key() -> Result<PathBuf> {
    if managed_key_exists()? {
        get_managed_key_path()
    } else {
        generate_managed_key()
    }
}

/// Load the managed private key using russh_keys (for authentication).
pub fn load_managed_private_key_russh() -> Result<Arc<russh_keys::PrivateKey>> {
    let key_path = ensure_managed_key()?;

    let private_key = russh_keys::load_secret_key(&key_path, None)
        .map_err(|e| SpuffError::Ssh(format!("Failed to load managed key: {}", e)))?;

    Ok(Arc::new(private_key))
}

/// Get the managed public key as a string (OpenSSH format).
pub fn get_managed_public_key() -> Result<String> {
    let key_path = ensure_managed_key()?;
    let pub_key_path = format!("{}.pub", key_path.display());

    std::fs::read_to_string(&pub_key_path)
        .map_err(|e| SpuffError::Ssh(format!("Failed to read managed public key: {}", e)))
        .map(|s| s.trim().to_string())
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_managed_key() {
        // Use a temp directory for testing
        let temp_dir = tempdir().unwrap();
        let key_path = temp_dir.path().join("ssh_key");

        // Generate key manually at temp path
        let private_key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519).unwrap();
        let private_openssh = private_key.to_openssh(LineEnding::LF).unwrap();
        std::fs::write(&key_path, private_openssh.as_bytes()).unwrap();

        // Verify it's readable
        let loaded = PrivateKey::from_openssh(std::fs::read_to_string(&key_path).unwrap()).unwrap();
        assert_eq!(loaded.algorithm(), Algorithm::Ed25519);
    }

    #[test]
    fn test_key_is_ed25519() {
        let private_key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519).unwrap();
        assert_eq!(private_key.algorithm(), Algorithm::Ed25519);
        assert_eq!(private_key.algorithm().as_str(), "ssh-ed25519");
    }
}
