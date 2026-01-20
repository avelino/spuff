//! Development environment installation (devbox, nix, dotfiles, tailscale).

use super::installer::DevToolsInstaller;
use super::types::ToolStatus;

/// Installer for development environments
pub struct EnvironmentInstaller<'a> {
    installer: &'a DevToolsInstaller,
}

impl<'a> EnvironmentInstaller<'a> {
    pub fn new(installer: &'a DevToolsInstaller) -> Self {
        Self { installer }
    }

    pub async fn install_devbox(&self) {
        self.installer
            .update_status("devenv", ToolStatus::Installing, None)
            .await;

        let cmd = format!(
            "curl -fsSL https://get.jetify.com/devbox | bash -s -- -f && \
             su - {} -c 'devbox version' || true",
            self.installer.username
        );

        match self.installer.run_command(&cmd).await {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command(&format!(
                        "su - {} -c 'devbox version 2>/dev/null'",
                        self.installer.username
                    ))
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("devenv", ToolStatus::Done, version)
                    .await;
                tracing::info!("Devbox installed");
            }
            Err(e) => {
                self.installer
                    .update_status("devenv", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Devbox installation failed: {}", e);
            }
        }
    }

    pub async fn install_nix(&self) {
        self.installer
            .update_status("devenv", ToolStatus::Installing, None)
            .await;

        let cmd = format!(
            "curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install --no-confirm && \
             echo '. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh' >> /home/{}/.bashrc",
            self.installer.username
        );

        match self.installer.run_command(&cmd).await {
            Ok(_) => {
                self.installer
                    .update_status("devenv", ToolStatus::Done, Some("nix".to_string()))
                    .await;
                tracing::info!("Nix installed");
            }
            Err(e) => {
                self.installer
                    .update_status("devenv", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Nix installation failed: {}", e);
            }
        }
    }

    pub async fn install_dotfiles(&self, url: &str) {
        self.installer
            .update_status("dotfiles", ToolStatus::Installing, None)
            .await;

        let username = &self.installer.username;
        let cmd = format!(
            "su - {} -c 'git clone {} ~/.dotfiles' && \
             if [ -f /home/{}/.dotfiles/install.sh ]; then \
                 su - {} -c 'cd ~/.dotfiles && ./install.sh'; \
             elif [ -f /home/{}/.dotfiles/setup.sh ]; then \
                 su - {} -c 'cd ~/.dotfiles && ./setup.sh'; \
             fi",
            username, url, username, username, username, username
        );

        match self.installer.run_command(&cmd).await {
            Ok(_) => {
                self.installer
                    .update_status("dotfiles", ToolStatus::Done, None)
                    .await;
                tracing::info!("Dotfiles installed");
            }
            Err(e) => {
                self.installer
                    .update_status("dotfiles", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Dotfiles installation failed: {}", e);
            }
        }
    }

    pub async fn install_tailscale(&self) {
        self.installer
            .update_status("tailscale", ToolStatus::Installing, None)
            .await;

        let mut cmd = "curl -fsSL https://tailscale.com/install.sh | sh".to_string();

        if let Some(ref authkey) = self.installer.config.tailscale_authkey {
            cmd.push_str(&format!(" && tailscale up --authkey='{}'", authkey));
        }

        match self.installer.run_command(&cmd).await {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command("tailscale --version 2>/dev/null | head -1")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("tailscale", ToolStatus::Done, version)
                    .await;
                tracing::info!("Tailscale installed");
            }
            Err(e) => {
                self.installer
                    .update_status("tailscale", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Tailscale installation failed: {}", e);
            }
        }
    }
}
