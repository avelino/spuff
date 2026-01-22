//! SSH key management - fingerprint calculation and passphrase detection.
//!
//! Replaces `ssh-keygen` binary calls with pure Rust implementations.

use std::path::Path;

use ssh_key::{HashAlg, PrivateKey};

use crate::error::{Result, SpuffError};

/// Check if an SSH key has a passphrase.
///
/// Returns `true` if the key is encrypted (requires passphrase), `false` if unencrypted.
/// Replaces: `ssh-keygen -y -P "" -f <key>`
pub fn key_has_passphrase(path: impl AsRef<Path>) -> Result<bool> {
    let path = path.as_ref();

    // Use russh_keys to try loading without passphrase
    match russh_keys::load_secret_key(path, None) {
        Ok(_) => Ok(false),
        Err(e) => {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("encrypted")
                || err_str.contains("passphrase")
                || err_str.contains("decrypt")
                || err_str.contains("password")
            {
                Ok(true)
            } else {
                tracing::debug!("load_secret_key error: {}", e);
                Ok(true)
            }
        }
    }
}

/// Calculate the SHA256 fingerprint of an SSH key.
///
/// Returns fingerprint in the format: `SHA256:base64hash`
/// Replaces: `ssh-keygen -lf <key>`
pub fn key_fingerprint(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();

    let key_data = std::fs::read_to_string(path).map_err(|e| {
        SpuffError::Ssh(format!("Failed to read key file {}: {}", path.display(), e))
    })?;

    // Try to parse as private key first
    if let Ok(private_key) = PrivateKey::from_openssh(&key_data) {
        let public_key = private_key.public_key();
        let fingerprint = public_key.fingerprint(HashAlg::Sha256);
        return Ok(fingerprint.to_string());
    }

    // Try to parse as public key
    if let Ok(public_key) = ssh_key::PublicKey::from_openssh(&key_data) {
        let fingerprint = public_key.fingerprint(HashAlg::Sha256);
        return Ok(fingerprint.to_string());
    }

    Err(SpuffError::Ssh(format!(
        "Failed to parse SSH key from {}",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_key() -> NamedTempFile {
        use ssh_key::{Algorithm, LineEnding};

        let key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519).unwrap();
        let openssh = key.to_openssh(LineEnding::LF).unwrap();

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(openssh.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_key_has_no_passphrase() {
        let file = create_test_key();
        let has_passphrase = key_has_passphrase(file.path()).unwrap();
        assert!(!has_passphrase);
    }

    #[test]
    fn test_key_fingerprint() {
        let file = create_test_key();
        let fp = key_fingerprint(file.path()).unwrap();
        assert!(fp.starts_with("SHA256:"));
    }
}
