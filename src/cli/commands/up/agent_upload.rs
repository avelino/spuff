//! Agent upload and devtools installation
//!
//! Functions for uploading local agent binaries and triggering devtools installation.

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};

/// Upload local spuff-agent binary and ensure the service is running
pub async fn upload_local_agent(ip: &str, config: &AppConfig, agent_path: &str) -> Result<()> {
    // Ensure /opt/spuff exists
    crate::connector::ssh::run_command(ip, config, "sudo mkdir -p /opt/spuff").await?;

    // Upload to a temp location first
    crate::connector::ssh::scp_upload(ip, config, agent_path, "/tmp/spuff-agent").await?;

    // Move to final location and set permissions
    crate::connector::ssh::run_command(
        ip,
        config,
        "sudo mv /tmp/spuff-agent /opt/spuff/spuff-agent && sudo chmod +x /opt/spuff/spuff-agent",
    )
    .await?;

    // Ensure the service is enabled and start/restart it
    // The service file should exist from cloud-init write_files
    crate::connector::ssh::run_command(
        ip,
        config,
        "sudo systemctl daemon-reload && sudo systemctl enable spuff-agent 2>/dev/null; sudo systemctl restart spuff-agent 2>/dev/null || sudo systemctl start spuff-agent 2>/dev/null || true",
    )
    .await?;

    Ok(())
}

/// Trigger devtools installation via the agent
///
/// Reads the devtools config from /opt/spuff/devtools.json and sends it to the agent's
/// /devtools/install endpoint. The agent will install tools asynchronously in the background.
pub async fn trigger_devtools_installation(ip: &str, config: &AppConfig) -> Result<()> {
    // Read devtools config from the VM
    let devtools_json = crate::connector::ssh::run_command(
        ip,
        config,
        "cat /opt/spuff/devtools.json 2>/dev/null || echo '{}'",
    )
    .await?;

    // Parse to validate it's valid JSON
    let devtools_config: serde_json::Value = serde_json::from_str(&devtools_json)
        .map_err(|e| SpuffError::Provider(format!("Invalid devtools.json: {}", e)))?;

    // Skip if config is empty
    if devtools_config.as_object().is_none_or(|o| o.is_empty()) {
        tracing::debug!("No devtools config found, skipping installation trigger");
        return Ok(());
    }

    // Call the agent's devtools install endpoint via SSH tunnel
    let curl_cmd = format!(
        r#"curl -s -X POST http://127.0.0.1:7575/devtools/install \
           -H "Content-Type: application/json" \
           -d '{}'"#,
        devtools_json.trim()
    );

    let result = crate::connector::ssh::run_command(ip, config, &curl_cmd).await?;

    // Check for success
    if result.contains("\"started\":true") || result.contains("already in progress") {
        tracing::info!("Devtools installation triggered successfully");
        Ok(())
    } else {
        tracing::warn!("Devtools install response: {}", result);
        // Don't fail even if the response is unexpected
        Ok(())
    }
}
