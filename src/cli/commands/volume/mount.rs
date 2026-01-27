//! Mount commands for volumes
//!
//! Commands for mounting volumes via SSHFS.

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::state::StateDb;
use crate::volume::{
    get_install_instructions, SshfsDriver, SshfsLocalCommands, VolumeConfig, VolumeState,
};

use super::sync::{is_file_volume_async, sync_to_vm};

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
pub async fn mount_single_volume(
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
