//! Core types for devtools installation.

use serde::{Deserialize, Serialize};

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

    /// Install Claude Code CLI (AI coding assistant from Anthropic)
    #[serde(default = "default_true")]
    pub claude_code: bool,

    /// Install Codex CLI (AI coding assistant from OpenAI)
    #[serde(default = "default_true")]
    pub codex: bool,

    /// Install OpenCode (open source AI coding assistant)
    #[serde(default = "default_true")]
    pub opencode: bool,

    /// Install GitHub Copilot CLI
    #[serde(default = "default_true")]
    pub copilot: bool,

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
                    description: "AI coding assistant (Anthropic)",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "codex",
                    name: "Codex CLI",
                    description: "AI coding assistant (OpenAI)",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "opencode",
                    name: "OpenCode",
                    description: "Open source AI coding assistant",
                    status: ToolStatus::Pending,
                    version: None,
                },
                DevTool {
                    id: "copilot",
                    name: "GitHub Copilot CLI",
                    description: "AI coding assistant (GitHub)",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devtools_config_serde_defaults() {
        let config: DevToolsConfig = serde_json::from_str("{}").unwrap();
        assert!(
            config.claude_code,
            "claude_code should default to true via serde"
        );
        assert!(config.codex, "codex should default to true via serde");
        assert!(config.opencode, "opencode should default to true via serde");
        assert!(config.copilot, "copilot should default to true via serde");
        assert!(config.docker, "docker should default to true via serde");
        assert!(
            config.shell_tools,
            "shell_tools should default to true via serde"
        );
        assert!(config.nodejs, "nodejs should default to true via serde");
    }

    #[test]
    fn test_devtools_config_deserialize_ai_tools() {
        let json = r#"{"claude_code": true, "codex": false, "opencode": true, "copilot": false}"#;
        let config: DevToolsConfig = serde_json::from_str(json).unwrap();
        assert!(config.claude_code);
        assert!(!config.codex);
        assert!(config.opencode);
        assert!(!config.copilot);
    }

    #[test]
    fn test_devtools_state_has_all_tools() {
        let state = DevToolsState::default();
        let tool_ids: Vec<&str> = state.tools.iter().map(|t| t.id).collect();

        assert!(tool_ids.contains(&"docker"));
        assert!(tool_ids.contains(&"nodejs"));
        assert!(tool_ids.contains(&"claude_code"));
        assert!(tool_ids.contains(&"codex"));
        assert!(tool_ids.contains(&"opencode"));
        assert!(tool_ids.contains(&"copilot"));
        assert!(tool_ids.contains(&"devenv"));
        assert!(tool_ids.contains(&"dotfiles"));
        assert!(tool_ids.contains(&"tailscale"));
    }

    #[test]
    fn test_tool_status_serialization() {
        assert_eq!(
            serde_json::to_string(&ToolStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&ToolStatus::Installing).unwrap(),
            "\"installing\""
        );
        assert_eq!(
            serde_json::to_string(&ToolStatus::Done).unwrap(),
            "\"done\""
        );
    }
}
