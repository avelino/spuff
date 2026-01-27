//! Cloud-init bootstrap progress tracking
//!
//! Functions for monitoring cloud-init progress during VM provisioning.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::error::Result;
use crate::tui::{ProgressMessage, StepState};

use super::{STEP_BOOTSTRAP, SUB_AGENT, SUB_PACKAGES};

pub async fn wait_for_cloud_init_with_progress(
    ip: &str,
    config: &AppConfig,
    tx: &mpsc::Sender<ProgressMessage>,
) -> Result<()> {
    let max_attempts = 60; // 5 minutes max (faster now with minimal bootstrap)
    let delay = Duration::from_secs(5);

    // Track which sub-steps have been completed
    let mut packages_done = false;
    let mut agent_done = false;

    // Start first sub-step
    tx.send(ProgressMessage::SetSubStep(
        STEP_BOOTSTRAP,
        SUB_PACKAGES,
        StepState::InProgress,
    ))
    .await
    .ok();
    tx.send(ProgressMessage::SetDetail(
        "Updating system packages...".to_string(),
    ))
    .await
    .ok();

    for _ in 0..max_attempts {
        // Check cloud-init log for progress
        if let Ok(log) = get_cloud_init_log(ip, config).await {
            // Check for package updates completion
            if !packages_done
                && (log.contains("apt-get update") || log.contains("package_update"))
                && (log.contains("Setting up") || log.contains("Unpacking"))
            {
                packages_done = true;
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_PACKAGES,
                    StepState::Done,
                ))
                .await
                .ok();
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_AGENT,
                    StepState::InProgress,
                ))
                .await
                .ok();
                tx.send(ProgressMessage::SetDetail(
                    "Installing spuff-agent...".to_string(),
                ))
                .await
                .ok();
            }

            // Check for spuff-agent installation
            if !agent_done
                && log.contains("spuff-agent")
                && (log.contains("systemctl enable spuff-agent")
                    || log.contains("systemctl start spuff-agent"))
            {
                agent_done = true;
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_AGENT,
                    StepState::Done,
                ))
                .await
                .ok();
                tx.send(ProgressMessage::SetDetail("Finalizing...".to_string()))
                    .await
                    .ok();
            }

            // Check if cloud-init is done
            if log.contains("spuff environment ready") || log.contains("final_message") {
                // Mark any remaining sub-steps as done
                for i in 0..2 {
                    tx.send(ProgressMessage::SetSubStep(
                        STEP_BOOTSTRAP,
                        i,
                        StepState::Done,
                    ))
                    .await
                    .ok();
                }
                return Ok(());
            }
        }

        // Also check cloud-init status
        if let Ok(true) = check_cloud_init_status(ip, config).await {
            // Mark any remaining sub-steps as done
            for i in 0..2 {
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    i,
                    StepState::Done,
                ))
                .await
                .ok();
            }
            return Ok(());
        }

        tokio::time::sleep(delay).await;
    }

    Err(crate::error::SpuffError::CloudInit(
        "Timeout waiting for cloud-init".to_string(),
    ))
}

async fn get_cloud_init_log(ip: &str, config: &AppConfig) -> Result<String> {
    crate::connector::ssh::run_command(
        ip,
        config,
        "tail -200 /var/log/cloud-init-output.log 2>/dev/null || echo ''",
    )
    .await
}

async fn check_cloud_init_status(ip: &str, config: &AppConfig) -> Result<bool> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "cloud-init status --format=json 2>/dev/null || echo '{\"status\": \"running\"}'",
    )
    .await?;

    Ok(output.contains("\"status\": \"done\"") || output.contains("\"status\":\"done\""))
}
