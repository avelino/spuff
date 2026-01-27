//! Formatting helpers for agent output
//!
//! Functions for formatting status, metrics, and activity entries.

use console::style;

use super::types::ActivityLogEntry;

pub fn format_bootstrap_status(status: &str) -> console::StyledObject<&str> {
    match status {
        "ready" => style(status).green(),
        "pending" | "starting" => style(status).yellow(),
        s if s.starts_with("installing:") => style(status).yellow(),
        "failed" => style(status).red(),
        "unknown" | "" => style("unknown").dim(),
        _ => style(status).white(),
    }
}

pub fn format_cpu_bar(usage: f32) -> String {
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

pub fn format_percent_colored(percent: f32) -> console::StyledObject<String> {
    let text = format!("{:.1}%", percent);
    if percent >= 90.0 {
        style(text).red()
    } else if percent >= 70.0 {
        style(text).yellow()
    } else {
        style(text).green()
    }
}

pub fn print_activity_entry(entry: &ActivityLogEntry) {
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

/// Unescape output from log (convert \\n back to actual newlines for display)
pub fn unescape_output(s: &str) -> String {
    // Truncate long output for display
    let truncated = if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s.to_string()
    };

    // Keep it single-line but show literal \n for readability
    truncated
}

