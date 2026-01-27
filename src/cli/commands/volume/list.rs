//! List and status commands for volumes
//!
//! Commands for viewing configured and mounted volumes.

use std::path::PathBuf;

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::state::StateDb;
use crate::volume::{
    get_install_instructions, SshfsDriver, SshfsLocalCommands, VolumeConfig, VolumeState,
};

use super::sync::is_file_volume_async;

/// Helper function - uses char-based truncation to avoid UTF-8 panics
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    } else {
        s.to_string()
    }
}

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
