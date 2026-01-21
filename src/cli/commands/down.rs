use console::style;
use dialoguer::Confirm;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;
use crate::volume::{SshfsLocalCommands, VolumeState};

pub async fn execute(config: &AppConfig, create_snapshot: bool, force: bool) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    super::ssh::print_banner();

    // Instance info card
    println!(
        "  {} {} {}",
        style("●").green().bold(),
        style(&instance.name).white().bold(),
        style(format!("({})", &instance.ip)).dim()
    );
    println!();
    println!("  {}      {}", style("Provider").dim(), &instance.provider);
    println!("  {}        {}", style("Region").dim(), &instance.region);
    println!("  {}          {}", style("Size").dim(), &instance.size);
    println!(
        "  {}        {}",
        style("Uptime").dim(),
        style(format_elapsed(instance.created_at)).yellow()
    );
    println!();

    if !force {
        let confirmed = Confirm::new()
            .with_prompt(format!(
                "  {} Destroy this instance?",
                style("?").cyan().bold()
            ))
            .default(false)
            .interact()?;

        if !confirmed {
            println!();
            println!("  {}", style("Cancelled.").dim());
            return Ok(());
        }
    }

    // Unmount any locally mounted volumes BEFORE destroying the instance
    // This prevents SSHFS from hanging when the remote server disappears
    let project_config = ProjectConfig::load_from_cwd().ok().flatten();
    let mut volume_state = VolumeState::load().unwrap_or_default();

    // Collect all mount points to unmount
    let mut mount_points_to_unmount: Vec<String> = Vec::new();

    // Add mounts from project config
    if let Some(ref pc) = project_config {
        for vol in &pc.volumes {
            let mount_point = vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());
            if !mount_points_to_unmount.contains(&mount_point) {
                mount_points_to_unmount.push(mount_point);
            }
        }
    }

    // Add tracked mounts from state
    for m in &volume_state.mounts {
        if !mount_points_to_unmount.contains(&m.mount_point) {
            mount_points_to_unmount.push(m.mount_point.clone());
        }
    }

    if !mount_points_to_unmount.is_empty() {
        println!();
        println!(
            "  {} {}",
            style("◐").cyan(),
            style("Unmounting local volumes...").dim()
        );

        for mount_point in &mount_points_to_unmount {
            print!("    {} {}", style("→").dim(), style(&mount_point).white());

            match SshfsLocalCommands::unmount(mount_point).await {
                Ok(_) => {
                    println!(" {}", style("✓").green());
                    volume_state.remove_mount(mount_point);
                }
                Err(e) => {
                    println!(" {}", style("✕").red());
                    tracing::warn!("Failed to unmount {}: {}", mount_point, e);
                    // Continue with other unmounts even if one fails
                }
            }
        }

        // Clear volume state
        volume_state.clear();
        volume_state.save().ok();
    }

    let provider = create_provider(config)?;

    if create_snapshot {
        println!();
        println!(
            "  {} {}",
            style("◐").cyan(),
            style("Creating snapshot...").dim()
        );
        let snapshot_name = format!("{}-snapshot", instance.name);
        match provider.create_snapshot(&instance.id, &snapshot_name).await {
            Ok(snapshot) => {
                println!(
                    "  {} Snapshot: {}",
                    style("✓").green().bold(),
                    style(&snapshot.id).cyan()
                );
            }
            Err(e) => {
                println!("  {} Snapshot failed: {}", style("!").yellow().bold(), e);
            }
        }
    }

    println!();
    println!(
        "  {} {}",
        style("◐").red(),
        style("Destroying instance...").dim()
    );
    provider.destroy_instance(&instance.id).await?;

    db.remove_instance(&instance.id)?;

    println!(
        "  {} Instance {} destroyed.",
        style("✓").green().bold(),
        style(&instance.name).cyan()
    );
    println!();

    Ok(())
}
