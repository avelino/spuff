//! Logs commands
//!
//! Commands for viewing logs, activity, and exec history.

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;

use super::docker;
use super::format::{print_activity_entry, unescape_output};
use super::http::agent_request;
use super::types::{ActivityLogResponse, ExecLogResponse};

pub async fn logs(config: &AppConfig, lines: usize, file: Option<String>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker::docker_logs(&instance.id, lines, file).await;
    }

    let mut url = format!("/logs?lines={}", lines);
    if let Some(f) = file {
        url.push_str(&format!("&file={}", f));
    }

    let response: serde_json::Value = agent_request(&instance.ip, config, &url).await?;

    if let Some(lines) = response["lines"].as_array() {
        for line in lines {
            if let Some(s) = line.as_str() {
                println!("{}", s);
            }
        }
    }

    Ok(())
}

/// Show agent activity logs (what the agent has executed)
pub async fn activity(config: &AppConfig, limit: usize, follow: bool) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        println!(
            "{} Activity logs not available for Docker containers.",
            style("i").blue().bold()
        );
        println!(
            "  {} Use {} to view container logs.",
            style("→").dim(),
            style("spuff logs").cyan()
        );
        return Ok(());
    }

    if follow {
        // Follow mode - poll for new entries
        println!(
            "{} Following agent activity on {} (Ctrl+C to stop)\n",
            style("→").cyan().bold(),
            style(&instance.name).cyan()
        );

        let mut last_timestamp: Option<String> = None;

        loop {
            let activity: ActivityLogResponse =
                agent_request(&instance.ip, config, &format!("/activity?limit={}", limit)).await?;

            for entry in activity.entries.iter().rev() {
                // Only print entries newer than what we've seen
                let should_print = match &last_timestamp {
                    None => true,
                    Some(last) => entry.timestamp > *last,
                };

                if should_print {
                    print_activity_entry(entry);
                    last_timestamp = Some(entry.timestamp.clone());
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    } else {
        // One-shot mode
        let activity: ActivityLogResponse =
            agent_request(&instance.ip, config, &format!("/activity?limit={}", limit)).await?;

        if activity.entries.is_empty() {
            println!("{}", style("No activity logged yet").dim());
        } else {
            println!("{}", style("Agent Activity Log").bold().cyan());
            println!();
            // Print in chronological order (oldest first)
            for entry in activity.entries.iter().rev() {
                print_activity_entry(entry);
            }
        }
    }

    Ok(())
}

/// Show persistent exec log (all commands executed via agent, survives restarts)
pub async fn exec_log(config: &AppConfig, lines: usize) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        println!(
            "{} Exec logs not available for Docker containers.",
            style("i").blue().bold()
        );
        println!(
            "  {} Docker doesn't maintain a persistent exec log.",
            style("→").dim()
        );
        println!(
            "  {} Use {} to execute commands.",
            style("→").dim(),
            style("spuff exec <command>").cyan()
        );
        return Ok(());
    }

    let response: ExecLogResponse =
        agent_request(&instance.ip, config, &format!("/exec-log?lines={}", lines)).await?;

    if response.entries.is_empty() {
        println!("{}", style("No exec commands logged yet").dim());
    } else {
        println!("{}", style("Exec Log (persistent)").bold().cyan());
        println!();

        for entry in &response.entries {
            let time = if entry.timestamp.len() >= 19 {
                &entry.timestamp[..19]
            } else {
                &entry.timestamp
            };

            let event_style = match entry.event.as_str() {
                "exec" => style(&entry.event).cyan(),
                "exec_failed" | "exec_timeout" => style(&entry.event).red(),
                _ => style(&entry.event).white(),
            };

            println!(
                "  {} {} {}",
                style(time).dim(),
                event_style,
                style(&entry.details).white()
            );

            // Show stdout if present
            if let Some(stdout) = &entry.stdout {
                if !stdout.is_empty() {
                    let output = unescape_output(stdout);
                    println!(
                        "    {} {}",
                        style("stdout:").green().dim(),
                        style(&output).dim()
                    );
                }
            }

            // Show stderr if present
            if let Some(stderr) = &entry.stderr {
                if !stderr.is_empty() {
                    let output = unescape_output(stderr);
                    println!(
                        "    {} {}",
                        style("stderr:").red().dim(),
                        style(&output).dim()
                    );
                }
            }
        }

        println!();
        println!(
            "  {} Total entries: {}",
            style("→").dim(),
            style(response.count).cyan()
        );
    }

    Ok(())
}
