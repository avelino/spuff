use console::style;
use dialoguer::Confirm;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;
use crate::volume::{SshfsLocalCommands, VolumeState};

/// Response from the shutdown endpoint.
#[derive(Debug, Deserialize)]
struct ShutdownResponse {
    success: bool,
    #[allow(dead_code)]
    message: String,
    steps: Vec<ShutdownStep>,
    duration_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ShutdownStep {
    name: String,
    success: bool,
    message: String,
}

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

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    // Step 1: Graceful shutdown on the remote VM (skip for Docker)
    // This runs pre_down hooks and stops docker-compose services
    if is_docker {
        println!(
            "  {} {}",
            style("○").dim(),
            style("Graceful shutdown skipped (Docker container)").dim()
        );
    } else {
        println!(
            "  {} {}",
            style("◐").cyan(),
            style("Running graceful shutdown on VM...").dim()
        );

        match graceful_shutdown(&instance.ip, config).await {
            Ok(response) => {
                if response.success {
                    println!(
                        "  {} Graceful shutdown completed in {}ms",
                        style("✓").green().bold(),
                        response.duration_ms
                    );
                } else {
                    println!(
                        "  {} Graceful shutdown completed with warnings",
                        style("!").yellow().bold()
                    );
                }
                // Print step details if verbose
                for step in &response.steps {
                    let icon = if step.success {
                        style("✓").green()
                    } else {
                        style("✕").red()
                    };
                    println!("    {} {} - {}", icon, step.name, step.message);
                }
            }
            Err(e) => {
                // Log the error but continue - VM might already be unreachable
                println!(
                    "  {} Graceful shutdown skipped: {}",
                    style("!").yellow().bold(),
                    e
                );
            }
        }
    }
    println!();

    // Step 2: Unmount any locally mounted volumes BEFORE destroying the instance
    // This prevents SSHFS from hanging when the remote server disappears
    // Skip for Docker as it uses bind mounts, not SSHFS
    if !is_docker {
        let project_config = ProjectConfig::load_from_cwd().ok().flatten();
        let mut volume_state = VolumeState::load_or_default();

        // Collect all mount points to unmount
        let mut mount_points_to_unmount: Vec<String> = Vec::new();

        // Add mounts from project config
        if let Some(ref pc) = project_config {
            for vol in &pc.volumes {
                let mount_point =
                    vol.resolve_mount_point(Some(&instance.name), pc.base_dir.as_deref());
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

            // Clear volume state - log errors but don't fail the down operation
            volume_state.clear();
            if let Err(e) = volume_state.save() {
                tracing::warn!("Failed to clear volume state: {}", e);
            }
        }
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

    // Explicitly drop the database to release the lock file before the process exits.
    // ChronDB (Tantivy/Lucene) needs time to properly release its write.lock.
    drop(db);

    println!(
        "  {} Instance {} destroyed.",
        style("✓").green().bold(),
        style(&instance.name).cyan()
    );
    println!();

    Ok(())
}

/// Call the agent's /shutdown endpoint for graceful shutdown.
async fn graceful_shutdown(ip: &str, config: &AppConfig) -> Result<ShutdownResponse> {
    // Use SSH to tunnel to the agent and POST to /shutdown
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s -X POST http://127.0.0.1:7575/shutdown",
    )
    .await?;

    // Extract JSON from output (may contain banner text from shell profile)
    let json_str = extract_json(&output);

    serde_json::from_str(json_str).map_err(|e| {
        SpuffError::Provider(format!(
            "Failed to parse shutdown response: {}. Response: {}",
            e, output
        ))
    })
}

/// Extract JSON from output that may contain banner text before/after.
fn extract_json(output: &str) -> &str {
    let brace_pos = output.find('{');

    let Some(start_pos) = brace_pos else {
        return output.trim();
    };

    // Extract JSON object
    let mut depth = 0;
    let mut end_pos = None;
    for (i, c) in output[start_pos..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(start_pos + i);
                    break;
                }
            }
            _ => {}
        }
    }
    if let Some(end) = end_pos {
        return &output[start_pos..=end];
    }

    output.trim()
}
