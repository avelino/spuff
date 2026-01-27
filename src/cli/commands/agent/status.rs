//! Status and metrics commands
//!
//! Commands for viewing agent status, metrics, and processes.

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;
use crate::utils::{format_bytes, format_duration, truncate};

use super::docker;
use super::format::{format_bootstrap_status, format_cpu_bar, format_percent_colored};
use super::http::agent_request;
use super::types::{ActivityLogResponse, AgentMetrics, AgentStatus, ProcessInfo};

pub async fn status(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let is_docker = instance.provider == "docker" || instance.provider == "local";

    if is_docker {
        return docker::docker_status(&instance.id).await;
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
        return docker::docker_metrics(&instance.id).await;
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
        return docker::docker_processes(&instance.id).await;
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
