//! spuff logs command - View project setup logs from the remote environment.
//!
//! Displays logs from various stages of project setup:
//! - Bundle installation
//! - Package installation
//! - Repository cloning
//! - Service startup
//! - Setup scripts

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;

/// Log category for filtering
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogCategory {
    /// General setup log
    Setup,
    /// Bundle-specific log
    Bundle,
    /// Package installation log
    Packages,
    /// Repository cloning log
    Repositories,
    /// Docker services log
    Services,
    /// Setup script log
    Script,
}

impl LogCategory {
    /// Get the remote log file path for this category
    pub fn log_path(&self, bundle: Option<&str>, script_index: Option<usize>) -> String {
        match self {
            LogCategory::Setup => "/var/log/spuff/setup.log".to_string(),
            LogCategory::Bundle => {
                if let Some(name) = bundle {
                    format!("/var/log/spuff/bundles/{}.log", name)
                } else {
                    "/var/log/spuff/bundles/".to_string()
                }
            }
            LogCategory::Packages => "/var/log/spuff/packages.log".to_string(),
            LogCategory::Repositories => "/var/log/spuff/repositories.log".to_string(),
            LogCategory::Services => "/var/log/spuff/services.log".to_string(),
            LogCategory::Script => {
                if let Some(idx) = script_index {
                    format!("/var/log/spuff/scripts/{:03}.log", idx)
                } else {
                    "/var/log/spuff/scripts/".to_string()
                }
            }
        }
    }
}

/// Execute the logs command
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config: &AppConfig,
    bundle: Option<String>,
    packages: bool,
    repos: bool,
    services: bool,
    script: Option<usize>,
    follow: bool,
    lines: usize,
) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    // Determine which log to show
    let (category, log_path) = if let Some(ref bundle_name) = bundle {
        (
            LogCategory::Bundle,
            LogCategory::Bundle.log_path(Some(bundle_name), None),
        )
    } else if packages {
        (
            LogCategory::Packages,
            LogCategory::Packages.log_path(None, None),
        )
    } else if repos {
        (
            LogCategory::Repositories,
            LogCategory::Repositories.log_path(None, None),
        )
    } else if services {
        (
            LogCategory::Services,
            LogCategory::Services.log_path(None, None),
        )
    } else if let Some(idx) = script {
        (
            LogCategory::Script,
            LogCategory::Script.log_path(None, Some(idx)),
        )
    } else {
        // Default to general setup log
        (LogCategory::Setup, LogCategory::Setup.log_path(None, None))
    };

    // Print header
    print_header(&category, bundle.as_deref(), script);

    if follow {
        // Follow mode - stream logs
        stream_logs(&instance.ip, config, &log_path).await
    } else {
        // One-shot mode - get last N lines
        show_logs(&instance.ip, config, &log_path, lines).await
    }
}

fn print_header(category: &LogCategory, bundle: Option<&str>, script: Option<usize>) {
    let title = match category {
        LogCategory::Setup => "Project Setup Log".to_string(),
        LogCategory::Bundle => format!("Bundle Log: {}", bundle.unwrap_or("all")),
        LogCategory::Packages => "Package Installation Log".to_string(),
        LogCategory::Repositories => "Repository Cloning Log".to_string(),
        LogCategory::Services => "Docker Services Log".to_string(),
        LogCategory::Script => format!("Setup Script #{} Log", script.unwrap_or(0)),
    };

    println!();
    println!("  {}", style(title).cyan().bold());
    println!("  {}", style("─".repeat(40)).dim());
    println!();
}

async fn show_logs(ip: &str, config: &AppConfig, log_path: &str, lines: usize) -> Result<()> {
    let cmd = format!(
        "tail -n {} {} 2>/dev/null || echo 'Log file not found or empty: {}'",
        lines, log_path, log_path
    );

    let output = crate::connector::ssh::run_command(ip, config, &cmd).await?;

    if output.contains("Log file not found") {
        println!("  {}", style(&output).yellow());
        println!();
        println!(
            "  {} Project setup may not have started yet.",
            style("Hint:").dim()
        );
        println!(
            "  {} Run {} to check setup progress.",
            style("").dim(),
            style("spuff status --detailed").cyan()
        );
    } else {
        // Format and colorize log output
        for line in output.lines() {
            print_log_line(line);
        }
    }

    println!();
    Ok(())
}

async fn stream_logs(ip: &str, config: &AppConfig, log_path: &str) -> Result<()> {
    println!(
        "  {} Press Ctrl+C to stop",
        style("Following logs...").dim()
    );
    println!();

    // Use SSH to stream logs with tail -f
    let cmd = format!(
        "tail -f {} 2>/dev/null || (echo 'Waiting for log file...' && while [ ! -f {} ]; do sleep 1; done && tail -f {})",
        log_path, log_path, log_path
    );

    // Run SSH interactively for follow mode
    crate::connector::ssh::exec_interactive(ip, config, &cmd).await?;

    Ok(())
}

fn print_log_line(line: &str) {
    // Parse and colorize log entries
    // Expected format: [timestamp] [LEVEL] [component] message

    if line.contains("[ERROR]") || line.contains("[FAIL]") {
        println!("  {}", style(line).red());
    } else if line.contains("[WARN]") || line.contains("[WARNING]") {
        println!("  {}", style(line).yellow());
    } else if line.contains("[SUCCESS]") || line.contains("[DONE]") || line.contains("[OK]") {
        println!("  {}", style(line).green());
    } else if line.contains("[INFO]") {
        // Info lines with subtle styling
        if let Some(idx) = line.find("[INFO]") {
            let (prefix, rest) = line.split_at(idx + 6);
            print!("  {}", style(prefix).dim());
            println!("{}", rest);
        } else {
            println!("  {}", line);
        }
    } else if line.contains("[CMD]") {
        // Command lines
        if let Some(idx) = line.find("[CMD]") {
            let (prefix, rest) = line.split_at(idx + 5);
            print!("  {}", style(prefix).dim());
            println!("{}", style(rest).cyan());
        } else {
            println!("  {}", style(line).cyan());
        }
    } else if line.contains("[OUTPUT]") {
        // Command output
        println!("  {}", style(line).dim());
    } else {
        println!("  {}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_category_paths() {
        assert_eq!(
            LogCategory::Setup.log_path(None, None),
            "/var/log/spuff/setup.log"
        );
        assert_eq!(
            LogCategory::Bundle.log_path(Some("rust"), None),
            "/var/log/spuff/bundles/rust.log"
        );
        assert_eq!(
            LogCategory::Packages.log_path(None, None),
            "/var/log/spuff/packages.log"
        );
        assert_eq!(
            LogCategory::Script.log_path(None, Some(1)),
            "/var/log/spuff/scripts/001.log"
        );
    }
}
