//! DevToolsInstaller - Orchestrates the installation of all dev tools.

use std::sync::Arc;
use tokio::sync::RwLock;

use super::ai_tools::AiToolsInstaller;
use super::environment::EnvironmentInstaller;
use super::system::SystemInstaller;
use super::types::{DevToolsConfig, DevToolsState, ToolStatus};

/// Internal installer that runs the actual installations
pub struct DevToolsInstaller {
    pub(crate) state: Arc<RwLock<DevToolsState>>,
    pub(crate) username: String,
    pub(crate) config: DevToolsConfig,
}

impl DevToolsInstaller {
    pub fn new(state: Arc<RwLock<DevToolsState>>, username: String, config: DevToolsConfig) -> Self {
        Self {
            state,
            username,
            config,
        }
    }

    pub async fn run(&self) {
        tracing::info!("Starting devtools installation");

        // Phase 1: Docker (background) + Shell tools (parallel)
        let system = SystemInstaller::new(self);
        let docker_handle = if self.config.docker {
            Some(system.install_docker())
        } else {
            None
        };

        if self.config.shell_tools {
            system.install_shell_tools().await;
        }

        // Wait for Docker
        if let Some(handle) = docker_handle {
            let _ = handle.await;
        }

        // Phase 2: Node.js (needed for AI tools)
        if self.config.nodejs {
            system.install_nodejs().await;
        }

        // Phase 3: AI Coding Tools (all need Node.js)
        let ai = AiToolsInstaller::new(self);
        if self.config.claude_code {
            ai.install_claude_code().await;
        }
        if self.config.codex {
            ai.install_codex().await;
        }
        if self.config.opencode {
            ai.install_opencode().await;
        }

        // Phase 4: Dev environment
        let env = EnvironmentInstaller::new(self);
        if let Some(ref environment) = self.config.environment {
            match environment.as_str() {
                "devbox" => env.install_devbox().await,
                "nix" => env.install_nix().await,
                _ => {
                    self.update_status("devenv", ToolStatus::Skipped, None)
                        .await;
                }
            }
        }

        // Phase 5: Dotfiles
        if let Some(ref dotfiles_url) = self.config.dotfiles {
            env.install_dotfiles(dotfiles_url).await;
        }

        // Phase 6: Tailscale
        if self.config.tailscale {
            env.install_tailscale().await;
        }

        // Mark as completed
        {
            let mut state = self.state.write().await;
            state.completed = true;
            state.completed_at = Some(chrono::Utc::now());
        }

        tracing::info!("Devtools installation completed");
    }

    pub async fn update_status(&self, tool_id: &str, status: ToolStatus, version: Option<String>) {
        let mut state = self.state.write().await;
        if let Some(tool) = state.tools.iter_mut().find(|t| t.id == tool_id) {
            tool.status = status;
            if version.is_some() {
                tool.version = version;
            }
        }
    }

    pub async fn run_command(&self, cmd: &str) -> Result<String, String> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}
