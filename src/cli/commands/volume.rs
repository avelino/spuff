//! Volume management commands
//!
//! Commands for mounting and managing volumes.
//! Mounts remote VM directories locally using SSHFS.

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::state::StateDb;
use crate::volume::{
    get_install_instructions, SshfsDriver, SshfsLocalCommands, VolumeConfig, VolumeState,
};

/// List configured and mounted volumes
pub async fn list(_config: &AppConfig) -> Result<()> {
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

    match project_config {
        Some(pc) if !pc.volumes.is_empty() => {
            println!(
                "  {:<30} {:<30} {}",
                style("Remote Path").bold(),
                style("Local Mount").bold(),
                style("Status").bold()
            );
            println!("  {}", style("─".repeat(75)).dim());

            for vol in &pc.volumes {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());
                let is_mounted = SshfsLocalCommands::is_mounted(&mount_point).await;

                let status = if is_mounted {
                    style("● mounted").green()
                } else {
                    style("○ not mounted").dim()
                };

                let options = if vol.read_only { " (ro)" } else { "" };

                println!(
                    "  {:<30} {:<30} {}{}",
                    truncate(&vol.target, 28),
                    truncate(&mount_point, 28),
                    status,
                    options
                );
            }

            println!();
            println!(
                "  {} {} configured volume(s)",
                style("•").cyan(),
                pc.volumes.len()
            );

            let mounted_count = volume_state.mounts.len();
            if mounted_count > 0 {
                println!(
                    "  {} {} currently mounted",
                    style("•").green(),
                    mounted_count
                );
            }
        }
        _ => {
            println!("  {}", style("No volumes configured in spuff.yaml").dim());
            println!();
            println!("  Add volumes to your spuff.yaml:");
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
    }

    println!();
    Ok(())
}

/// Show detailed status of volumes
pub async fn status(_config: &AppConfig) -> Result<()> {
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

    match project_config {
        Some(pc) if !pc.volumes.is_empty() => {
            for vol in &pc.volumes {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());
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
                    "  {} VM:{} → {}",
                    status_icon,
                    style(&vol.target).cyan(),
                    style(&mount_point).white()
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

                if vol.read_only {
                    println!("    {} read-only", style("Mode:").dim());
                }

                if let Some(ref err) = mount_status.error {
                    println!("    {} {}", style("Error:").red(), err);
                }

                println!();
            }
        }
        _ => {
            println!("  {}", style("No volumes configured").dim());
        }
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
            // Mount all volumes from project config
            let project_config = ProjectConfig::load_from_cwd().ok().flatten();

            match project_config {
                Some(pc) if !pc.volumes.is_empty() => {
                    println!(
                        "  {} {} volume(s)...",
                        style("Mounting").cyan(),
                        pc.volumes.len()
                    );
                    println!();

                    for vol in &pc.volumes {
                        match mount_single_volume(
                            &instance.ip,
                            &config.ssh_user,
                            &ssh_key_str,
                            vol,
                            Some(&instance.name),
                            pc.base_dir.as_deref(),
                        )
                        .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                println!("  {} {} - {}", style("✕").red().bold(), vol.target, e);
                            }
                        }
                    }
                }
                _ => {
                    println!("  {}", style("No volumes configured in spuff.yaml").dim());
                    println!();
                    println!("  Mount a volume ad-hoc:");
                    println!(
                        "    {}",
                        style("spuff volume mount /home/dev/project:~/mnt/project").cyan()
                    );
                }
            }
        }
    }

    println!();
    Ok(())
}

/// Mount a single volume
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

/// Sync local directory to VM using rsync
async fn sync_to_vm(
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    use tokio::process::Command;

    // Build rsync command
    // rsync -avz --delete -e "ssh -i key -o StrictHostKeyChecking=no" local/ user@ip:remote/
    let ssh_cmd = format!(
        "ssh -i \"{}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
        ssh_key_path
    );

    // Ensure local path ends with / to sync contents, not the directory itself
    let local_source = if local_path.ends_with('/') {
        local_path.to_string()
    } else {
        format!("{}/", local_path)
    };

    let remote_dest = format!("{}@{}:{}", ssh_user, vm_ip, remote_path);

    let output = Command::new("rsync")
        .args([
            "-avz",
            "--delete",
            "-e",
            &ssh_cmd,
            &local_source,
            &remote_dest,
        ])
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

    tracing::info!("Synced {} to {}:{}", local_path, vm_ip, remote_path);
    Ok(())
}

/// Unmount a volume or all volumes
pub async fn unmount(_config: &AppConfig, target: Option<String>, all: bool) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!();

    let mut state = VolumeState::load_or_default();

    if all {
        println!("  {} all volumes...", style("Unmounting").cyan());

        let project_config = ProjectConfig::load_from_cwd().ok().flatten();
        if let Some(pc) = project_config {
            for vol in &pc.volumes {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());
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

    match (target, project_config) {
        (Some(target_path), Some(pc)) => {
            // Remount specific volume
            if let Some(vol) = pc.volumes.iter().find(|v| v.target == target_path) {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());

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
                    pc.base_dir.as_deref(),
                )
                .await?;

                println!("  {} Volume remounted", style("✓").green().bold());
            } else {
                return Err(SpuffError::Volume(format!(
                    "Volume not found in config: {}",
                    target_path
                )));
            }
        }
        (None, Some(pc)) if !pc.volumes.is_empty() => {
            // Remount all volumes
            println!("  {} all volumes...", style("Remounting").cyan());

            for vol in &pc.volumes {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());

                // Unmount first
                SshfsLocalCommands::unmount(&mount_point).await.ok();

                // Mount again
                match mount_single_volume(
                    &instance.ip,
                    &config.ssh_user,
                    &ssh_key_str,
                    vol,
                    Some(&instance.name),
                    pc.base_dir.as_deref(),
                )
                .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        println!("  {} {} - {}", style("✕").red().bold(), vol.target, e);
                    }
                }
            }
        }
        _ => {
            println!("  {}", style("No volumes configured").dim());
        }
    }

    println!();
    Ok(())
}

// Helper function
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}
