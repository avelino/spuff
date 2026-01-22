use console::style;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::Result;
use crate::project_config::{ProjectConfig, ProjectSetupState, SetupStatus};
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;

#[derive(Debug, Deserialize)]
struct DevToolsState {
    started: bool,
    completed: bool,
    tools: Vec<DevTool>,
}

#[derive(Debug, Deserialize)]
struct DevTool {
    #[allow(dead_code)]
    id: String,
    name: String,
    status: DevToolStatus,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum DevToolStatus {
    Pending,
    Installing,
    Done,
    Failed(String),
    Skipped,
}

#[derive(Debug, Deserialize)]
struct BootstrapStatus {
    #[serde(default)]
    bootstrap_status: String,
    #[serde(default)]
    #[allow(dead_code)]
    bootstrap_ready: bool,
}

/// Bootstrap steps in order of execution (minimal bootstrap)
/// Devtools are now installed via agent after bootstrap completes
const BOOTSTRAP_STEPS: &[(&str, &str)] = &[
    ("starting", "Starting bootstrap"),
    ("installing:agent", "Installing spuff-agent"),
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
                        println!(
                            "  {}        {}",
                            style("Status").dim(),
                            format_status(&remote.status.to_string())
                        );
                    }
                    Ok(None) => {
                        println!(
                            "  {}        {}",
                            style("Status").dim(),
                            style("not found").red()
                        );
                    }
                    Err(e) => {
                        println!(
                            "  {}        {} ({})",
                            style("Status").dim(),
                            style("unknown").yellow(),
                            e
                        );
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

                    // Show devtools status if bootstrap is ready
                    if bootstrap.bootstrap_status == "ready" {
                        if let Ok(devtools) = get_devtools_status(&instance.ip, config).await {
                            println!();
                            print_devtools_status(&devtools);
                        }

                        // Show project setup status if project.json exists
                        if let Ok(project_status) = get_project_status(&instance.ip, config).await {
                            println!();
                            print_project_setup_status(&project_status);
                        }
                    }
                }
            }

            // Check for local spuff.yaml
            if let Ok(Some(_pc)) = ProjectConfig::load_from_cwd() {
                if !detailed {
                    println!();
                    println!(
                        "  {} {} spuff status --detailed",
                        style("Project config found (spuff.yaml).").dim(),
                        style("Run").dim()
                    );
                }
            }

            println!();
            println!(
                "  {} spuff ssh    {} spuff down",
                style("→").cyan(),
                style("×").red()
            );
        }
        None => {
            super::ssh::print_banner();
            println!(
                "  {} {}",
                style("○").dim(),
                style("No active environment").dim()
            );
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

    let step_idx = BOOTSTRAP_STEPS.iter().position(|(key, _)| *key == step_key);

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
            StepState::Completed => (style("[x]").green(), style(*step_label).green()),
            StepState::InProgress => (
                style("[>]").yellow().bold(),
                style(*step_label).yellow().bold(),
            ),
            StepState::Pending => (style("[ ]").dim(), style(*step_label).dim()),
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

async fn get_devtools_status(ip: &str, config: &AppConfig) -> Result<DevToolsState> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/devtools 2>/dev/null || echo '{\"started\":false,\"completed\":false,\"tools\":[]}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse devtools response: {}", e))
    })
}

fn print_devtools_status(devtools: &DevToolsState) {
    if !devtools.started {
        println!("  {}", style("Devtools: not started").dim());
        println!(
            "  {}",
            style("Run 'spuff agent devtools install' to start").dim()
        );
        return;
    }

    let status_text = if devtools.completed {
        style("Devtools: installed").green()
    } else {
        style("Devtools: installing...").yellow()
    };

    println!("  {}", status_text);
    println!();

    // Count stats
    let done = devtools
        .tools
        .iter()
        .filter(|t| t.status == DevToolStatus::Done)
        .count();
    let installing = devtools
        .tools
        .iter()
        .filter(|t| t.status == DevToolStatus::Installing)
        .count();
    let failed = devtools
        .tools
        .iter()
        .filter(|t| matches!(t.status, DevToolStatus::Failed(_)))
        .count();
    let total = devtools
        .tools
        .iter()
        .filter(|t| t.status != DevToolStatus::Skipped)
        .count();

    if total > 0 {
        println!(
            "  {} {}/{} installed{}{}",
            style("→").dim(),
            done,
            total,
            if installing > 0 {
                format!(", {} installing", installing)
            } else {
                String::new()
            },
            if failed > 0 {
                format!(", {} failed", style(failed).red())
            } else {
                String::new()
            },
        );
    }

    // Show individual tools
    for tool in &devtools.tools {
        if tool.status == DevToolStatus::Skipped {
            continue;
        }

        let (icon, name_style) = match &tool.status {
            DevToolStatus::Done => (style("[x]").green(), style(&tool.name).green()),
            DevToolStatus::Installing => (
                style("[>]").yellow().bold(),
                style(&tool.name).yellow().bold(),
            ),
            DevToolStatus::Pending => (style("[ ]").dim(), style(&tool.name).dim()),
            DevToolStatus::Failed(_) => (style("[!]").red(), style(&tool.name).red()),
            DevToolStatus::Skipped => continue,
        };

        let version_str = tool
            .version
            .as_ref()
            .map(|v| format!(" ({})", style(v).dim()))
            .unwrap_or_default();

        let error_str = if let DevToolStatus::Failed(e) = &tool.status {
            format!(" - {}", style(e).red())
        } else {
            String::new()
        };

        println!("    {} {}{}{}", icon, name_style, version_str, error_str);
    }
}

async fn get_project_status(ip: &str, config: &AppConfig) -> Result<ProjectSetupState> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/project/status 2>/dev/null || echo '{}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse project status: {}", e))
    })
}

fn print_project_setup_status(status: &ProjectSetupState) {
    // Check if project setup was started
    if !status.started {
        return;
    }

    println!("  {} {}", style("╭").dim(), style("─".repeat(56)).dim());
    println!(
        "  {}  {}",
        style("│").dim(),
        style("Project Setup (spuff.yaml)").cyan().bold()
    );
    println!("  {} {}", style("├").dim(), style("─".repeat(56)).dim());

    // Bundles
    if !status.bundles.is_empty() {
        println!("  {}  {}", style("│").dim(), style("Bundles").dim().bold());
        for bundle in &status.bundles {
            let (icon, name_style) = format_setup_status(&bundle.status, &bundle.name);
            let version_str = bundle
                .version
                .as_ref()
                .map(|v| format!(" ({})", style(v).dim()))
                .unwrap_or_default();
            println!(
                "  {}    {} {}{}",
                style("│").dim(),
                icon,
                name_style,
                version_str
            );
        }
    }

    // Packages
    let packages = &status.packages;
    if !packages.installed.is_empty()
        || !packages.failed.is_empty()
        || packages.status != SetupStatus::Pending
    {
        println!("  {}  {}", style("│").dim(), style("Packages").dim().bold());
        let installed_count = packages.installed.len();
        let failed_count = packages.failed.len();
        let (icon, status_text) = match &packages.status {
            SetupStatus::Done => (
                style("[✓]").green(),
                style(format!("{} installed", installed_count)).green(),
            ),
            SetupStatus::InProgress => (
                style("[>]").yellow().bold(),
                style(format!("{} installed, installing...", installed_count)).yellow(),
            ),
            SetupStatus::Pending => (style("[ ]").dim(), style("pending".to_string()).dim()),
            SetupStatus::Failed(_) => (
                style("[!]").red(),
                style(format!("{} failed", failed_count)).red(),
            ),
            SetupStatus::Skipped => (style("[-]").dim(), style("skipped".to_string()).dim()),
        };
        println!("  {}    {} {}", style("│").dim(), icon, status_text);
    }

    // Services
    let services = &status.services;
    if !services.containers.is_empty() {
        println!(
            "  {}  {}",
            style("│").dim(),
            style("Services (docker-compose)").dim().bold()
        );
        for container in &services.containers {
            let is_running = container.status == "running";
            let (icon, name_style) = if is_running {
                (style("[✓]").green(), style(&container.name).green())
            } else {
                (style("[ ]").dim(), style(&container.name).dim())
            };
            let port_str = container
                .port
                .map(|p| format!(" ({})", style(p).dim()))
                .unwrap_or_default();
            println!(
                "  {}    {} {}{}",
                style("│").dim(),
                icon,
                name_style,
                port_str
            );
        }
    }

    // Repositories
    if !status.repositories.is_empty() {
        println!(
            "  {}  {}",
            style("│").dim(),
            style("Repositories").dim().bold()
        );
        for repo in &status.repositories {
            // Use URL as display name, extract repo name from it
            let display_name = repo
                .url
                .rsplit('/')
                .next()
                .unwrap_or(&repo.url)
                .strip_suffix(".git")
                .unwrap_or(&repo.url);
            let (icon, name_style) = format_setup_status(&repo.status, display_name);
            let path_str = format!(" → {}", style(&repo.path).dim());
            println!(
                "  {}    {} {}{}",
                style("│").dim(),
                icon,
                name_style,
                path_str
            );
        }
    }

    // Setup Scripts
    if !status.scripts.is_empty() {
        println!(
            "  {}  {}",
            style("│").dim(),
            style("Setup Scripts").dim().bold()
        );
        for (i, script) in status.scripts.iter().enumerate() {
            let (icon, _) = format_setup_status(&script.status, &script.command);
            let cmd_display = if script.command.len() > 45 {
                format!("{}...", &script.command[..42])
            } else {
                script.command.clone()
            };
            let cmd_style = match &script.status {
                SetupStatus::Done => style(cmd_display).green(),
                SetupStatus::InProgress => style(cmd_display).yellow().bold(),
                SetupStatus::Pending => style(cmd_display).dim(),
                SetupStatus::Failed(_) => style(cmd_display).red(),
                SetupStatus::Skipped => style(cmd_display).dim(),
            };
            println!(
                "  {}    {} #{} {}",
                style("│").dim(),
                icon,
                i + 1,
                cmd_style
            );
        }
    }

    println!("  {} {}", style("╰").dim(), style("─".repeat(56)).dim());
}

fn format_setup_status<'a>(
    status: &SetupStatus,
    name: &'a str,
) -> (
    console::StyledObject<&'static str>,
    console::StyledObject<&'a str>,
) {
    match status {
        SetupStatus::Done => (style("[✓]").green(), style(name).green()),
        SetupStatus::InProgress => (style("[>]").yellow().bold(), style(name).yellow().bold()),
        SetupStatus::Pending => (style("[ ]").dim(), style(name).dim()),
        SetupStatus::Failed(_) => (style("[!]").red(), style(name).red()),
        SetupStatus::Skipped => (style("[-]").dim(), style(name).dim()),
    }
}
