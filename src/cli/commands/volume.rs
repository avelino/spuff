//! Volume management commands
//!
//! Commands for mounting and managing volumes.
//! Mounts remote VM directories locally using SSHFS.

use std::path::PathBuf;

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::ssh::{SshClient, SshConfig};
use crate::state::StateDb;
use crate::volume::{
    get_install_instructions, SshfsDriver, SshfsLocalCommands, VolumeConfig, VolumeState,
};

/// List configured and mounted volumes
pub async fn list(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!();
    println!(
        "  {} {}",
        style("Volumes for").dim(),
        style(&instance.name).cyan()
    );
    println!();

    // Get volumes from project config
    let project_config = ProjectConfig::load_from_cwd().ok().flatten();

    // Get current mount state
    let volume_state = VolumeState::load_or_default();

    // Build merged volume list: global volumes first, then project volumes
    // Project volumes with same target override global ones
    let mut merged_volumes: Vec<(&VolumeConfig, Option<&std::path::Path>)> = Vec::new();

    // Add global volumes (no base_dir)
    for vol in &config.volumes {
        merged_volumes.push((vol, None));
    }

    // Add project volumes (with base_dir), overriding globals with same target
    if let Some(ref pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            merged_volumes.retain(|(v, _)| v.target != vol.target);
            merged_volumes.push((vol, pc.base_dir.as_deref()));
        }
    }

    if !merged_volumes.is_empty() {
        println!(
            "  {:<30} {:<30} {} {}",
            style("Remote Path").bold(),
            style("Local Mount").bold(),
            style("Status").bold(),
            style("Source").bold()
        );
        println!("  {}", style("─".repeat(85)).dim());

        for (vol, base_dir) in &merged_volumes {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), *base_dir);
            let is_mounted = SshfsLocalCommands::is_mounted(&mount_point).await;

            let status = if is_mounted {
                style("● mounted").green()
            } else {
                style("○ not mounted").dim()
            };

            let source = if base_dir.is_some() {
                style("project").cyan()
            } else {
                style("global").yellow()
            };

            let options = if vol.read_only { " (ro)" } else { "" };

            println!(
                "  {:<30} {:<30} {}{} {}",
                truncate(&vol.target, 28),
                truncate(&mount_point, 28),
                status,
                options,
                source
            );
        }

        let global_count = config.volumes.len();
        let project_count = project_config
            .as_ref()
            .map(|pc| pc.volumes.len())
            .unwrap_or(0);

        println!();
        if global_count > 0 {
            println!(
                "  {} {} global volume(s)",
                style("•").yellow(),
                global_count
            );
        }
        if project_count > 0 {
            println!(
                "  {} {} project volume(s)",
                style("•").cyan(),
                project_count
            );
        }

        let mounted_count = volume_state.mounts.len();
        if mounted_count > 0 {
            println!(
                "  {} {} currently mounted",
                style("•").green(),
                mounted_count
            );
        }
    } else {
        println!(
            "  {}",
            style("No volumes configured in config.yaml or spuff.yaml").dim()
        );
        println!();
        println!("  Add volumes to your ~/.spuff/config.yaml (global):");
        println!("  or to your project's spuff.yaml (project-specific):");
        println!();
        println!("  {}", style("volumes:").cyan());
        println!(
            "    {}  {}",
            style("-").dim(),
            style("target: /home/dev/project").white()
        );
        println!(
            "      {}  {}",
            style("mount_point:").dim(),
            style("~/mnt/project").white()
        );
    }

    println!();
    Ok(())
}

/// Show detailed status of volumes
pub async fn status(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let project_config = ProjectConfig::load_from_cwd().ok().flatten();

    println!();
    println!(
        "  {} {}",
        style("Volume Status for").dim(),
        style(&instance.name).cyan()
    );
    println!();

    // Check SSHFS availability
    let sshfs_available = SshfsDriver::check_sshfs_installed().await;
    let fuse_available = SshfsDriver::check_fuse_available().await;

    if !sshfs_available || !fuse_available {
        println!("  {} SSHFS not available", style("⚠").yellow().bold());
        println!();
        println!("{}", get_install_instructions());
        println!();
        return Ok(());
    }

    println!("  {} SSHFS installed and ready", style("✓").green().bold());
    println!();

    // Build merged volume list: global volumes first, then project volumes
    // Project volumes with same target override global ones
    let mut merged_volumes: Vec<(&VolumeConfig, Option<&std::path::Path>, bool)> = Vec::new();

    // Add global volumes (no base_dir, is_global = true)
    for vol in &config.volumes {
        merged_volumes.push((vol, None, true));
    }

    // Add project volumes (with base_dir), overriding globals with same target
    if let Some(ref pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            merged_volumes.retain(|(v, _, _)| v.target != vol.target);
            merged_volumes.push((vol, pc.base_dir.as_deref(), false));
        }
    }

    // Get SSH key path for remote checks
    let ssh_key_path = crate::ssh::managed_key::get_managed_key_path()
        .unwrap_or_else(|_| PathBuf::from(&config.ssh_key_path));
    let ssh_key_str = ssh_key_path.to_string_lossy().to_string();

    if !merged_volumes.is_empty() {
        for (vol, base_dir, is_global) in &merged_volumes {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), *base_dir);
            let source_path = vol.resolve_source(*base_dir);

            // Determine if this is a file using SSH check when local source doesn't exist
            let is_file = is_file_volume_async(
                &source_path,
                &vol.target,
                &instance.ip,
                &config.ssh_user,
                &ssh_key_str,
            )
            .await;

            let source_label = if *is_global {
                style("[global]").yellow()
            } else {
                style("[project]").cyan()
            };

            if is_file {
                // For files, check if the local source exists (was synced)
                let source_exists =
                    !source_path.is_empty() && std::path::Path::new(&source_path).exists();

                let (status_icon, status_text) = if source_exists {
                    (style("✓").green(), style("synced (file)").green())
                } else {
                    (style("○").dim(), style("no local source").dim())
                };

                println!(
                    "  {} VM:{} ← {} {}",
                    status_icon,
                    style(&vol.target).cyan(),
                    style(&source_path).white(),
                    source_label
                );
                println!(
                    "    {} {} {}",
                    style("Status:").dim(),
                    status_text,
                    style("(SSHFS doesn't mount files)").dim()
                );
            } else {
                // For directories, check SSHFS mount status
                let mount_status = SshfsLocalCommands::check_status(&mount_point).await?;

                let status_icon = if mount_status.mounted && mount_status.healthy {
                    style("●").green()
                } else if mount_status.mounted {
                    style("●").yellow()
                } else {
                    style("○").red()
                };

                let status_text = if mount_status.mounted && mount_status.healthy {
                    style("healthy").green()
                } else if mount_status.mounted {
                    style("degraded").yellow()
                } else {
                    style("not mounted").red()
                };

                println!(
                    "  {} VM:{} → {} {}",
                    status_icon,
                    style(&vol.target).cyan(),
                    style(&mount_point).white(),
                    source_label
                );
                println!(
                    "    {} {}{}",
                    style("Status:").dim(),
                    status_text,
                    mount_status
                        .latency_ms
                        .map(|l| format!(" ({}ms)", l))
                        .unwrap_or_default()
                );

                if let Some(ref err) = mount_status.error {
                    println!("    {} {}", style("Error:").red(), err);
                }
            }

            if vol.read_only {
                println!("    {} read-only", style("Mode:").dim());
            }

            println!();
        }
    } else {
        println!("  {}", style("No volumes configured").dim());
    }

    Ok(())
}

/// Mount a volume (ad-hoc or from config)
pub async fn mount(config: &AppConfig, spec: Option<&str>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    // Check SSHFS availability
    if !SshfsDriver::check_sshfs_installed().await {
        println!();
        println!("{}", get_install_instructions());
        println!();
        return Err(SpuffError::Volume("SSHFS not installed".to_string()));
    }

    if !SshfsDriver::check_fuse_available().await {
        println!();
        println!("{}", get_install_instructions());
        println!();
        return Err(SpuffError::Volume("FUSE not available".to_string()));
    }

    let ssh_key_path = crate::ssh::managed_key::get_managed_key_path()
        .unwrap_or_else(|_| std::path::PathBuf::from(&config.ssh_key_path));

    let ssh_key_str = ssh_key_path.to_string_lossy().to_string();

    println!();

    match spec {
        Some(volume_spec) => {
            // Mount ad-hoc volume (no project base dir for ad-hoc)
            let volume_config = VolumeConfig::from_spec(volume_spec).map_err(SpuffError::Volume)?;
            mount_single_volume(
                &instance.ip,
                &config.ssh_user,
                &ssh_key_str,
                &volume_config,
                Some(&instance.name),
                None,
            )
            .await?;
        }
        None => {
            // Mount all volumes from global config and project config
            let project_config = ProjectConfig::load_from_cwd().ok().flatten();

            // Build merged volume list: global volumes first, then project volumes
            // Project volumes with same target override global ones
            let mut merged_volumes: Vec<(&VolumeConfig, Option<&std::path::Path>)> = Vec::new();

            // Add global volumes (no base_dir)
            for vol in &config.volumes {
                merged_volumes.push((vol, None));
            }

            // Add project volumes (with base_dir), overriding globals with same target
            if let Some(ref pc) = project_config {
                for vol in &pc.volumes {
                    // Remove any global volume with the same target
                    merged_volumes.retain(|(v, _)| v.target != vol.target);
                    merged_volumes.push((vol, pc.base_dir.as_deref()));
                }
            }

            if !merged_volumes.is_empty() {
                println!(
                    "  {} {} volume(s)...",
                    style("Mounting").cyan(),
                    merged_volumes.len()
                );
                println!();

                for (vol, base_dir) in &merged_volumes {
                    match mount_single_volume(
                        &instance.ip,
                        &config.ssh_user,
                        &ssh_key_str,
                        vol,
                        Some(&instance.name),
                        *base_dir,
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            println!("  {} {} - {}", style("✕").red().bold(), vol.target, e);
                        }
                    }
                }
            } else {
                println!(
                    "  {}",
                    style("No volumes configured in config.yaml or spuff.yaml").dim()
                );
                println!();
                println!("  Mount a volume ad-hoc:");
                println!(
                    "    {}",
                    style("spuff volume mount /home/dev/project:~/mnt/project").cyan()
                );
            }
        }
    }

    println!();
    Ok(())
}

/// Mount a single volume
///
/// For directories: syncs local -> remote, then mounts remote -> local via SSHFS
/// For files: only syncs local -> remote (SSHFS doesn't support mounting individual files)
async fn mount_single_volume(
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
    volume: &VolumeConfig,
    instance_name: Option<&str>,
    project_base_dir: Option<&std::path::Path>,
) -> Result<()> {
    let mount_point = volume.resolve_mount_point(instance_name, project_base_dir);
    let source_path = volume.resolve_source(project_base_dir);

    // Determine if source is a file or directory using SSH check when local source doesn't exist
    let source_is_file =
        is_file_volume_async(&source_path, &volume.target, vm_ip, ssh_user, ssh_key_path).await;

    // For files, we only sync - SSHFS doesn't support mounting individual files
    if source_is_file {
        if !source_path.is_empty() && std::path::Path::new(&source_path).exists() {
            println!(
                "  {} {} → VM:{} {}",
                style("Syncing").cyan(),
                style(&source_path).white(),
                style(&volume.target).green(),
                style("(file)").dim()
            );

            sync_to_vm(vm_ip, ssh_user, ssh_key_path, &source_path, &volume.target).await?;

            println!(
                "  {} {} (synced, SSHFS doesn't support file mounts)",
                style("✓").green().bold(),
                source_path
            );
        } else {
            println!(
                "  {} {} (file target, no local source to sync)",
                style("○").dim(),
                volume.target
            );
        }
        return Ok(());
    }

    // Directory handling (existing behavior)

    // Check if already mounted
    if SshfsLocalCommands::is_mounted(&mount_point).await {
        println!(
            "  {} {} (already mounted)",
            style("✓").green().bold(),
            mount_point
        );
        return Ok(());
    }

    // If source is defined, sync local files to VM first
    if !source_path.is_empty() && std::path::Path::new(&source_path).exists() {
        println!(
            "  {} {} → VM:{}",
            style("Syncing").cyan(),
            style(&source_path).white(),
            style(&volume.target).green()
        );

        sync_to_vm(vm_ip, ssh_user, ssh_key_path, &source_path, &volume.target).await?;
    }

    println!(
        "  {} VM:{} → {}",
        style("Mounting").cyan(),
        style(&volume.target).white(),
        style(&mount_point).green()
    );

    // Execute the mount
    SshfsLocalCommands::mount(
        vm_ip,
        ssh_user,
        ssh_key_path,
        &volume.target,
        &mount_point,
        volume.read_only,
        &volume.options,
    )
    .await?;

    // Save to state - log errors but don't fail the mount
    let mut state = VolumeState::load_or_default();
    let handle = crate::volume::MountHandle::new("sshfs", &volume.target, &mount_point)
        .with_vm_info(vm_ip, ssh_user)
        .with_source(&source_path)
        .with_read_only(volume.read_only);
    state.add_mount(handle);
    if let Err(e) = state.save() {
        tracing::warn!("Failed to save volume state: {}", e);
    }

    println!("  {} {}", style("✓").green().bold(), mount_point);

    Ok(())
}

/// Sync local file or directory to VM using rsync
async fn sync_to_vm(
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
async fn ensure_remote_path_exists(
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

/// Unmount a volume or all volumes
pub async fn unmount(config: &AppConfig, target: Option<String>, all: bool) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!();

    let mut state = VolumeState::load_or_default();

    if all {
        println!("  {} all volumes...", style("Unmounting").cyan());

        let project_config = ProjectConfig::load_from_cwd().ok().flatten();

        // Build merged volume list: global volumes first, then project volumes
        let mut merged_volumes: Vec<(&VolumeConfig, Option<&std::path::Path>)> = Vec::new();

        // Add global volumes (no base_dir)
        for vol in &config.volumes {
            merged_volumes.push((vol, None));
        }

        // Add project volumes (with base_dir), overriding globals with same target
        if let Some(ref pc) = project_config {
            for vol in &pc.volumes {
                // Remove any global volume with the same target
                merged_volumes.retain(|(v, _)| v.target != vol.target);
                merged_volumes.push((vol, pc.base_dir.as_deref()));
            }
        }

        // Unmount all configured volumes
        for (vol, base_dir) in &merged_volumes {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), *base_dir);
            match SshfsLocalCommands::unmount(&mount_point).await {
                Ok(_) => {
                    state.remove_mount(&mount_point);
                    println!("  {} {}", style("✓").green().bold(), mount_point);
                }
                Err(e) => {
                    println!("  {} {} - {}", style("✕").red().bold(), mount_point, e);
                }
            }
        }

        // Also unmount any tracked mounts not in config
        let mount_points: Vec<String> =
            state.mounts.iter().map(|m| m.mount_point.clone()).collect();
        for mp in mount_points {
            match SshfsLocalCommands::unmount(&mp).await {
                Ok(_) => {
                    state.remove_mount(&mp);
                    println!("  {} {}", style("✓").green().bold(), mp);
                }
                Err(e) => {
                    println!("  {} {} - {}", style("✕").red().bold(), mp, e);
                }
            }
        }
    } else if let Some(target_path) = target {
        println!(
            "  {} {}",
            style("Unmounting").cyan(),
            style(&target_path).white()
        );

        // Try to find the mount point - could be target (remote path) or mount_point (local path)
        let mount_point = if let Some(handle) = state.find_mount(&target_path) {
            handle.mount_point.clone()
        } else {
            // Assume it's the local path
            target_path.clone()
        };

        SshfsLocalCommands::unmount(&mount_point).await?;
        state.remove_mount(&target_path);

        println!("  {} Volume unmounted", style("✓").green().bold());
    } else {
        println!(
            "  {} Specify a target path or use --all",
            style("!").yellow().bold()
        );
        return Ok(());
    }

    if let Err(e) = state.save() {
        tracing::warn!("Failed to save volume state after unmount: {}", e);
    }
    println!();
    Ok(())
}

/// Remount volumes (useful after connection issues)
pub async fn remount(config: &AppConfig, target: Option<String>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let project_config = ProjectConfig::load_from_cwd().ok().flatten();

    let ssh_key_path = crate::ssh::managed_key::get_managed_key_path()
        .unwrap_or_else(|_| std::path::PathBuf::from(&config.ssh_key_path));

    let ssh_key_str = ssh_key_path.to_string_lossy().to_string();

    println!();

    // Build merged volume list: global volumes first, then project volumes
    // Project volumes with same target override global ones
    let mut merged_volumes: Vec<(&VolumeConfig, Option<&std::path::Path>)> = Vec::new();

    // Add global volumes (no base_dir)
    for vol in &config.volumes {
        merged_volumes.push((vol, None));
    }

    // Add project volumes (with base_dir), overriding globals with same target
    if let Some(ref pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            merged_volumes.retain(|(v, _)| v.target != vol.target);
            merged_volumes.push((vol, pc.base_dir.as_deref()));
        }
    }

    if let Some(target_path) = target {
        // Remount specific volume
        if let Some((vol, base_dir)) = merged_volumes.iter().find(|(v, _)| v.target == target_path)
        {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), *base_dir);

            println!(
                "  {} {}",
                style("Remounting").cyan(),
                style(&target_path).white()
            );

            // Unmount first
            SshfsLocalCommands::unmount(&mount_point).await.ok();

            // Mount again
            mount_single_volume(
                &instance.ip,
                &config.ssh_user,
                &ssh_key_str,
                vol,
                Some(&instance.name),
                *base_dir,
            )
            .await?;

            println!("  {} Volume remounted", style("✓").green().bold());
        } else {
            return Err(SpuffError::Volume(format!(
                "Volume not found in config: {}",
                target_path
            )));
        }
    } else if !merged_volumes.is_empty() {
        // Remount all volumes
        println!("  {} all volumes...", style("Remounting").cyan());

        for (vol, base_dir) in &merged_volumes {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), *base_dir);

            // Unmount first
            SshfsLocalCommands::unmount(&mount_point).await.ok();

            // Mount again
            match mount_single_volume(
                &instance.ip,
                &config.ssh_user,
                &ssh_key_str,
                vol,
                Some(&instance.name),
                *base_dir,
            )
            .await
            {
                Ok(_) => {}
                Err(e) => {
                    println!("  {} {} - {}", style("✕").red().bold(), vol.target, e);
                }
            }
        }
    } else {
        println!("  {}", style("No volumes configured").dim());
    }

    println!();
    Ok(())
}

/// Check if a remote path is a file (not directory) via SSH
/// Returns true if it's a file, false if it's a directory or doesn't exist
async fn is_file_volume_async(
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

// Helper function - uses char-based truncation to avoid UTF-8 panics
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    } else {
        s.to_string()
    }
}
