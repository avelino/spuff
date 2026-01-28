//! Up command - provision a new development environment
//!
//! This module contains the implementation of the `spuff up` command,
//! split into logical components for maintainability.

mod agent_upload;
mod bootstrap;
mod build;
mod display;
mod preflight;
mod provision;
mod volumes;

use console::style;
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::error::Result;
use crate::project_config::{AiToolsConfig, ProjectConfig};
use crate::state::StateDb;
use crate::tui::{run_progress_ui, ProgressMessage};

use build::{build_linux_agent, LINUX_TARGET};
use display::print_project_summary;
use preflight::{verify_ssh_key_accessible, verify_sshfs_available};
use provision::{provision_instance, ProvisionParams};

// Step constants used across modules
pub const STEP_CLOUD_INIT: usize = 0;
pub const STEP_CREATE: usize = 1;
pub const STEP_WAIT_READY: usize = 2;
pub const STEP_WAIT_SSH: usize = 3;
pub const STEP_BOOTSTRAP: usize = 4;
pub const STEP_VOLUMES: usize = 5;
pub const STEP_UPLOAD_AGENT: usize = 6;

// Bootstrap sub-steps (indices) - minimal bootstrap, devtools installed via agent
pub const SUB_PACKAGES: usize = 0;
pub const SUB_AGENT: usize = 1;

pub async fn execute(
    config: &AppConfig,
    size: Option<String>,
    snapshot: Option<String>,
    region: Option<String>,
    no_connect: bool,
    dev: bool,
    ai_tools: Option<String>,
) -> Result<()> {
    let db = StateDb::open()?;

    if let Some(instance) = db.get_active_instance()? {
        println!(
            "{} Active instance found: {} ({})",
            style("!").yellow().bold(),
            style(&instance.name).cyan(),
            style(&instance.ip).dim()
        );
        println!(
            "Run {} first or {} to connect.",
            style("spuff down").yellow(),
            style("spuff ssh").cyan()
        );
        return Ok(());
    }

    // Load project config from spuff.yaml (if exists)
    let project_config = ProjectConfig::load_from_cwd().ok().flatten();

    // Apply project config overrides (CLI args take precedence)
    let effective_size = size.or_else(|| {
        project_config
            .as_ref()
            .and_then(|p| p.resources.size.clone())
    });
    let effective_region = region.or_else(|| {
        project_config
            .as_ref()
            .and_then(|p| p.resources.region.clone())
    });

    // Process AI tools configuration
    // Priority: CLI > Project config > Global config > Default (all)
    let cli_ai_tools = parse_ai_tools_arg(ai_tools.as_deref());

    let is_docker = config.provider == "docker" || config.provider == "local";

    // Pre-flight checks (skip for Docker which doesn't use SSH)
    if !is_docker {
        // Pre-flight check: verify SSH key is usable
        verify_ssh_key_accessible(config).await?;

        // Pre-flight check: verify SSHFS is available if volumes are configured
        // Check both global config volumes and project volumes
        let has_volumes = !config.volumes.is_empty()
            || project_config
                .as_ref()
                .map(|pc| !pc.volumes.is_empty())
                .unwrap_or(false);
        if has_volumes {
            verify_sshfs_available().await?;
        }
    }

    // In dev mode, build agent for Linux and upload (skip for Docker)
    if dev && !is_docker {
        println!(
            "{} Dev mode: building spuff-agent for Linux ({})",
            style("*").cyan().bold(),
            style(LINUX_TARGET).dim()
        );

        match build_linux_agent().await {
            Ok(path) => {
                println!(
                    "{} Built successfully: {}",
                    style("✓").green().bold(),
                    style(&path).dim()
                );
            }
            Err(e) => {
                println!("{} Failed to build agent: {}", style("✕").red().bold(), e);
                return Err(e);
            }
        }
    }

    // Create channel for progress updates
    let (tx, rx) = mpsc::channel::<ProgressMessage>(32);

    // Define the steps
    let mut steps = vec![
        "Generating cloud-init config".to_string(),
        "Creating instance".to_string(),
        "Waiting for instance".to_string(),
        "Waiting for SSH".to_string(),
        "Running bootstrap".to_string(),
        "Mounting volumes".to_string(),
    ];

    if dev {
        steps.push("Uploading local agent".to_string());
    }

    // Print project config summary if found
    if let Some(ref pc) = project_config {
        print_project_summary(pc);
    }

    // Clone config values for the async task
    let params = ProvisionParams {
        config: config.clone(),
        size: effective_size.clone(),
        snapshot: snapshot.clone(),
        region: effective_region.clone(),
        project_config: project_config.clone(),
        cli_ai_tools: cli_ai_tools.clone(),
        dev,
        db,
    };

    // Run provision in a spawn task and UI in the main task
    // This ensures true concurrent execution
    let provision_task = tokio::spawn(async move {
        let result = provision_instance(params, tx).await;
        tracing::debug!("provision_instance future completed");
        result
    });

    // Run the TUI
    tracing::debug!("Starting progress UI");
    let tui_result = run_progress_ui(steps, rx).await;
    tracing::debug!("Progress UI completed");

    // Use select with timeout to avoid infinite wait
    let provision_result = tokio::select! {
        result = provision_task => {
            tracing::debug!("provision_task returned normally");
            result
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
            tracing::error!("provision_task timeout - forcing completion");
            // The provision already completed (UI received Close), just return success
            Ok(Ok(()))
        }
    };

    // Handle results
    handle_provision_result(config, tui_result, provision_result, no_connect).await
}

fn parse_ai_tools_arg(ai_tools: Option<&str>) -> Option<AiToolsConfig> {
    match ai_tools {
        Some("ask") => {
            // Interactive mode - prompt user to select AI tools
            println!(
                "{} Select AI coding tools to install:",
                style("?").cyan().bold()
            );
            println!("  1. {} (Anthropic)", style("claude-code").cyan());
            println!("  2. {} (OpenAI)", style("codex").cyan());
            println!("  3. {} (open source)", style("opencode").cyan());
            println!("  a. {} - install all", style("all").green());
            println!("  n. {} - install none", style("none").yellow());
            println!();
            print!("Enter your choice (comma-separated numbers, 'all', or 'none'): ");
            use std::io::Write;
            std::io::stdout().flush().ok();

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            let input = input.trim().to_lowercase();

            Some(match input.as_str() {
                "a" | "all" => AiToolsConfig::All,
                "n" | "none" => AiToolsConfig::None,
                _ => {
                    let tools: Vec<String> = input
                        .split(',')
                        .filter_map(|s| match s.trim() {
                            "1" | "claude-code" => Some("claude-code".to_string()),
                            "2" | "codex" => Some("codex".to_string()),
                            "3" | "opencode" => Some("opencode".to_string()),
                            _ => None,
                        })
                        .collect();
                    if tools.is_empty() {
                        AiToolsConfig::All
                    } else {
                        AiToolsConfig::List(tools)
                    }
                }
            })
        }
        Some(arg) => Some(AiToolsConfig::from_cli_arg(arg)),
        None => None, // Will use project/global config defaults
    }
}

async fn handle_provision_result(
    config: &AppConfig,
    tui_result: std::result::Result<Option<(String, String)>, std::io::Error>,
    provision_result: std::result::Result<Result<()>, tokio::task::JoinError>,
    no_connect: bool,
) -> Result<()> {
    let is_docker = config.provider == "docker" || config.provider == "local";

    match (tui_result, provision_result) {
        (Ok(Some((name, ip_or_id))), Ok(Ok(()))) => {
            // Success - print final message
            crate::cli::commands::ssh::print_banner();
            println!(
                "  {} {} {}",
                style("✓").green().bold(),
                style(&name).white().bold(),
                style(format!("({})", &ip_or_id)).dim()
            );
            println!();

            if is_docker {
                println!(
                    "  {}  docker exec -it {} /bin/bash",
                    style("Shell").dim(),
                    &ip_or_id[..12.min(ip_or_id.len())]
                );
                println!(
                    "  {}  container runs until 'spuff down'",
                    style("Note").dim()
                );
            } else {
                println!(
                    "  {}  ssh {}@{}",
                    style("SSH").dim(),
                    config.ssh_user,
                    ip_or_id
                );
                println!(
                    "  {}  auto-destroy after {}",
                    style("Idle").dim(),
                    style(&config.idle_timeout).yellow()
                );
            }

            if !no_connect {
                println!();
                if is_docker {
                    // For Docker, get the container ID from state
                    let db = StateDb::open()?;
                    if let Some(instance) = db.get_active_instance()? {
                        crate::connector::docker::connect(&instance.id).await?;
                    }
                } else {
                    crate::connector::ssh::connect(&ip_or_id, config).await?;
                }
            }
        }
        (Ok(None), _) => {
            // User cancelled
            println!("{} Cancelled by user.", style("!").yellow().bold());
        }
        (Err(e), _) => {
            println!("{} TUI error: {}", style("✕").red().bold(), e);
        }
        (_, Ok(Err(e))) => {
            println!("{} Provisioning failed: {}", style("✕").red().bold(), e);
        }
        (_, Err(e)) => {
            println!("{} Task error: {}", style("✕").red().bold(), e);
        }
    }

    Ok(())
}
