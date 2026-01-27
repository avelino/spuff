//! Display functions for status output
//!
//! Functions for printing status information to the console.

use console::style;

use crate::project_config::{ProjectSetupState, SetupStatus};

use super::format::{format_setup_status, get_step_state};
use super::types::{DevToolStatus, DevToolsState, StepState, BOOTSTRAP_STEPS};

/// Print a visual checklist of bootstrap progress
pub fn print_bootstrap_checklist(current_status: &str) {
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

pub fn print_devtools_status(devtools: &DevToolsState) {
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

pub fn print_project_setup_status(status: &ProjectSetupState) {
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
