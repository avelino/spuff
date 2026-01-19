use console::style;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::Result;
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;

#[derive(Debug, Deserialize)]
struct BootstrapStatus {
    #[serde(default)]
    bootstrap_status: String,
    #[serde(default)]
    bootstrap_ready: bool,
}

/// Bootstrap steps in order of execution
const BOOTSTRAP_STEPS: &[(&str, &str)] = &[
    ("starting", "Starting bootstrap"),
    ("installing:parallel", "Installing tools (Docker, fzf, bat, eza...)"),
    ("installing:devenv", "Installing dev environment (Node.js, Claude Code)"),
    ("installing:agent", "Installing spuff-agent"),
    ("installing:dotfiles", "Configuring dotfiles"),
    ("installing:tailscale", "Setting up Tailscale"),
    ("ready", "Bootstrap complete"),
];

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum StepState {
    Pending,
    InProgress,
    Completed,
}

pub async fn execute(config: &AppConfig, detailed: bool) -> Result<()> {
    let db = StateDb::open()?;

    match db.get_active_instance()? {
        Some(instance) => {
            super::ssh::print_banner();
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
                "  {}       {}",
                style("Uptime").dim(),
                style(format_elapsed(instance.created_at)).yellow()
            );

            if detailed {
                let provider = create_provider(config)?;
                match provider.get_instance(&instance.id).await {
                    Ok(Some(remote)) => {
                        println!("  {}        {}", style("Status").dim(), format_status(&remote.status.to_string()));
                    }
                    Ok(None) => {
                        println!(
                            "  {}        {}",
                            style("Status").dim(),
                            style("not found").red()
                        );
                    }
                    Err(e) => {
                        println!("  {}        {} ({})", style("Status").dim(), style("unknown").yellow(), e);
                    }
                }

                // Try to get bootstrap status from agent
                if let Ok(bootstrap) = get_bootstrap_status(&instance.ip, config).await {
                    println!(
                        "  {}     {}",
                        style("Bootstrap").dim(),
                        format_bootstrap_status(&bootstrap.bootstrap_status)
                    );

                    // Show bootstrap checklist if not ready
                    if bootstrap.bootstrap_status != "ready" {
                        println!();
                        print_bootstrap_checklist(&bootstrap.bootstrap_status);
                    }
                }
            }

            println!();
            println!("  {} spuff ssh    {} spuff down", style("→").cyan(), style("×").red());
        }
        None => {
            super::ssh::print_banner();
            println!("  {} {}", style("○").dim(), style("No active environment").dim());
            println!();
            println!("  Run {} to create one.", style("spuff up").cyan());
        }
    }

    Ok(())
}

fn format_status(status: &str) -> console::StyledObject<&str> {
    match status.to_lowercase().as_str() {
        "active" | "running" => style(status).green(),
        "new" | "starting" => style(status).yellow(),
        _ => style(status).red(),
    }
}

fn format_bootstrap_status(status: &str) -> console::StyledObject<String> {
    let display = match status {
        "ready" => "ready".to_string(),
        "pending" => "pending".to_string(),
        "starting" => "starting...".to_string(),
        s if s.starts_with("installing:") => {
            let phase = s.strip_prefix("installing:").unwrap_or(s);
            format!("installing ({})", phase)
        }
        "failed" => "failed".to_string(),
        "" | "unknown" => "unknown".to_string(),
        other => other.to_string(),
    };

    match status {
        "ready" => style(display).green(),
        "failed" => style(display).red(),
        "" | "unknown" => style(display).dim(),
        _ => style(display).yellow(),
    }
}

/// Get the state of a step based on current status
fn get_step_state(step_key: &str, current_status: &str) -> StepState {
    // Find the index of the current status in the steps
    let current_idx = BOOTSTRAP_STEPS
        .iter()
        .position(|(key, _)| *key == current_status || current_status.starts_with(key));

    let step_idx = BOOTSTRAP_STEPS
        .iter()
        .position(|(key, _)| *key == step_key);

    match (current_idx, step_idx) {
        (Some(curr), Some(step)) => {
            if step < curr {
                StepState::Completed
            } else if step == curr {
                StepState::InProgress
            } else {
                StepState::Pending
            }
        }
        _ => StepState::Pending,
    }
}

/// Print a visual checklist of bootstrap progress
fn print_bootstrap_checklist(current_status: &str) {
    println!("  {}", style("Bootstrap Progress").dim().bold());
    println!();

    for (step_key, step_label) in BOOTSTRAP_STEPS {
        // Skip optional steps that might not apply
        if *step_key == "installing:dotfiles" || *step_key == "installing:tailscale" {
            // Only show if we're at or past this step
            let state = get_step_state(step_key, current_status);
            if state == StepState::Pending && current_status != *step_key {
                continue;
            }
        }

        let state = get_step_state(step_key, current_status);

        let (icon, styled_label) = match state {
            StepState::Completed => (
                style("[x]").green(),
                style(*step_label).green(),
            ),
            StepState::InProgress => (
                style("[>]").yellow().bold(),
                style(*step_label).yellow().bold(),
            ),
            StepState::Pending => (
                style("[ ]").dim(),
                style(*step_label).dim(),
            ),
        };

        println!("    {} {}", icon, styled_label);
    }
}

async fn get_bootstrap_status(ip: &str, config: &AppConfig) -> Result<BootstrapStatus> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/status 2>/dev/null || echo '{}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse agent response: {}", e))
    })
}
