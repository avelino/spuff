//! System tools installation (Docker, shell tools, Node.js).

use super::installer::DevToolsInstaller;
use super::types::{DevToolsConfig, ToolStatus};

/// Installer for system-level tools
pub struct SystemInstaller<'a> {
    installer: &'a DevToolsInstaller,
}

impl<'a> SystemInstaller<'a> {
    pub fn new(installer: &'a DevToolsInstaller) -> Self {
        Self { installer }
    }

    pub fn install_docker(&self) -> tokio::task::JoinHandle<()> {
        let state = self.installer.state.clone();
        let username = self.installer.username.clone();

        tokio::spawn(async move {
            let installer =
                DevToolsInstaller::new(state, username.clone(), DevToolsConfig::default());

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
                        .run_command(&format!("usermod -aG docker {}", username))
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

    pub async fn install_shell_tools(&self) {
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

    fn install_fzf(&self) -> tokio::task::JoinHandle<()> {
        let state = self.installer.state.clone();
        let username = self.installer.username.clone();

        tokio::spawn(async move {
            let installer =
                DevToolsInstaller::new(state, username.clone(), DevToolsConfig::default());

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
        let state = self.installer.state.clone();

        tokio::spawn(async move {
            let installer = DevToolsInstaller::new(state, String::new(), DevToolsConfig::default());

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
        let state = self.installer.state.clone();

        tokio::spawn(async move {
            let installer = DevToolsInstaller::new(state, String::new(), DevToolsConfig::default());

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
        let state = self.installer.state.clone();

        tokio::spawn(async move {
            let installer = DevToolsInstaller::new(state, String::new(), DevToolsConfig::default());

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
        let state = self.installer.state.clone();

        tokio::spawn(async move {
            let installer = DevToolsInstaller::new(state, String::new(), DevToolsConfig::default());

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

    pub async fn install_nodejs(&self) {
        self.installer
            .update_status("nodejs", ToolStatus::Installing, None)
            .await;

        let cmd =
            "curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs";

        match self.installer.run_command(cmd).await {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command("node --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("nodejs", ToolStatus::Done, version)
                    .await;
                tracing::info!("Node.js installed");
            }
            Err(e) => {
                self.installer
                    .update_status("nodejs", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Node.js installation failed: {}", e);
            }
        }
    }
}
