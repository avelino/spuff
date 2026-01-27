//! HTTP requests to agent
//!
//! Functions for fetching status from the agent API.

use crate::config::AppConfig;
use crate::error::Result;
use crate::project_config::ProjectSetupState;

use super::types::{BootstrapStatus, DevToolsState};

pub async fn get_bootstrap_status(ip: &str, config: &AppConfig) -> Result<BootstrapStatus> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/status 2>/dev/null || echo '{}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse agent response: {}", e))
    })
}

pub async fn get_devtools_status(ip: &str, config: &AppConfig) -> Result<DevToolsState> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/devtools 2>/dev/null || echo '{\"started\":false,\"completed\":false,\"tools\":[]}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse devtools response: {}", e))
    })
}

pub async fn get_project_status(ip: &str, config: &AppConfig) -> Result<ProjectSetupState> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "curl -s http://127.0.0.1:7575/project/status 2>/dev/null || echo '{}'",
    )
    .await?;

    serde_json::from_str(&output).map_err(|e| {
        crate::error::SpuffError::Provider(format!("Failed to parse project status: {}", e))
    })
}
