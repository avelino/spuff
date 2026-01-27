//! Formatting helpers for status display
//!
//! Functions for formatting status values with colors.

use console::style;

use crate::project_config::SetupStatus;

use super::types::{StepState, BOOTSTRAP_STEPS};

pub fn format_status(status: &str) -> console::StyledObject<&str> {
    match status.to_lowercase().as_str() {
        "active" | "running" => style(status).green(),
        "new" | "starting" => style(status).yellow(),
        _ => style(status).red(),
    }
}

pub fn format_bootstrap_status(status: &str) -> console::StyledObject<String> {
    let display = match status {
        "ready" => "ready".to_string(),
        "pending" => "pending".to_string(),
        "starting" => "starting...".to_string(),
        s if s.starts_with("installing:") => {
            let phase = s.strip_prefix("installing:").unwrap_or(s);
            format!("installing ({})", phase)
        }
        "failed" => "failed".to_string(),
        "" | "unknown" => "unknown".to_string(),
        other => other.to_string(),
    };

    match status {
        "ready" => style(display).green(),
        "failed" => style(display).red(),
        "" | "unknown" => style(display).dim(),
        _ => style(display).yellow(),
    }
}

/// Get the state of a step based on current status
pub fn get_step_state(step_key: &str, current_status: &str) -> StepState {
    // Find the index of the current status in the steps
    let current_idx = BOOTSTRAP_STEPS
        .iter()
        .position(|(key, _)| *key == current_status || current_status.starts_with(key));

    let step_idx = BOOTSTRAP_STEPS.iter().position(|(key, _)| *key == step_key);

    match (current_idx, step_idx) {
        (Some(curr), Some(step)) => {
            if step < curr {
                StepState::Completed
            } else if step == curr {
                StepState::InProgress
            } else {
                StepState::Pending
            }
        }
        _ => StepState::Pending,
    }
}

pub fn format_setup_status<'a>(
    status: &SetupStatus,
    name: &'a str,
) -> (
    console::StyledObject<&'static str>,
    console::StyledObject<&'a str>,
) {
    match status {
        SetupStatus::Done => (style("[✓]").green(), style(name).green()),
        SetupStatus::InProgress => (style("[>]").yellow().bold(), style(name).yellow().bold()),
        SetupStatus::Pending => (style("[ ]").dim(), style(name).dim()),
        SetupStatus::Failed(_) => (style("[!]").red(), style(name).red()),
        SetupStatus::Skipped => (style("[-]").dim(), style(name).dim()),
    }
}
