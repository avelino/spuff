//! Volume mounting and syncing
//!
//! Functions for syncing files to VMs and building Docker volume mounts.

use std::path::PathBuf;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::provider::VolumeMount;
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

    // Build rsync command with SSH options
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

    let remote_dest = format!("{}@{}:{}", ssh_user, vm_ip, remote_path);

    // First ensure remote directory/parent exists
    let path_to_create = if is_file {
        // For files, create the parent directory
        std::path::Path::new(remote_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        remote_path.to_string()
    };

    if !path_to_create.is_empty() && path_to_create != "/" {
        // Use internal SSH implementation
        let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
        match SshClient::connect(vm_ip, 22, &config).await {
            Ok(client) => {
                let command = format!("mkdir -p '{}'", path_to_create.replace('\'', "'\\''"));
                match client.exec(&command).await {
                    Ok(output) => {
                        if !output.success {
                            tracing::warn!("Failed to create remote directory: {}", output.stderr);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create remote directory: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to connect via SSH: {}", e);
            }
        }
    }

    // Build rsync args - only use --delete for directories
    let mut rsync_args = vec!["-avz"];
    if !is_file {
        rsync_args.push("--delete");
    }
    rsync_args.extend(["-e", &ssh_cmd, &local_source, &remote_dest]);

    // Run rsync
    let output = Command::new("rsync")
        .args(&rsync_args)
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

/// Build volume mounts for Docker provider from config and project config.
///
/// Merges global volumes with project-specific volumes, converting them to
/// Docker bind mount format for container creation.
pub fn build_docker_volume_mounts(
    config: &AppConfig,
    project_config: Option<&ProjectConfig>,
) -> Vec<VolumeMount> {
    let mut mounts = Vec::new();

    // Add global volumes from config
    for vol in &config.volumes {
        let source = vol.resolve_source(None);
        if !source.is_empty() {
            mounts.push(VolumeMount {
                source,
                target: vol.target.clone(),
                read_only: vol.read_only,
            });
        }
    }

    // Add project volumes (override globals with same target)
    if let Some(pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            mounts.retain(|m| m.target != vol.target);

            let source = vol.resolve_source(pc.base_dir.as_deref());
            if !source.is_empty() {
                mounts.push(VolumeMount {
                    source,
                    target: vol.target.clone(),
                    read_only: vol.read_only,
                });
            }
        }
    }

    mounts
}

/// Check if a source path is a file (not directory)
/// If source exists locally, use its metadata.
/// If source doesn't exist, check remotely via SSH.
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
    // This allows the mount to proceed and the user can see if it fails
    false
}
