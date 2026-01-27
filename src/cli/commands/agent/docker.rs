//! Docker-specific agent implementations
//!
//! Commands for interacting with Docker containers instead of cloud VMs.

use console::style;

use crate::error::{Result, SpuffError};
use crate::utils::{format_bytes, format_duration, truncate};

use super::format::format_percent_colored;

/// Show Docker container status.
pub async fn docker_status(container_id: &str) -> Result<()> {
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
pub async fn docker_metrics(container_id: &str) -> Result<()> {
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
pub async fn docker_processes(container_id: &str) -> Result<()> {
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
pub async fn docker_logs(container_id: &str, lines: usize, file: Option<String>) -> Result<()> {
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
