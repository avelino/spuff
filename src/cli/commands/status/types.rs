//! Types for status responses
//!
//! Response types from the agent status endpoints.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DevToolsState {
    pub started: bool,
    pub completed: bool,
    pub tools: Vec<DevTool>,
}

#[derive(Debug, Deserialize)]
pub struct DevTool {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub status: DevToolStatus,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DevToolStatus {
    Pending,
    Installing,
    Done,
    Failed(String),
    Skipped,
}

#[derive(Debug, Deserialize)]
pub struct BootstrapStatus {
    #[serde(default)]
    pub bootstrap_status: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub bootstrap_ready: bool,
}

/// Bootstrap steps in order of execution (minimal bootstrap)
/// Devtools are now installed via agent after bootstrap completes
pub const BOOTSTRAP_STEPS: &[(&str, &str)] = &[
    ("starting", "Starting bootstrap"),
    ("installing:agent", "Installing spuff-agent"),
    ("ready", "Bootstrap complete"),
];

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum StepState {
    Pending,
    InProgress,
    Completed,
}
