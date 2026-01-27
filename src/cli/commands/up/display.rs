//! Display and UI helpers
//!
//! Functions for printing project summaries and progress information.

use console::style;

use crate::project_config::{AiToolsConfig, ProjectConfig};

/// Print project configuration summary
pub fn print_project_summary(project_config: &ProjectConfig) {
    println!();
    println!(
        "  {}",
        style("╭──────────────────────────────────────────────────────────╮").dim()
    );
    println!(
        "  {}  {:<56} {}",
        style("│").dim(),
        style("Project Setup (spuff.yaml)").cyan().bold(),
        style("│").dim()
    );

    if !project_config.bundles.is_empty() {
        let bundles_str = project_config.bundles.join(", ");
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Bundles: {}", bundles_str),
            style("│").dim()
        );
    }

    if !project_config.packages.is_empty() {
        let count = project_config.packages.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Packages: {} to install", count),
            style("│").dim()
        );
    }

    if project_config.services.enabled {
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Services: {}", project_config.services.compose_file),
            style("│").dim()
        );
    }

    if !project_config.repositories.is_empty() {
        let count = project_config.repositories.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Repositories: {} to clone", count),
            style("│").dim()
        );
    }

    if !project_config.ports.is_empty() {
        let ports_str = project_config
            .ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Ports: {} (tunnel via spuff ssh)", ports_str),
            style("│").dim()
        );
    }

    if !project_config.setup.is_empty() {
        let count = project_config.setup.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Setup scripts: {} commands", count),
            style("│").dim()
        );
    }

    // Show volumes if configured
    if !project_config.volumes.is_empty() {
        let count = project_config.volumes.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Volumes: {} mount(s)", count),
            style("│").dim()
        );
    }

    // Show AI tools config if not default (all)
    match &project_config.ai_tools {
        AiToolsConfig::None => {
            println!(
                "  {}  {:<56} {}",
                style("│").dim(),
                format!("AI tools: {}", style("none").yellow()),
                style("│").dim()
            );
        }
        AiToolsConfig::List(tools) => {
            let tools_str = tools.join(", ");
            println!(
                "  {}  {:<56} {}",
                style("│").dim(),
                format!("AI tools: {}", tools_str),
                style("│").dim()
            );
        }
        AiToolsConfig::All => {
            // Default - don't print anything
        }
    }

    println!(
        "  {}",
        style("╰──────────────────────────────────────────────────────────╯").dim()
    );
    println!();
}
