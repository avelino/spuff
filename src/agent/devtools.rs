//! Devtools installation manager for spuff-agent.
//!
//! Manages async installation of development tools with per-tool status tracking.
//! Each tool can be installed independently and reports its own status.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Status of a single devtool installation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// Not yet started
    #[default]
    Pending,
    /// Currently installing
    Installing,
    /// Successfully installed
    Done,
    /// Installation failed
    Failed(String),
    /// Skipped (not configured or not applicable)
    Skipped,
}

/// A single devtool definition
#[derive(Debug, Clone, Serialize)]
pub struct DevTool {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub status: ToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Configuration for devtools installation
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DevToolsConfig {
    /// Install Docker
    #[serde(default = "default_true")]
    pub docker: bool,

    /// Install shell tools (fzf, bat, eza, zoxide, starship)
    #[serde(default = "default_true")]
    pub shell_tools: bool,

    /// Install Node.js
    #[serde(default = "default_true")]
    pub nodejs: bool,

    /// Install Claude Code CLI
    #[serde(default = "default_true")]
    pub claude_code: bool,

    /// Dev environment: "devbox", "nix", or empty
    #[serde(default)]
    pub environment: Option<String>,

    /// Dotfiles repository URL
    #[serde(default)]
    pub dotfiles: Option<String>,

    /// Install Tailscale
    #[serde(default)]
    pub tailscale: bool,

    /// Tailscale auth key
    #[serde(default)]
    pub tailscale_authkey: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Overall devtools installation state
#[derive(Debug, Clone, Serialize)]
pub struct DevToolsState {
    /// Whether installation has started
    pub started: bool,
    /// Whether all installations are complete
    pub completed: bool,
    /// Individual tool statuses
    pub tools: Vec<DevTool>,
    /// When installation started
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When installation completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for DevToolsState {
    fn default() -> Self {
        Self {
            started: false,
            completed: false,
            tools: vec![
                DevTool {
                    id: "docker",
                    name: "Docker",
                    description: "Container runtime",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "fzf",
                    name: "fzf",
                    description: "Fuzzy finder",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "bat",
                    name: "bat",
                    description: "Cat with syntax highlighting",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "eza",
                    name: "eza",
                    description: "Modern ls replacement",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "zoxide",
                    name: "zoxide",
                    description: "Smarter cd command",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "starship",
                    name: "Starship",
                    description: "Cross-shell prompt",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "nodejs",
                    name: "Node.js",
                    description: "JavaScript runtime",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "claude_code",
                    name: "Claude Code",
                    description: "AI coding assistant",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "devenv",
                    name: "Dev Environment",
                    description: "Devbox or Nix",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "dotfiles",
                    name: "Dotfiles",
                    description: "User configuration",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "tailscale",
                    name: "Tailscale",
                    description: "Private networking",
                    status: ToolStatus::Pending,
                    version: None,
                },
            ],
            started_at: None,
            completed_at: None,
        }
    }
}

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
            let installer = DevToolsInstaller {
                state,
                username,
                config,
            };
            installer.run().await;
        });

        Ok(())
    }
}

/// Internal installer that runs the actual installations
struct DevToolsInstaller {
    state: Arc<RwLock<DevToolsState>>,
    username: String,
    config: DevToolsConfig,
}

impl DevToolsInstaller {
    async fn run(&self) {
        tracing::info!("Starting devtools installation");

        // Phase 1: Docker (background) + Shell tools (parallel)
        let docker_handle = if self.config.docker {
            Some(self.install_docker())
        } else {
            None
        };

        if self.config.shell_tools {
            // Install shell tools in parallel
            let handles = vec![
                self.install_fzf(),
                self.install_bat(),
                self.install_eza(),
                self.install_zoxide(),
                self.install_starship(),
            ];

            for handle in handles {
                let _ = handle.await;
            }
        }

        // Wait for Docker
        if let Some(handle) = docker_handle {
            let _ = handle.await;
        }

        // Phase 2: Node.js (needed for Claude Code)
        if self.config.nodejs {
            self.install_nodejs().await;
        }

        // Phase 3: Claude Code (needs Node.js)
        if self.config.claude_code {
            self.install_claude_code().await;
        }

        // Phase 4: Dev environment
        if let Some(ref env) = self.config.environment {
            match env.as_str() {
                "devbox" => self.install_devbox().await,
                "nix" => self.install_nix().await,
                _ => {
                    self.update_status("devenv", ToolStatus::Skipped, None)
                        .await;
                }
            }
        }

        // Phase 5: Dotfiles
        if let Some(ref dotfiles_url) = self.config.dotfiles {
            self.install_dotfiles(dotfiles_url).await;
        }

        // Phase 6: Tailscale
        if self.config.tailscale {
            self.install_tailscale().await;
        }

        // Mark as completed
        {
            let mut state = self.state.write().await;
            state.completed = true;
            state.completed_at = Some(chrono::Utc::now());
        }

        tracing::info!("Devtools installation completed");
    }

    async fn update_status(&self, tool_id: &str, status: ToolStatus, version: Option<String>) {
        let mut state = self.state.write().await;
        if let Some(tool) = state.tools.iter_mut().find(|t| t.id == tool_id) {
            tool.status = status;
            if version.is_some() {
                tool.version = version;
            }
        }
    }

    async fn run_command(&self, cmd: &str) -> Result<String, String> {
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

    fn install_docker(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let username = self.username.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username,
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("docker", ToolStatus::Installing, None)
                .await;

            match installer
                .run_command("curl -fsSL https://get.docker.com | sh")
                .await
            {
                Ok(_) => {
                    // Add user to docker group
                    let _ = installer
                        .run_command(&format!("usermod -aG docker {}", installer.username))
                        .await;

                    // Get version
                    let version = installer
                        .run_command("docker --version 2>/dev/null | awk '{print $3}' | tr -d ','")
                        .await
                        .ok();

                    installer
                        .update_status("docker", ToolStatus::Done, version)
                        .await;
                    tracing::info!("Docker installed");
                }
                Err(e) => {
                    installer
                        .update_status("docker", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("Docker installation failed: {}", e);
                }
            }
        })
    }

    fn install_fzf(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let username = self.username.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username: username.clone(),
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("fzf", ToolStatus::Installing, None)
                .await;

            let home = format!("/home/{}", username);
            let cmd = format!(
                "git clone --depth 1 https://github.com/junegunn/fzf.git {}/.fzf && \
                 chown -R {}:{} {}/.fzf && \
                 su - {} -c '{}/.fzf/install --all --no-update-rc'",
                home, username, username, home, username, home
            );

            match installer.run_command(&cmd).await {
                Ok(_) => {
                    installer.update_status("fzf", ToolStatus::Done, None).await;
                    tracing::info!("fzf installed");
                }
                Err(e) => {
                    installer
                        .update_status("fzf", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("fzf installation failed: {}", e);
                }
            }
        })
    }

    fn install_bat(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username: String::new(),
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("bat", ToolStatus::Installing, None)
                .await;

            let cmd = r#"
                BAT_VERSION="0.24.0"
                ARCH=$(dpkg --print-architecture)
                curl -fsSL "https://github.com/sharkdp/bat/releases/download/v${BAT_VERSION}/bat_${BAT_VERSION}_${ARCH}.deb" -o /tmp/bat.deb
                dpkg -i /tmp/bat.deb || true
                rm -f /tmp/bat.deb
            "#;

            match installer.run_command(cmd).await {
                Ok(_) => {
                    let version = installer
                        .run_command("bat --version 2>/dev/null | awk '{print $2}'")
                        .await
                        .ok();
                    installer
                        .update_status("bat", ToolStatus::Done, version)
                        .await;
                    tracing::info!("bat installed");
                }
                Err(e) => {
                    installer
                        .update_status("bat", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("bat installation failed: {}", e);
                }
            }
        })
    }

    fn install_eza(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username: String::new(),
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("eza", ToolStatus::Installing, None)
                .await;

            let cmd = r#"
                EZA_VERSION="0.20.13"
                ARCH=$(uname -m)
                case $ARCH in
                    x86_64) EZA_ARCH="x86_64" ;;
                    aarch64) EZA_ARCH="aarch64" ;;
                    *) exit 1 ;;
                esac
                curl -fsSL "https://github.com/eza-community/eza/releases/download/v${EZA_VERSION}/eza_${EZA_ARCH}-unknown-linux-gnu.zip" -o /tmp/eza.zip
                unzip -o /tmp/eza.zip -d /tmp/eza
                mv /tmp/eza/eza /usr/local/bin/
                chmod +x /usr/local/bin/eza
                rm -rf /tmp/eza /tmp/eza.zip
            "#;

            match installer.run_command(cmd).await {
                Ok(_) => {
                    let version = installer
                        .run_command("eza --version 2>/dev/null | head -1 | awk '{print $2}'")
                        .await
                        .ok();
                    installer
                        .update_status("eza", ToolStatus::Done, version)
                        .await;
                    tracing::info!("eza installed");
                }
                Err(e) => {
                    installer
                        .update_status("eza", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("eza installation failed: {}", e);
                }
            }
        })
    }

    fn install_zoxide(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username: String::new(),
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("zoxide", ToolStatus::Installing, None)
                .await;

            let cmd = r#"
                curl -sS https://raw.githubusercontent.com/ajeetdsouza/zoxide/main/install.sh | bash
                mv ~/.local/bin/zoxide /usr/local/bin/ 2>/dev/null || true
            "#;

            match installer.run_command(cmd).await {
                Ok(_) => {
                    let version = installer
                        .run_command("zoxide --version 2>/dev/null | awk '{print $2}'")
                        .await
                        .ok();
                    installer
                        .update_status("zoxide", ToolStatus::Done, version)
                        .await;
                    tracing::info!("zoxide installed");
                }
                Err(e) => {
                    installer
                        .update_status("zoxide", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("zoxide installation failed: {}", e);
                }
            }
        })
    }

    fn install_starship(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();

        tokio::spawn(async move {
            let installer = Self {
                state,
                username: String::new(),
                config: DevToolsConfig::default(),
            };

            installer
                .update_status("starship", ToolStatus::Installing, None)
                .await;

            match installer
                .run_command("curl -sS https://starship.rs/install.sh | sh -s -- -y")
                .await
            {
                Ok(_) => {
                    let version = installer
                        .run_command("starship --version 2>/dev/null | head -1 | awk '{print $2}'")
                        .await
                        .ok();
                    installer
                        .update_status("starship", ToolStatus::Done, version)
                        .await;
                    tracing::info!("starship installed");
                }
                Err(e) => {
                    installer
                        .update_status("starship", ToolStatus::Failed(e.clone()), None)
                        .await;
                    tracing::error!("starship installation failed: {}", e);
                }
            }
        })
    }

    async fn install_nodejs(&self) {
        self.update_status("nodejs", ToolStatus::Installing, None)
            .await;

        let cmd = "curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs";

        match self.run_command(cmd).await {
            Ok(_) => {
                let version = self
                    .run_command("node --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.update_status("nodejs", ToolStatus::Done, version)
                    .await;
                tracing::info!("Node.js installed");
            }
            Err(e) => {
                self.update_status("nodejs", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Node.js installation failed: {}", e);
            }
        }
    }

    async fn install_claude_code(&self) {
        self.update_status("claude_code", ToolStatus::Installing, None)
            .await;

        match self
            .run_command("npm install -g @anthropic-ai/claude-code")
            .await
        {
            Ok(_) => {
                let version = self
                    .run_command("claude --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.update_status("claude_code", ToolStatus::Done, version)
                    .await;
                tracing::info!("Claude Code installed");
            }
            Err(e) => {
                self.update_status("claude_code", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Claude Code installation failed: {}", e);
            }
        }
    }

    async fn install_devbox(&self) {
        self.update_status("devenv", ToolStatus::Installing, None)
            .await;

        let cmd = format!(
            "curl -fsSL https://get.jetify.com/devbox | bash -s -- -f && \
             su - {} -c 'devbox version' || true",
            self.username
        );

        match self.run_command(&cmd).await {
            Ok(_) => {
                let version = self
                    .run_command(&format!(
                        "su - {} -c 'devbox version 2>/dev/null'",
                        self.username
                    ))
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.update_status("devenv", ToolStatus::Done, version)
                    .await;
                tracing::info!("Devbox installed");
            }
            Err(e) => {
                self.update_status("devenv", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Devbox installation failed: {}", e);
            }
        }
    }

    async fn install_nix(&self) {
        self.update_status("devenv", ToolStatus::Installing, None)
            .await;

        let cmd = format!(
            "curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install --no-confirm && \
             echo '. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh' >> /home/{}/.bashrc",
            self.username
        );

        match self.run_command(&cmd).await {
            Ok(_) => {
                self.update_status("devenv", ToolStatus::Done, Some("nix".to_string()))
                    .await;
                tracing::info!("Nix installed");
            }
            Err(e) => {
                self.update_status("devenv", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Nix installation failed: {}", e);
            }
        }
    }

    async fn install_dotfiles(&self, url: &str) {
        self.update_status("dotfiles", ToolStatus::Installing, None)
            .await;

        let cmd = format!(
            "su - {} -c 'git clone {} ~/.dotfiles' && \
             if [ -f /home/{}/.dotfiles/install.sh ]; then \
                 su - {} -c 'cd ~/.dotfiles && ./install.sh'; \
             elif [ -f /home/{}/.dotfiles/setup.sh ]; then \
                 su - {} -c 'cd ~/.dotfiles && ./setup.sh'; \
             fi",
            self.username, url, self.username, self.username, self.username, self.username
        );

        match self.run_command(&cmd).await {
            Ok(_) => {
                self.update_status("dotfiles", ToolStatus::Done, None).await;
                tracing::info!("Dotfiles installed");
            }
            Err(e) => {
                self.update_status("dotfiles", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Dotfiles installation failed: {}", e);
            }
        }
    }

    async fn install_tailscale(&self) {
        self.update_status("tailscale", ToolStatus::Installing, None)
            .await;

        let mut cmd = "curl -fsSL https://tailscale.com/install.sh | sh".to_string();

        if let Some(ref authkey) = self.config.tailscale_authkey {
            cmd.push_str(&format!(" && tailscale up --authkey='{}'", authkey));
        }

        match self.run_command(&cmd).await {
            Ok(_) => {
                let version = self
                    .run_command("tailscale --version 2>/dev/null | head -1")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.update_status("tailscale", ToolStatus::Done, version)
                    .await;
                tracing::info!("Tailscale installed");
            }
            Err(e) => {
                self.update_status("tailscale", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Tailscale installation failed: {}", e);
            }
        }
    }
}
