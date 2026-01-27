use std::io::IsTerminal;

use console::style;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;
use crate::utils::{format_bytes, format_duration, truncate};

/// Commands known to require interactive TTY (editors, pagers, monitors, REPLs, shells).
const INTERACTIVE_COMMANDS: &[&str] = &[
    // Shells
    "bash", "sh", "zsh", "fish", "dash", "ksh", "csh", "tcsh", // Editors
    "vim", "vi", "nano", "emacs", "nvim", "helix", "hx", // Pagers
    "less", "more", "man", // Monitors
    "htop", "top", "iotop", "btop", "glances", "nmon", // REPLs
    "python", "python3", "node", "irb", "iex", "ghci", "lua", "erl", // Multiplexers
    "tmux", "screen", // Interactive git commands (partial matches handled separately)
    "tig",
];

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
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecLogResponse {
    entries: Vec<ExecLogEntry>,
    #[allow(dead_code)]
    count: usize,
}

/// Response from agent's /exec endpoint.
#[derive(Debug, Deserialize)]
struct AgentExecResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    #[allow(dead_code)]
    duration_ms: u64,
}

pub async fn status(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker_status(&instance.id).await;
    }

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
    if let Ok(activity) =
        agent_request::<ActivityLogResponse>(&instance.ip, config, "/activity?limit=10").await
    {
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

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker_metrics(&instance.id).await;
    }

    let metrics: AgentMetrics = agent_request(&instance.ip, config, "/metrics").await?;

    println!("{}", style("System Metrics").bold().cyan());
    println!("  Hostname:     {}", style(&metrics.hostname).white());
    println!("  OS:           {}", style(&metrics.os).dim());
    println!("  CPUs:         {}", style(metrics.cpus).white());
    println!("  CPU Usage:    {}", format_cpu_bar(metrics.cpu_usage));
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

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker_processes(&instance.id).await;
    }

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

/// Execute a command on the remote environment.
///
/// Automatically chooses between:
/// - Agent HTTP: faster, lower overhead, good for non-interactive commands
/// - SSH with TTY: required for interactive commands (editors, REPLs, etc.)
///
/// The decision can be overridden with `-t` (force TTY) or `-T` (force no TTY).
pub async fn exec(
    config: &AppConfig,
    command: String,
    force_tty: bool,
    no_tty: bool,
) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    let needs_tty = match (force_tty, no_tty) {
        (true, _) => true,  // -t flag: force TTY
        (_, true) => false, // -T flag: force no TTY
        _ => detect_needs_tty(&command),
    };

    if is_docker {
        // Docker mode: use docker exec
        if needs_tty {
            let exit_code =
                crate::connector::docker::exec_interactive(&instance.id, &command).await?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        } else {
            let output = crate::connector::docker::run_command(&instance.id, &command).await?;
            print!("{}", output);
        }
    } else {
        // Cloud mode: use SSH
        if needs_tty {
            // Interactive mode via SSH with PTY
            let exit_code =
                crate::connector::ssh::exec_interactive(&instance.ip, config, &command).await?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        } else {
            // Non-interactive mode via agent HTTP (faster)
            exec_via_agent(&instance.ip, config, &command).await?;
        }
    }

    Ok(())
}

/// Detect if a command needs TTY based on:
/// 1. Whether stdout is a terminal (if piped, no TTY needed)
/// 2. Whether the command is in the list of known interactive commands
fn detect_needs_tty(command: &str) -> bool {
    // If stdout is not a terminal (e.g., piped), don't need TTY
    if !std::io::stdout().is_terminal() {
        return false;
    }

    // Check if command is known to be interactive
    is_interactive_command(command)
}

/// Check if a command is known to require interactive TTY.
fn is_interactive_command(command: &str) -> bool {
    // Get the base command (first word)
    let base_cmd = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");

    // Check direct match
    if INTERACTIVE_COMMANDS.contains(&base_cmd) {
        return true;
    }

    // Check for interactive git commands
    if base_cmd == "git" {
        let cmd_lower = command.to_lowercase();
        if cmd_lower.contains(" -i")
            || cmd_lower.contains(" -p")
            || cmd_lower.contains(" add -i")
            || cmd_lower.contains(" add -p")
            || cmd_lower.contains(" rebase -i")
        {
            return true;
        }
    }

    false
}

/// Execute command via agent HTTP endpoint.
async fn exec_via_agent(ip: &str, config: &AppConfig, command: &str) -> Result<()> {
    let response: AgentExecResponse = agent_request_post(
        ip,
        config,
        "/exec",
        &serde_json::json!({ "command": command }),
    )
    .await?;

    // Output stdout/stderr
    if !response.stdout.is_empty() {
        print!("{}", response.stdout);
    }
    if !response.stderr.is_empty() {
        eprint!("{}", response.stderr);
    }

    if response.exit_code != 0 {
        std::process::exit(response.exit_code);
    }

    Ok(())
}

pub async fn logs(config: &AppConfig, lines: usize, file: Option<String>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker_logs(&instance.id, lines, file).await;
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

/// Unescape output from log (convert \\n back to actual newlines for display)
fn unescape_output(s: &str) -> String {
    // Truncate long output for display
    let truncated = if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s.to_string()
    };

    // Keep it single-line but show literal \n for readability
    truncated
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

/// Make a POST request to the agent with JSON body.
async fn agent_request_post<T: serde::de::DeserializeOwned>(
    ip: &str,
    config: &AppConfig,
    endpoint: &str,
    body: &serde_json::Value,
) -> Result<T> {
    // Escape single quotes in JSON for shell
    let json_body = body.to_string().replace('\'', "'\\''");

    let output = crate::connector::ssh::run_command(
        ip,
        config,
        &format!(
            "curl -s -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:7575{}",
            json_body, endpoint
        ),
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

// ============================================================================
// Docker-specific implementations
// ============================================================================

/// Show Docker container status.
async fn docker_status(container_id: &str) -> Result<()> {
    println!("{}", style("Container Status").bold().cyan());

    // Get container info via docker exec
    let output = crate::connector::docker::run_command(container_id, "hostname").await?;
    println!("  Hostname:     {}", style(output.trim()).white());

    // Get uptime
    let uptime = crate::connector::docker::run_command(
        container_id,
        "cat /proc/uptime 2>/dev/null || echo '0'",
    )
    .await
    .unwrap_or_else(|_| "0".to_string());
    let uptime_secs: f64 = uptime
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    println!(
        "  Uptime:       {}",
        style(format_duration(uptime_secs as i64)).yellow()
    );

    // Get OS info
    let os_info = crate::connector::docker::run_command(
        container_id,
        "cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"'",
    )
    .await
    .unwrap_or_else(|_| "Unknown".to_string());
    println!("  OS:           {}", style(os_info.trim()).dim());

    println!("\n{}", style("Note").bold().yellow());
    println!(
        "  {} Docker containers don't run the spuff-agent.",
        style("i").blue()
    );
    println!(
        "  {} Use {} for detailed metrics.",
        style("→").dim(),
        style("spuff metrics").cyan()
    );

    Ok(())
}

/// Show Docker container metrics.
async fn docker_metrics(container_id: &str) -> Result<()> {
    println!("{}", style("Container Metrics").bold().cyan());

    // Get hostname
    let hostname = crate::connector::docker::run_command(container_id, "hostname")
        .await
        .unwrap_or_else(|_| "unknown".to_string());
    println!("  Hostname:     {}", style(hostname.trim()).white());

    // Get OS info
    let os_info = crate::connector::docker::run_command(
        container_id,
        "cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"'",
    )
    .await
    .unwrap_or_else(|_| "Unknown".to_string());
    println!("  OS:           {}", style(os_info.trim()).dim());

    // Get CPU count
    let cpus = crate::connector::docker::run_command(container_id, "nproc 2>/dev/null || echo 1")
        .await
        .unwrap_or_else(|_| "1".to_string());
    println!("  CPUs:         {}", style(cpus.trim()).white());

    // Get memory info
    let mem_info = crate::connector::docker::run_command(
        container_id,
        "free -b 2>/dev/null | awk '/^Mem:/ {print $2, $3}'",
    )
    .await
    .unwrap_or_else(|_| "0 0".to_string());
    let mem_parts: Vec<u64> = mem_info
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if mem_parts.len() >= 2 {
        let total = mem_parts[0];
        let used = mem_parts[1];
        let percent = if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        println!(
            "  Memory:       {} / {} ({})",
            style(format_bytes(used)).white(),
            style(format_bytes(total)).dim(),
            format_percent_colored(percent)
        );
    }

    // Get disk info
    let disk_info = crate::connector::docker::run_command(
        container_id,
        "df -B1 / 2>/dev/null | awk 'NR==2 {print $2, $3}'",
    )
    .await
    .unwrap_or_else(|_| "0 0".to_string());
    let disk_parts: Vec<u64> = disk_info
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if disk_parts.len() >= 2 {
        let total = disk_parts[0];
        let used = disk_parts[1];
        let percent = if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        println!(
            "  Disk:         {} / {} ({})",
            style(format_bytes(used)).white(),
            style(format_bytes(total)).dim(),
            format_percent_colored(percent)
        );
    }

    // Get load average
    let load = crate::connector::docker::run_command(
        container_id,
        "cat /proc/loadavg 2>/dev/null | awk '{print $1, $2, $3}'",
    )
    .await
    .unwrap_or_else(|_| "0 0 0".to_string());
    let load_parts: Vec<&str> = load.split_whitespace().collect();
    if load_parts.len() >= 3 {
        println!(
            "  Load Avg:     {} {} {}",
            load_parts[0], load_parts[1], load_parts[2]
        );
    }

    Ok(())
}

/// Show Docker container processes.
async fn docker_processes(container_id: &str) -> Result<()> {
    println!("{}", style("Top Processes by CPU").bold().cyan());
    println!(
        "  {:<8} {:<30} {:>10} {:>12}",
        "PID", "NAME", "CPU %", "MEMORY"
    );
    println!("  {}", "-".repeat(64));

    // Get process list using ps
    let ps_output = crate::connector::docker::run_command(
        container_id,
        "ps aux --sort=-%cpu 2>/dev/null | head -11 | tail -10",
    )
    .await?;

    for line in ps_output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 11 {
            let pid = parts[1];
            let cpu = parts[2];
            let mem = parts[3];
            let cmd = parts[10..].join(" ");
            let name = truncate(&cmd, 30);
            println!("  {:<8} {:<30} {:>9}% {:>11}%", pid, name, cpu, mem);
        }
    }

    Ok(())
}

/// Show Docker container logs.
async fn docker_logs(container_id: &str, lines: usize, file: Option<String>) -> Result<()> {
    let file_path = file.unwrap_or_else(|| "/var/log/syslog".to_string());

    // Validate path to prevent traversal
    if file_path.contains("..") {
        return Err(SpuffError::Config("Invalid log file path".to_string()));
    }

    let output = crate::connector::docker::run_command(
        container_id,
        &format!(
            "tail -n {} {} 2>/dev/null || echo 'Log file not found: {}'",
            lines, file_path, file_path
        ),
    )
    .await?;

    for line in output.lines() {
        println!("{}", line);
    }

    Ok(())
}
