use console::style;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;
use crate::utils::{format_bytes, format_duration, truncate};

#[derive(Debug, Deserialize)]
struct AgentStatus {
    uptime_seconds: i64,
    idle_seconds: i64,
    hostname: String,
    cloud_init_done: bool,
    #[serde(default)]
    bootstrap_status: String,
    #[serde(default)]
    #[allow(dead_code)]
    bootstrap_ready: bool,
    agent_version: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct AgentMetrics {
    cpu_usage: f32,
    memory_total: u64,
    memory_used: u64,
    memory_percent: f32,
    disk_total: u64,
    disk_used: u64,
    disk_percent: f32,
    load_avg: LoadAverage,
    hostname: String,
    os: String,
    cpus: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct LoadAverage {
    one: f64,
    five: f64,
    fifteen: f64,
}

#[derive(Debug, Deserialize)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory: u64,
}

#[derive(Debug, Deserialize)]
struct ActivityLogEntry {
    timestamp: String,
    event: String,
    details: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActivityLogResponse {
    entries: Vec<ActivityLogEntry>,
    #[allow(dead_code)]
    count: usize,
}

#[derive(Debug, Deserialize)]
struct ExecLogEntry {
    timestamp: String,
    event: String,
    details: String,
}

#[derive(Debug, Deserialize)]
struct ExecLogResponse {
    entries: Vec<ExecLogEntry>,
    #[allow(dead_code)]
    count: usize,
}

pub async fn status(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!(
        "{} Fetching agent status from {}...\n",
        style("→").cyan().bold(),
        style(&instance.name).cyan()
    );

    let status: AgentStatus = agent_request(&instance.ip, config, "/status").await?;
    let metrics: AgentMetrics = agent_request(&instance.ip, config, "/metrics").await?;

    println!("{}", style("Agent Status").bold().cyan());
    println!("  Version:      {}", style(&status.agent_version).white());
    println!("  Hostname:     {}", style(&status.hostname).white());
    println!(
        "  Uptime:       {}",
        style(format_duration(status.uptime_seconds)).yellow()
    );
    println!(
        "  Idle:         {}",
        style(format_duration(status.idle_seconds)).dim()
    );
    println!(
        "  Cloud-init:   {}",
        if status.cloud_init_done {
            style("done").green()
        } else {
            style("running").yellow()
        }
    );
    println!(
        "  Bootstrap:    {}",
        format_bootstrap_status(&status.bootstrap_status)
    );

    println!("\n{}", style("System Metrics").bold().cyan());
    println!("  OS:           {}", style(&metrics.os).dim());
    println!("  CPUs:         {}", style(metrics.cpus).white());
    println!(
        "  CPU Usage:    {}",
        style(format!("{:.1}%", metrics.cpu_usage)).yellow()
    );
    println!(
        "  Memory:       {} / {} ({:.1}%)",
        style(format_bytes(metrics.memory_used)).white(),
        style(format_bytes(metrics.memory_total)).dim(),
        metrics.memory_percent
    );
    println!(
        "  Disk:         {} / {} ({:.1}%)",
        style(format_bytes(metrics.disk_used)).white(),
        style(format_bytes(metrics.disk_total)).dim(),
        metrics.disk_percent
    );
    println!(
        "  Load:         {:.2} {:.2} {:.2}",
        metrics.load_avg.one, metrics.load_avg.five, metrics.load_avg.fifteen
    );

    // Fetch and display recent activity log
    if let Ok(activity) = agent_request::<ActivityLogResponse>(&instance.ip, config, "/activity?limit=10").await {
        if !activity.entries.is_empty() {
            println!("\n{}", style("Recent Activity").bold().cyan());
            for entry in activity.entries.iter().take(10) {
                let time = &entry.timestamp[11..19]; // Extract HH:MM:SS from ISO timestamp
                let details = entry.details.as_deref().unwrap_or("");
                println!(
                    "  {} {} {}",
                    style(time).dim(),
                    style(&entry.event).white(),
                    style(details).dim()
                );
            }
        }
    }

    Ok(())
}

pub async fn metrics(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let metrics: AgentMetrics = agent_request(&instance.ip, config, "/metrics").await?;

    println!("{}", style("System Metrics").bold().cyan());
    println!("  Hostname:     {}", style(&metrics.hostname).white());
    println!("  OS:           {}", style(&metrics.os).dim());
    println!("  CPUs:         {}", style(metrics.cpus).white());
    println!(
        "  CPU Usage:    {}",
        format_cpu_bar(metrics.cpu_usage)
    );
    println!(
        "  Memory:       {} / {} ({})",
        style(format_bytes(metrics.memory_used)).white(),
        style(format_bytes(metrics.memory_total)).dim(),
        format_percent_colored(metrics.memory_percent)
    );
    println!(
        "  Disk:         {} / {} ({})",
        style(format_bytes(metrics.disk_used)).white(),
        style(format_bytes(metrics.disk_total)).dim(),
        format_percent_colored(metrics.disk_percent)
    );
    println!(
        "  Load Avg:     {:.2} {:.2} {:.2}",
        metrics.load_avg.one, metrics.load_avg.five, metrics.load_avg.fifteen
    );

    Ok(())
}

pub async fn processes(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let procs: Vec<ProcessInfo> = agent_request(&instance.ip, config, "/processes").await?;

    println!("{}", style("Top Processes by CPU").bold().cyan());
    println!(
        "  {:<8} {:<30} {:>10} {:>12}",
        "PID", "NAME", "CPU %", "MEMORY"
    );
    println!("  {}", "-".repeat(64));

    for proc in procs {
        println!(
            "  {:<8} {:<30} {:>9.1}% {:>12}",
            proc.pid,
            truncate(&proc.name, 30),
            proc.cpu_usage,
            format_bytes(proc.memory)
        );
    }

    Ok(())
}

pub async fn exec(config: &AppConfig, command: String) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    // Use SSH with TTY for interactive command execution
    let exit_code = crate::connector::ssh::exec_interactive(&instance.ip, config, &command).await?;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub async fn logs(config: &AppConfig, lines: usize, file: Option<String>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

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

    if follow {
        // Follow mode - poll for new entries
        println!(
            "{} Following agent activity on {} (Ctrl+C to stop)\n",
            style("→").cyan().bold(),
            style(&instance.name).cyan()
        );

        let mut last_timestamp: Option<String> = None;

        loop {
            let activity: ActivityLogResponse = agent_request(
                &instance.ip,
                config,
                &format!("/activity?limit={}", limit)
            ).await?;

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
        let activity: ActivityLogResponse = agent_request(
            &instance.ip,
            config,
            &format!("/activity?limit={}", limit)
        ).await?;

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

    let response: ExecLogResponse = agent_request(
        &instance.ip,
        config,
        &format!("/exec-log?lines={}", lines)
    ).await?;

    if response.entries.is_empty() {
        println!("{}", style("No exec commands logged yet").dim());
    } else {
        println!("{}", style("Exec Log (persistent)").bold().cyan());
        println!(
            "  {:<20} {:<15} {}",
            style("TIMESTAMP").dim(),
            style("EVENT").dim(),
            style("DETAILS").dim()
        );
        println!("  {}", "-".repeat(70));

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
                "  {:<20} {:<15} {}",
                style(time).dim(),
                event_style,
                style(&entry.details).white()
            );
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

fn print_activity_entry(entry: &ActivityLogEntry) {
    let time = if entry.timestamp.len() >= 19 {
        &entry.timestamp[..19]
    } else {
        &entry.timestamp
    };

    let event_style = match entry.event.as_str() {
        "agent_started" => style(&entry.event).green(),
        "exec" => style(&entry.event).cyan(),
        "exec_failed" | "exec_timeout" => style(&entry.event).red(),
        "heartbeat" => style(&entry.event).dim(),
        _ => style(&entry.event).white(),
    };

    let details = entry.details.as_deref().unwrap_or("");
    println!(
        "{} {} {}",
        style(time).dim(),
        event_style,
        style(details).dim()
    );
}

fn format_bootstrap_status(status: &str) -> console::StyledObject<&str> {
    match status {
        "ready" => style(status).green(),
        "pending" | "starting" => style(status).yellow(),
        s if s.starts_with("installing:") => style(status).yellow(),
        "failed" => style(status).red(),
        "unknown" | "" => style("unknown").dim(),
        _ => style(status).white(),
    }
}

fn format_cpu_bar(usage: f32) -> String {
    let bar_width: usize = 20;
    let filled = ((usage / 100.0) * bar_width as f32).round() as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

    let colored_bar = if usage >= 90.0 {
        style(bar).red()
    } else if usage >= 70.0 {
        style(bar).yellow()
    } else {
        style(bar).green()
    };

    format!("{} {:.1}%", colored_bar, usage)
}

fn format_percent_colored(percent: f32) -> console::StyledObject<String> {
    let text = format!("{:.1}%", percent);
    if percent >= 90.0 {
        style(text).red()
    } else if percent >= 70.0 {
        style(text).yellow()
    } else {
        style(text).green()
    }
}

/// Find the start of a JSON array, skipping ANSI escape sequences.
fn find_json_array_start(output: &str) -> Option<usize> {
    let bytes = output.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'[' {
            // Skip if this is an ANSI escape sequence (preceded by ESC 0x1b)
            if i > 0 && bytes[i - 1] == 0x1b {
                continue;
            }
            // Check if it looks like a JSON array: '[{', '["', '[n' (number), or '[]'
            if i + 1 < bytes.len() {
                let next = bytes[i + 1];
                if next == b'{' || next == b'"' || next == b']' || next.is_ascii_digit() {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Extract JSON from output that may contain banner text before/after.
///
/// Priority: whichever appears first in the output (array or object).
fn extract_json(output: &str) -> &str {
    let brace_pos = output.find('{');
    let bracket_pos = find_json_array_start(output);

    // Determine which comes first: array or object
    let (start_pos, is_array) = match (bracket_pos, brace_pos) {
        (Some(b), Some(o)) => {
            if b < o {
                (b, true)
            } else {
                (o, false)
            }
        }
        (Some(b), None) => (b, true),
        (None, Some(o)) => (o, false),
        (None, None) => return output.trim(),
    };

    if is_array {
        // Extract JSON array
        let mut depth = 0;
        let mut end_pos = None;
        for (i, c) in output[start_pos..].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
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
    } else {
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
    }

    output.trim()
}

async fn agent_request<T: serde::de::DeserializeOwned>(
    ip: &str,
    config: &AppConfig,
    endpoint: &str,
) -> Result<T> {
    // Use SSH to tunnel to the agent
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        &format!("curl -s http://127.0.0.1:7575{}", endpoint),
    )
    .await?;

    // Extract JSON from output (may contain banner text from shell profile)
    let json_str = extract_json(&output);

    serde_json::from_str(json_str).map_err(|e| {
        SpuffError::Provider(format!(
            "Failed to parse agent response: {}. Response: {}",
            e, output
        ))
    })
}

