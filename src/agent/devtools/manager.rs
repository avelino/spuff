//! DevToolsManager - Coordinates devtools installation.

use std::sync::Arc;
use tokio::sync::RwLock;

use super::installer::DevToolsInstaller;
use super::types::{DevToolsConfig, DevToolsState, ToolStatus};

/// Devtools installation manager
pub struct DevToolsManager {
    state: Arc<RwLock<DevToolsState>>,
    username: String,
}

impl DevToolsManager {
    pub fn new(username: String) -> Self {
        Self {
            state: Arc::new(RwLock::new(DevToolsState::default())),
            username,
        }
    }

    /// Get current devtools state
    pub async fn get_state(&self) -> DevToolsState {
        self.state.read().await.clone()
    }

    /// Check if installation is already running
    pub async fn is_running(&self) -> bool {
        let state = self.state.read().await;
        state.started && !state.completed
    }

    /// Update status of a specific tool
    async fn update_tool_status(&self, tool_id: &str, status: ToolStatus, version: Option<String>) {
        let mut state = self.state.write().await;
        if let Some(tool) = state.tools.iter_mut().find(|t| t.id == tool_id) {
            tool.status = status;
            if version.is_some() {
                tool.version = version;
            }
        }
    }

    /// Start devtools installation with given config
    pub async fn install(&self, config: DevToolsConfig) -> Result<(), String> {
        // Check if already running
        {
            let state = self.state.read().await;
            if state.started && !state.completed {
                return Err("Installation already in progress".to_string());
            }
        }

        // Mark as started
        {
            let mut state = self.state.write().await;
            state.started = true;
            state.completed = false;
            state.started_at = Some(chrono::Utc::now());
            state.completed_at = None;

            // Reset all tool statuses
            for tool in &mut state.tools {
                tool.status = ToolStatus::Pending;
                tool.version = None;
            }
        }

        // Skip tools that aren't configured
        if !config.docker {
            self.update_tool_status("docker", ToolStatus::Skipped, None)
                .await;
        }
        if !config.shell_tools {
            for id in &["fzf", "bat", "eza", "zoxide", "starship"] {
                self.update_tool_status(id, ToolStatus::Skipped, None).await;
            }
        }
        if !config.nodejs {
            self.update_tool_status("nodejs", ToolStatus::Skipped, None)
                .await;
        }
        if !config.claude_code {
            self.update_tool_status("claude_code", ToolStatus::Skipped, None)
                .await;
        }
        if !config.codex {
            self.update_tool_status("codex", ToolStatus::Skipped, None)
                .await;
        }
        if !config.opencode {
            self.update_tool_status("opencode", ToolStatus::Skipped, None)
                .await;
        }
        if !config.copilot {
            self.update_tool_status("copilot", ToolStatus::Skipped, None)
                .await;
        }
        if !config.cursor {
            self.update_tool_status("cursor", ToolStatus::Skipped, None)
                .await;
        }
        if !config.cody {
            self.update_tool_status("cody", ToolStatus::Skipped, None)
                .await;
        }
        if !config.aider {
            self.update_tool_status("aider", ToolStatus::Skipped, None)
                .await;
        }
        if !config.gemini {
            self.update_tool_status("gemini", ToolStatus::Skipped, None)
                .await;
        }
        if config.environment.is_none() {
            self.update_tool_status("devenv", ToolStatus::Skipped, None)
                .await;
        }
        if config.dotfiles.is_none() {
            self.update_tool_status("dotfiles", ToolStatus::Skipped, None)
                .await;
        }
        if !config.tailscale {
            self.update_tool_status("tailscale", ToolStatus::Skipped, None)
                .await;
        }

        let state = self.state.clone();
        let username = self.username.clone();

        // Spawn installation task
        tokio::spawn(async move {
            let installer = DevToolsInstaller::new(state, username, config);
            installer.run().await;
        });

        Ok(())
    }
}
