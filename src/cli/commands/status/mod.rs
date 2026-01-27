//! Status command
//!
//! Shows the status of the active development environment.

mod display;
mod format;
mod http;
mod types;

use console::style;

use crate::config::AppConfig;
use crate::error::Result;
use crate::project_config::ProjectConfig;
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;

use display::{print_bootstrap_checklist, print_devtools_status, print_project_setup_status};
use format::{format_bootstrap_status, format_status};
use http::{get_bootstrap_status, get_devtools_status, get_project_status};

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
