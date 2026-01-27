//! Sync operations for volumes
//!
//! Functions for syncing files between local and remote.

use std::path::PathBuf;

use crate::error::{Result, SpuffError};
use crate::ssh::{SshClient, SshConfig};

/// Sync local file or directory to VM using rsync
pub async fn sync_to_vm(
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    use tokio::process::Command;

    let local_metadata = std::fs::metadata(local_path).map_err(|e| {
        SpuffError::Volume(format!(
            "Failed to get metadata for '{}': {}",
            local_path, e
        ))
    })?;

    let is_file = local_metadata.is_file();

    // Build rsync command
    let ssh_cmd = format!(
        "ssh -i \"{}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
        ssh_key_path
    );

    // For directories: add trailing / to sync contents, not the directory itself
    // For files: use the path as-is
    let local_source = if is_file || local_path.ends_with('/') {
        local_path.to_string()
    } else {
        format!("{}/", local_path)
    };

    // For files, ensure remote parent directory exists
    if is_file {
        if let Some(parent) = std::path::Path::new(remote_path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                ensure_remote_path_exists(vm_ip, ssh_user, ssh_key_path, &parent_str, true).await?;
            }
        }
    } else {
        // For directories, ensure the target directory exists
        ensure_remote_path_exists(vm_ip, ssh_user, ssh_key_path, remote_path, true).await?;
    }

    let remote_dest = format!("{}@{}:{}", ssh_user, vm_ip, remote_path);

    let mut args = vec!["-avz", "-e", &ssh_cmd, &local_source, &remote_dest];

    // Only use --delete for directories, not for single files
    if !is_file {
        args.insert(1, "--delete");
    }

    let output = Command::new("rsync")
        .args(&args)
        .output()
        .await
        .map_err(|e| SpuffError::Volume(format!("Failed to execute rsync: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SpuffError::Volume(format!(
            "Failed to sync to VM: {}",
            stderr
        )));
    }

    tracing::debug!(
        "Synced {} ({}) to {}:{}",
        local_path,
        if is_file { "file" } else { "directory" },
        vm_ip,
        remote_path
    );
    Ok(())
}

/// Ensure a remote path exists (creates directories as needed)
pub async fn ensure_remote_path_exists(
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
    remote_path: &str,
    is_directory: bool,
) -> Result<()> {
    let path_to_create = if is_directory {
        remote_path.to_string()
    } else {
        // For files, create the parent directory
        std::path::Path::new(remote_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    };

    if path_to_create.is_empty() || path_to_create == "/" {
        return Ok(());
    }

    // Use internal SSH implementation
    let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
    let client = SshClient::connect(vm_ip, 22, &config)
        .await
        .map_err(|e| SpuffError::Volume(format!("Failed to connect via SSH: {}", e)))?;

    let command = format!("mkdir -p '{}'", path_to_create.replace('\'', "'\\''"));
    let output = client
        .exec(&command)
        .await
        .map_err(|e| SpuffError::Volume(format!("Failed to create remote path via SSH: {}", e)))?;

    if !output.success {
        return Err(SpuffError::Volume(format!(
            "Failed to create remote path '{}': {}",
            path_to_create, output.stderr
        )));
    }

    tracing::debug!("Ensured remote path exists: {}", path_to_create);
    Ok(())
}

/// Check if a remote path is a file (not directory) via SSH
/// Returns true if it's a file, false if it's a directory or doesn't exist
pub async fn is_file_volume_async(
    source_path: &str,
    target: &str,
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
) -> bool {
    // If source exists locally, use its metadata
    if !source_path.is_empty() && std::path::Path::new(source_path).exists() {
        return std::fs::metadata(source_path)
            .map(|m| m.is_file())
            .unwrap_or(false);
    }

    // If target ends with /, it's definitely a directory
    if target.ends_with('/') {
        return false;
    }

    // Check remotely via SSH if the target exists and is a directory
    let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
    if let Ok(client) = SshClient::connect(vm_ip, 22, &config).await {
        // Use test -d to check if path is a directory, test -f to check if it's a file
        let escaped_target = target.replace('\'', "'\\''");
        let command = format!(
            "if [ -d '{}' ]; then echo 'dir'; elif [ -f '{}' ]; then echo 'file'; else echo 'unknown'; fi",
            escaped_target, escaped_target
        );
        if let Ok(output) = client.exec(&command).await {
            let result = output.stdout.trim();
            match result {
                "dir" => return false,
                "file" => return true,
                _ => {
                    // Path doesn't exist yet - default to directory (safer for SSHFS)
                    return false;
                }
            }
        }
    }

    // Fallback: if SSH check fails, default to directory (safer for SSHFS)
    false
}
