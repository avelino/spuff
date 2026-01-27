//! Pre-flight checks before provisioning
//!
//! Verifies SSH keys and SSHFS availability before creating VMs.

use std::path::PathBuf;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::ssh::{is_key_in_agent, is_ssh_agent_running, key_has_passphrase};
use crate::volume::{get_install_instructions, SshfsDriver};

/// Verify SSH key is accessible for authentication.
///
/// Uses pure Rust SSH libraries - no external binaries required.
///
/// Checks:
/// 1. SSH agent is running (SSH_AUTH_SOCK exists)
/// 2. Key is either in the agent OR has no passphrase
///
/// If the key requires a passphrase and is not in the agent, returns an error
/// with instructions for the user to manually add the key.
pub async fn verify_ssh_key_accessible(config: &AppConfig) -> Result<()> {
    let key_path = PathBuf::from(&config.ssh_key_path);

    // Check if key exists
    if !key_path.exists() {
        return Err(SpuffError::Ssh(format!(
            "SSH key not found: {}",
            config.ssh_key_path
        )));
    }

    // Check if key has no passphrase (can be used directly)
    match key_has_passphrase(&key_path) {
        Ok(false) => {
            // Key has no passphrase - can be used directly
            return Ok(());
        }
        Ok(true) => {
            // Key has passphrase - need agent
        }
        Err(e) => {
            return Err(SpuffError::Ssh(format!(
                "Failed to check SSH key: {}. Make sure the key format is supported.",
                e
            )));
        }
    }

    // Key has passphrase - check if agent is running
    if !is_ssh_agent_running() {
        return Err(SpuffError::Ssh(
            "SSH key requires passphrase but ssh-agent is not running.\n\
             Start ssh-agent and add your key:\n  \
             eval \"$(ssh-agent -s)\"\n  \
             ssh-add ~/.ssh/id_ed25519"
                .to_string(),
        ));
    }

    // Check if key is loaded in the agent
    if is_key_in_agent(&key_path).await {
        return Ok(());
    }

    // Key has passphrase but not in agent
    Err(SpuffError::Ssh(format!(
        "SSH key requires passphrase but is not loaded in the agent.\n\
         Add your key to the agent:\n  \
         ssh-add {}\n\n\
         On macOS, you can also use Keychain:\n  \
         ssh-add --apple-use-keychain {}",
        config.ssh_key_path, config.ssh_key_path
    )))
}

/// Verify SSHFS is available before creating a VM.
///
/// This is a pre-flight check to fail fast with helpful instructions
/// rather than creating a VM that won't work as expected.
pub async fn verify_sshfs_available() -> Result<()> {
    let sshfs_installed = SshfsDriver::check_sshfs_installed().await;
    let fuse_available = SshfsDriver::check_fuse_available().await;

    if !sshfs_installed || !fuse_available {
        return Err(SpuffError::Volume(format!(
            "SSHFS is required for volume mounts but is not available.\n\n{}",
            get_install_instructions()
        )));
    }

    Ok(())
}
