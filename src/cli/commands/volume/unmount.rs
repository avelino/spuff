//! Unmount commands for volumes
//!
//! Commands for unmounting and remounting volumes.

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::state::StateDb;
use crate::volume::{SshfsLocalCommands, VolumeConfig, VolumeState};

use super::mount::mount_single_volume;

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
