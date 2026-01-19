//! Project setup manager for spuff-agent.
//!
//! Manages async project setup from spuff.yaml configuration:
//! - Language bundle installation
//! - System package installation
//! - Repository cloning
//! - Docker services startup
//! - Setup script execution

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;

/// Project configuration (loaded from /opt/spuff/project.json)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub version: String,
    pub name: Option<String>,
    #[serde(default)]
    pub bundles: Vec<String>,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub services: ServicesConfig,
    #[serde(default)]
    pub repositories: Vec<RepositoryConfig>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ServicesConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_compose_file")]
    pub compose_file: String,
    #[serde(default)]
    pub profiles: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_compose_file() -> String {
    "docker-compose.yaml".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RepositoryConfig {
    Short(String),
    Full {
        url: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
}

impl RepositoryConfig {
    pub fn url(&self) -> String {
        match self {
            RepositoryConfig::Short(s) => {
                if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("git@") {
                    s.clone()
                } else {
                    format!("git@github.com:{}.git", s)
                }
            }
            RepositoryConfig::Full { url, .. } => url.clone(),
        }
    }

    pub fn path(&self, projects_dir: &str) -> String {
        match self {
            RepositoryConfig::Short(s) => {
                let name = s.rsplit('/').next().unwrap_or(s).trim_end_matches(".git");
                format!("{}/{}", projects_dir, name)
            }
            RepositoryConfig::Full { url, path, .. } => {
                if let Some(p) = path {
                    if p.starts_with('~') {
                        p.clone()
                    } else {
                        p.clone()
                    }
                } else {
                    let name = url.rsplit('/').next().unwrap_or(url).trim_end_matches(".git");
                    format!("{}/{}", projects_dir, name)
                }
            }
        }
    }

    pub fn branch(&self) -> Option<String> {
        match self {
            RepositoryConfig::Short(_) => None,
            RepositoryConfig::Full { branch, .. } => branch.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HooksConfig {
    pub post_up: Option<String>,
    pub pre_down: Option<String>,
}

/// Status of a setup item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed(String),
    Skipped,
}

/// Bundle installation status
#[derive(Debug, Clone, Serialize, Default)]
pub struct BundleStatus {
    pub name: String,
    pub status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Packages installation status
#[derive(Debug, Clone, Serialize, Default)]
pub struct PackagesStatus {
    pub status: SetupStatus,
    pub installed: Vec<String>,
    pub failed: Vec<String>,
}

/// Services (docker-compose) status
#[derive(Debug, Clone, Serialize, Default)]
pub struct ServicesStatus {
    pub status: SetupStatus,
    pub containers: Vec<ContainerStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerStatus {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Repository clone status
#[derive(Debug, Clone, Serialize, Default)]
pub struct RepositoryStatus {
    pub url: String,
    pub path: String,
    pub status: SetupStatus,
}

/// Script execution status
#[derive(Debug, Clone, Serialize, Default)]
pub struct ScriptStatus {
    pub command: String,
    pub status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Overall project setup state
#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectSetupState {
    pub started: bool,
    pub completed: bool,
    pub bundles: Vec<BundleStatus>,
    pub packages: PackagesStatus,
    pub services: ServicesStatus,
    pub repositories: Vec<RepositoryStatus>,
    pub scripts: Vec<ScriptStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Project setup manager
pub struct ProjectSetupManager {
    state: Arc<RwLock<ProjectSetupState>>,
    username: String,
    home_dir: String,
}

impl ProjectSetupManager {
    pub fn new(username: String) -> Self {
        let home_dir = if username == "root" {
            "/root".to_string()
        } else {
            format!("/home/{}", username)
        };

        Self {
            state: Arc::new(RwLock::new(ProjectSetupState::default())),
            username,
            home_dir,
        }
    }

    /// Get current setup state
    pub async fn get_state(&self) -> ProjectSetupState {
        self.state.read().await.clone()
    }

    /// Check if setup is already running
    pub async fn is_running(&self) -> bool {
        let state = self.state.read().await;
        state.started && !state.completed
    }

    /// Load project config from /opt/spuff/project.json
    pub fn load_config() -> Option<ProjectConfig> {
        let config_path = "/opt/spuff/project.json";
        if !Path::new(config_path).exists() {
            return None;
        }

        let content = std::fs::read_to_string(config_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Start project setup with the loaded config
    pub async fn start_setup(&self) -> Result<(), String> {
        let config = Self::load_config().ok_or("No project config found at /opt/spuff/project.json")?;

        // Check if already running
        {
            let state = self.state.read().await;
            if state.started && !state.completed {
                return Err("Setup already in progress".to_string());
            }
        }

        // Initialize state based on config
        {
            let mut state = self.state.write().await;
            state.started = true;
            state.completed = false;
            state.started_at = Some(chrono::Utc::now());
            state.completed_at = None;

            // Initialize bundles
            state.bundles = config
                .bundles
                .iter()
                .map(|name| BundleStatus {
                    name: name.clone(),
                    status: SetupStatus::Pending,
                    version: None,
                })
                .collect();

            // Initialize packages
            state.packages = PackagesStatus {
                status: if config.packages.is_empty() {
                    SetupStatus::Skipped
                } else {
                    SetupStatus::Pending
                },
                installed: vec![],
                failed: vec![],
            };

            // Initialize services
            state.services = ServicesStatus {
                status: if config.services.enabled {
                    SetupStatus::Pending
                } else {
                    SetupStatus::Skipped
                },
                containers: vec![],
            };

            // Initialize repositories
            let projects_dir = format!("{}/projects", self.home_dir);
            state.repositories = config
                .repositories
                .iter()
                .map(|repo| RepositoryStatus {
                    url: repo.url(),
                    path: repo.path(&projects_dir),
                    status: SetupStatus::Pending,
                })
                .collect();

            // Initialize scripts
            state.scripts = config
                .setup
                .iter()
                .map(|cmd| ScriptStatus {
                    command: cmd.clone(),
                    status: SetupStatus::Pending,
                    exit_code: None,
                })
                .collect();
        }

        // Spawn setup task
        let state = self.state.clone();
        let username = self.username.clone();
        let home_dir = self.home_dir.clone();

        tokio::spawn(async move {
            let installer = ProjectSetupInstaller {
                state,
                username,
                home_dir,
                config,
            };
            installer.run().await;
        });

        Ok(())
    }
}

/// Internal installer that runs the actual setup
struct ProjectSetupInstaller {
    state: Arc<RwLock<ProjectSetupState>>,
    username: String,
    home_dir: String,
    config: ProjectConfig,
}

impl ProjectSetupInstaller {
    async fn run(&self) {
        tracing::info!("Starting project setup");

        // Create log directory
        let _ = tokio::fs::create_dir_all("/var/log/spuff/bundles").await;
        let _ = tokio::fs::create_dir_all("/var/log/spuff/scripts").await;

        // Set up environment variables
        self.setup_env_vars().await;

        // Install bundles (in order, one at a time to avoid conflicts)
        for (i, bundle_name) in self.config.bundles.iter().enumerate() {
            self.install_bundle(bundle_name, i).await;
        }

        // Install packages
        self.install_packages().await;

        // Clone repositories
        self.clone_repositories().await;

        // Start services (docker-compose)
        self.start_services().await;

        // Run setup scripts
        self.run_setup_scripts().await;

        // Run post_up hook
        if let Some(ref hook) = self.config.hooks.post_up {
            self.run_hook("post_up", hook).await;
        }

        // Mark as completed
        {
            let mut state = self.state.write().await;
            state.completed = true;
            state.completed_at = Some(chrono::Utc::now());
        }

        tracing::info!("Project setup completed");
    }

    async fn setup_env_vars(&self) {
        if self.config.env.is_empty() {
            return;
        }

        // Write environment variables to a file that will be sourced by shell
        let env_file = format!("{}/.bashrc.d/spuff-project.sh", self.home_dir);
        let mut content = String::from("# Project environment variables from spuff.yaml\n");

        for (key, value) in &self.config.env {
            content.push_str(&format!("export {}=\"{}\"\n", key, value));
        }

        if let Err(e) = tokio::fs::write(&env_file, content).await {
            tracing::warn!("Failed to write project env file: {}", e);
        }
    }

    async fn install_bundle(&self, bundle_name: &str, index: usize) {
        // Update status to in progress
        {
            let mut state = self.state.write().await;
            if let Some(bundle) = state.bundles.get_mut(index) {
                bundle.status = SetupStatus::InProgress;
            }
        }

        let log_file = format!("/var/log/spuff/bundles/{}.log", bundle_name);

        tracing::info!("Installing bundle: {}", bundle_name);
        self.log_to_file(&log_file, &format!("[INFO] Starting {} bundle installation", bundle_name)).await;

        let result = match bundle_name {
            "rust" => self.install_rust(&log_file).await,
            "go" => self.install_go(&log_file).await,
            "python" => self.install_python(&log_file).await,
            "node" => self.install_node(&log_file).await,
            "elixir" => self.install_elixir(&log_file).await,
            "java" => self.install_java(&log_file).await,
            "zig" => self.install_zig(&log_file).await,
            "cpp" => self.install_cpp(&log_file).await,
            "ruby" => self.install_ruby(&log_file).await,
            _ => {
                tracing::warn!("Unknown bundle: {}", bundle_name);
                Err(format!("Unknown bundle: {}", bundle_name))
            }
        };

        // Update status
        {
            let mut state = self.state.write().await;
            if let Some(bundle) = state.bundles.get_mut(index) {
                match result {
                    Ok(version) => {
                        bundle.status = SetupStatus::Done;
                        bundle.version = version;
                        self.log_to_file(&log_file, &format!("[SUCCESS] {} bundle installed", bundle_name)).await;
                    }
                    Err(e) => {
                        bundle.status = SetupStatus::Failed(e.clone());
                        self.log_to_file(&log_file, &format!("[FAIL] {} bundle failed: {}", bundle_name, e)).await;
                    }
                }
            }
        }
    }

    async fn install_rust(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install rustup
        self.run_cmd_logged(
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
            log_file,
        )
        .await?;

        // Source cargo env
        let cargo_env = format!("{}/.cargo/env", self.home_dir);
        if Path::new(&cargo_env).exists() {
            std::env::set_var("PATH", format!("{}/.cargo/bin:{}", self.home_dir, std::env::var("PATH").unwrap_or_default()));
        }

        // Get version
        let version = self.get_cmd_output("~/.cargo/bin/rustc --version").await.ok();

        // Install additional tools
        self.run_cmd_logged("~/.cargo/bin/rustup component add rust-analyzer clippy rustfmt", log_file).await.ok();
        self.run_cmd_logged("~/.cargo/bin/cargo install cargo-watch cargo-edit sccache", log_file).await.ok();

        Ok(version)
    }

    async fn install_go(&self, log_file: &str) -> Result<Option<String>, String> {
        // Download and install Go
        self.run_cmd_logged(
            "curl -fsSL https://go.dev/dl/go1.22.3.linux-amd64.tar.gz | sudo tar -C /usr/local -xzf -",
            log_file,
        )
        .await?;

        // Add to PATH
        std::env::set_var("PATH", format!("/usr/local/go/bin:{}", std::env::var("PATH").unwrap_or_default()));

        // Get version
        let version = self.get_cmd_output("/usr/local/go/bin/go version").await.ok();

        // Install tools
        self.run_cmd_logged("go install golang.org/x/tools/gopls@latest", log_file).await.ok();
        self.run_cmd_logged("go install github.com/go-delve/delve/cmd/dlv@latest", log_file).await.ok();
        self.run_cmd_logged("go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest", log_file).await.ok();

        Ok(version)
    }

    async fn install_python(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install Python and tools
        self.run_cmd_logged(
            "sudo apt-get install -y python3 python3-pip python3-venv python3-dev",
            log_file,
        )
        .await?;

        // Install uv
        self.run_cmd_logged("curl -LsSf https://astral.sh/uv/install.sh | sh", log_file).await.ok();

        // Get version
        let version = self.get_cmd_output("python3 --version").await.ok();

        // Install Python tools
        self.run_cmd_logged("pip3 install --user ruff pyright ipython", log_file).await.ok();

        Ok(version)
    }

    async fn install_node(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install Node.js via NodeSource
        self.run_cmd_logged(
            "curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -",
            log_file,
        )
        .await?;

        self.run_cmd_logged("sudo apt-get install -y nodejs", log_file).await?;

        // Get version
        let version = self.get_cmd_output("node --version").await.ok();

        // Install pnpm and tools
        self.run_cmd_logged("sudo npm install -g pnpm typescript eslint prettier", log_file).await.ok();

        Ok(version)
    }

    async fn install_elixir(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install Erlang and Elixir
        self.run_cmd_logged(
            "sudo apt-get install -y erlang elixir",
            log_file,
        )
        .await?;

        // Get version
        let version = self.get_cmd_output("elixir --version | head -1").await.ok();

        Ok(version)
    }

    async fn install_java(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install OpenJDK
        self.run_cmd_logged(
            "sudo apt-get install -y openjdk-21-jdk maven gradle",
            log_file,
        )
        .await?;

        // Get version
        let version = self.get_cmd_output("java --version | head -1").await.ok();

        Ok(version)
    }

    async fn install_zig(&self, log_file: &str) -> Result<Option<String>, String> {
        // Download and install Zig
        self.run_cmd_logged(
            "curl -fsSL https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz | sudo tar -xJf - -C /usr/local && sudo ln -sf /usr/local/zig-linux-x86_64-0.13.0/zig /usr/local/bin/zig",
            log_file,
        )
        .await?;

        // Get version
        let version = self.get_cmd_output("/usr/local/bin/zig version").await.ok();

        Ok(version)
    }

    async fn install_cpp(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install C++ toolchain
        self.run_cmd_logged(
            "sudo apt-get install -y gcc g++ clang cmake ninja-build gdb lldb",
            log_file,
        )
        .await?;

        // Get version
        let version = self.get_cmd_output("g++ --version | head -1").await.ok();

        Ok(version)
    }

    async fn install_ruby(&self, log_file: &str) -> Result<Option<String>, String> {
        // Install Ruby
        self.run_cmd_logged(
            "sudo apt-get install -y ruby ruby-dev",
            log_file,
        )
        .await?;

        // Get version
        let version = self.get_cmd_output("ruby --version").await.ok();

        // Install bundler
        self.run_cmd_logged("sudo gem install bundler solargraph rubocop", log_file).await.ok();

        Ok(version)
    }

    async fn install_packages(&self) {
        if self.config.packages.is_empty() {
            return;
        }

        // Update status
        {
            let mut state = self.state.write().await;
            state.packages.status = SetupStatus::InProgress;
        }

        let log_file = "/var/log/spuff/packages.log";
        self.log_to_file(log_file, "[INFO] Starting package installation").await;

        // Update apt
        self.run_cmd_logged("sudo apt-get update", log_file).await.ok();

        let mut installed = vec![];
        let mut failed = vec![];

        for package in &self.config.packages {
            self.log_to_file(log_file, &format!("[CMD] apt-get install -y {}", package)).await;
            match self.run_cmd_logged(&format!("sudo apt-get install -y {}", package), log_file).await {
                Ok(_) => {
                    installed.push(package.clone());
                    self.log_to_file(log_file, &format!("[OK] {} installed", package)).await;
                }
                Err(e) => {
                    failed.push(package.clone());
                    self.log_to_file(log_file, &format!("[FAIL] {} failed: {}", package, e)).await;
                }
            }
        }

        // Update status
        {
            let mut state = self.state.write().await;
            state.packages.installed = installed;
            state.packages.failed = failed.clone();
            state.packages.status = if failed.is_empty() {
                SetupStatus::Done
            } else {
                SetupStatus::Failed(format!("{} packages failed", failed.len()))
            };
        }
    }

    async fn clone_repositories(&self) {
        if self.config.repositories.is_empty() {
            return;
        }

        let log_file = "/var/log/spuff/repositories.log";
        self.log_to_file(log_file, "[INFO] Starting repository cloning").await;

        // Create projects directory
        let projects_dir = format!("{}/projects", self.home_dir);
        let _ = tokio::fs::create_dir_all(&projects_dir).await;

        for (i, repo_config) in self.config.repositories.iter().enumerate() {
            // Update status
            {
                let mut state = self.state.write().await;
                if let Some(repo) = state.repositories.get_mut(i) {
                    repo.status = SetupStatus::InProgress;
                }
            }

            let url = repo_config.url();
            let path = repo_config.path(&projects_dir);
            let branch = repo_config.branch();

            self.log_to_file(log_file, &format!("[CMD] git clone {} {}", url, path)).await;

            let mut cmd = format!("git clone --depth 1 {} {}", url, path);
            if let Some(ref b) = branch {
                cmd = format!("git clone --depth 1 -b {} {} {}", b, url, path);
            }

            let result = self.run_cmd_logged(&cmd, log_file).await;

            // Update status
            {
                let mut state = self.state.write().await;
                if let Some(repo) = state.repositories.get_mut(i) {
                    match result {
                        Ok(_) => {
                            repo.status = SetupStatus::Done;
                            self.log_to_file(log_file, &format!("[OK] {} cloned", url)).await;
                        }
                        Err(e) => {
                            repo.status = SetupStatus::Failed(e.clone());
                            self.log_to_file(log_file, &format!("[FAIL] {} failed: {}", url, e)).await;
                        }
                    }
                }
            }
        }
    }

    async fn start_services(&self) {
        if !self.config.services.enabled {
            return;
        }

        // Update status
        {
            let mut state = self.state.write().await;
            state.services.status = SetupStatus::InProgress;
        }

        let log_file = "/var/log/spuff/services.log";
        self.log_to_file(log_file, "[INFO] Starting docker-compose services").await;

        // Check if compose file exists in any cloned repo
        let projects_dir = format!("{}/projects", self.home_dir);
        let compose_file = &self.config.services.compose_file;

        // Find compose file
        let mut compose_path = None;
        if let Ok(mut entries) = tokio::fs::read_dir(&projects_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path().join(compose_file);
                if path.exists() {
                    compose_path = Some(path);
                    break;
                }
            }
        }

        let Some(compose_path) = compose_path else {
            self.log_to_file(log_file, &format!("[WARN] {} not found in any repository", compose_file)).await;
            let mut state = self.state.write().await;
            state.services.status = SetupStatus::Skipped;
            return;
        };

        let compose_dir = compose_path.parent().unwrap_or(&compose_path);

        // Build docker-compose command
        let mut cmd = format!("cd {} && docker compose", compose_dir.display());

        // Add profiles if specified
        for profile in &self.config.services.profiles {
            cmd.push_str(&format!(" --profile {}", profile));
        }

        cmd.push_str(" up -d");

        self.log_to_file(log_file, &format!("[CMD] {}", cmd)).await;

        let result = self.run_cmd_logged(&cmd, log_file).await;

        // Get container status
        let containers = self.get_docker_containers().await;

        // Update status
        {
            let mut state = self.state.write().await;
            state.services.containers = containers;
            state.services.status = match result {
                Ok(_) => SetupStatus::Done,
                Err(e) => SetupStatus::Failed(e),
            };
        }
    }

    async fn get_docker_containers(&self) -> Vec<ContainerStatus> {
        let output = self.get_cmd_output("docker ps --format '{{.Names}}|{{.Status}}|{{.Ports}}'").await;
        let Ok(output) = output else {
            return vec![];
        };

        output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 2 {
                    let port = parts.get(2).and_then(|p| {
                        // Extract first port number from ports string
                        p.split(':').nth(1).and_then(|s| {
                            s.split('-').next().and_then(|s| s.parse().ok())
                        })
                    });

                    Some(ContainerStatus {
                        name: parts[0].to_string(),
                        status: if parts[1].contains("Up") { "running" } else { "stopped" }.to_string(),
                        port,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    async fn run_setup_scripts(&self) {
        if self.config.setup.is_empty() {
            return;
        }

        let projects_dir = format!("{}/projects", self.home_dir);

        for (i, script) in self.config.setup.iter().enumerate() {
            // Update status
            {
                let mut state = self.state.write().await;
                if let Some(s) = state.scripts.get_mut(i) {
                    s.status = SetupStatus::InProgress;
                }
            }

            let log_file = format!("/var/log/spuff/scripts/{:03}.log", i + 1);
            self.log_to_file(&log_file, &format!("[CMD] {}", script)).await;

            // Run script in the first project directory if it exists
            let cmd = if Path::new(&projects_dir).exists() {
                format!("cd {} && {}", projects_dir, script)
            } else {
                script.clone()
            };

            let result = self.run_cmd_as_user(&cmd, &log_file).await;

            // Update status
            {
                let mut state = self.state.write().await;
                if let Some(s) = state.scripts.get_mut(i) {
                    match result {
                        Ok(code) => {
                            s.exit_code = Some(code);
                            if code == 0 {
                                s.status = SetupStatus::Done;
                                self.log_to_file(&log_file, &format!("[SUCCESS] Script completed with exit code {}", code)).await;
                            } else {
                                s.status = SetupStatus::Failed(format!("Exit code: {}", code));
                                self.log_to_file(&log_file, &format!("[FAIL] Script failed with exit code {}", code)).await;
                            }
                        }
                        Err(e) => {
                            s.status = SetupStatus::Failed(e.clone());
                            self.log_to_file(&log_file, &format!("[FAIL] Script error: {}", e)).await;
                        }
                    }
                }
            }
        }
    }

    async fn run_hook(&self, name: &str, script: &str) {
        let log_file = format!("/var/log/spuff/hook-{}.log", name);
        self.log_to_file(&log_file, &format!("[CMD] Running {} hook", name)).await;
        self.log_to_file(&log_file, script).await;

        if let Err(e) = self.run_cmd_as_user(script, &log_file).await {
            tracing::warn!("Hook {} failed: {}", name, e);
        }
    }

    async fn run_cmd_logged(&self, cmd: &str, log_file: &str) -> Result<(), String> {
        self.log_to_file(log_file, &format!("[CMD] {}", cmd)).await;

        let output = Command::new("bash")
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| e.to_string())?;

        // Log output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            self.log_to_file(log_file, &format!("[OUTPUT] {}", stdout)).await;
        }
        if !stderr.is_empty() {
            self.log_to_file(log_file, &format!("[STDERR] {}", stderr)).await;
        }

        if output.status.success() {
            Ok(())
        } else {
            Err(format!("Command failed with code {:?}", output.status.code()))
        }
    }

    async fn run_cmd_as_user(&self, cmd: &str, log_file: &str) -> Result<i32, String> {
        self.log_to_file(log_file, &format!("[CMD] su {} -c '{}'", self.username, cmd)).await;

        let output = Command::new("su")
            .arg(&self.username)
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| e.to_string())?;

        // Log output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            self.log_to_file(log_file, &format!("[OUTPUT] {}", stdout)).await;
        }
        if !stderr.is_empty() {
            self.log_to_file(log_file, &format!("[STDERR] {}", stderr)).await;
        }

        output.status.code().ok_or_else(|| "Process terminated by signal".to_string())
    }

    async fn get_cmd_output(&self, cmd: &str) -> Result<String, String> {
        let output = Command::new("bash")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    async fn log_to_file(&self, path: &str, message: &str) {
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let log_line = format!("[{}] {}\n", timestamp, message);

        let _ = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map(|mut f| async move {
                use tokio::io::AsyncWriteExt;
                f.write_all(log_line.as_bytes()).await
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_url_short() {
        let repo = RepositoryConfig::Short("owner/repo".to_string());
        assert_eq!(repo.url(), "git@github.com:owner/repo.git");
    }

    #[test]
    fn test_repository_url_full() {
        let repo = RepositoryConfig::Full {
            url: "git@gitlab.com:owner/repo.git".to_string(),
            path: Some("~/projects/myrepo".to_string()),
            branch: Some("develop".to_string()),
        };
        assert_eq!(repo.url(), "git@gitlab.com:owner/repo.git");
        assert_eq!(repo.branch(), Some("develop".to_string()));
    }

    #[test]
    fn test_repository_path_default() {
        let repo = RepositoryConfig::Short("owner/repo".to_string());
        assert_eq!(repo.path("/home/user/projects"), "/home/user/projects/repo");
    }
}
