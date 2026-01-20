//! AI coding tools installation (claude-code, codex, opencode).

use super::installer::DevToolsInstaller;
use super::types::ToolStatus;

/// Installer for AI coding tools
pub struct AiToolsInstaller<'a> {
    installer: &'a DevToolsInstaller,
}

impl<'a> AiToolsInstaller<'a> {
    pub fn new(installer: &'a DevToolsInstaller) -> Self {
        Self { installer }
    }

    pub async fn install_claude_code(&self) {
        self.installer
            .update_status("claude_code", ToolStatus::Installing, None)
            .await;

        match self
            .installer
            .run_command("npm install -g @anthropic-ai/claude-code")
            .await
        {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command("claude --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("claude_code", ToolStatus::Done, version)
                    .await;
                tracing::info!("Claude Code installed");
            }
            Err(e) => {
                self.installer
                    .update_status("claude_code", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Claude Code installation failed: {}", e);
            }
        }
    }

    pub async fn install_codex(&self) {
        self.installer
            .update_status("codex", ToolStatus::Installing, None)
            .await;

        match self
            .installer
            .run_command("npm install -g @openai/codex")
            .await
        {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command("codex --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("codex", ToolStatus::Done, version)
                    .await;
                tracing::info!("Codex CLI installed");
            }
            Err(e) => {
                self.installer
                    .update_status("codex", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("Codex CLI installation failed: {}", e);
            }
        }
    }

    pub async fn install_opencode(&self) {
        self.installer
            .update_status("opencode", ToolStatus::Installing, None)
            .await;

        match self
            .installer
            .run_command("npm install -g opencode-ai")
            .await
        {
            Ok(_) => {
                let version = self
                    .installer
                    .run_command("opencode --version 2>/dev/null")
                    .await
                    .ok()
                    .map(|v| v.trim().to_string());
                self.installer
                    .update_status("opencode", ToolStatus::Done, version)
                    .await;
                tracing::info!("OpenCode installed");
            }
            Err(e) => {
                self.installer
                    .update_status("opencode", ToolStatus::Failed(e.clone()), None)
                    .await;
                tracing::error!("OpenCode installation failed: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// AI tool installation specification (test-only)
    #[derive(Debug, Clone)]
    pub struct AiToolSpec {
        pub id: &'static str,
        pub npm_package: &'static str,
        pub binary: &'static str,
        pub version_cmd: &'static str,
    }

    /// Specifications for all AI coding tools (test-only)
    pub const AI_TOOL_SPECS: &[AiToolSpec] = &[
        AiToolSpec {
            id: "claude-code",
            npm_package: "@anthropic-ai/claude-code",
            binary: "claude",
            version_cmd: "claude --version",
        },
        AiToolSpec {
            id: "codex",
            npm_package: "@openai/codex",
            binary: "codex",
            version_cmd: "codex --version",
        },
        AiToolSpec {
            id: "opencode",
            npm_package: "opencode-ai",
            binary: "opencode",
            version_cmd: "opencode --version",
        },
    ];

    /// Check if a binary is available in PATH (test-only)
    pub fn is_binary_in_path(binary: &str) -> bool {
        std::process::Command::new("which")
            .arg(binary)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get version of a tool if installed (test-only)
    pub fn get_tool_version(version_cmd: &str) -> Option<String> {
        let parts: Vec<&str> = version_cmd.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        std::process::Command::new(parts[0])
            .args(&parts[1..])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
    }

    #[test]
    fn test_ai_tool_specs_have_required_fields() {
        for spec in AI_TOOL_SPECS {
            assert!(!spec.id.is_empty(), "Tool ID should not be empty");
            assert!(
                !spec.npm_package.is_empty(),
                "npm package should not be empty"
            );
            assert!(!spec.binary.is_empty(), "Binary name should not be empty");
            assert!(
                !spec.version_cmd.is_empty(),
                "Version command should not be empty"
            );
        }
    }

    #[test]
    fn test_ai_tool_specs_version_cmd_uses_correct_binary() {
        for spec in AI_TOOL_SPECS {
            assert!(
                spec.version_cmd.starts_with(spec.binary),
                "Version command for {} should start with binary name '{}'",
                spec.id,
                spec.binary
            );
        }
    }

    #[test]
    fn test_ai_tool_specs_has_all_tools() {
        let tool_ids: Vec<&str> = AI_TOOL_SPECS.iter().map(|s| s.id).collect();
        assert!(tool_ids.contains(&"claude-code"));
        assert!(tool_ids.contains(&"codex"));
        assert!(tool_ids.contains(&"opencode"));
    }

    #[test]
    fn test_ai_tool_npm_packages() {
        let specs: std::collections::HashMap<&str, &str> = AI_TOOL_SPECS
            .iter()
            .map(|s| (s.id, s.npm_package))
            .collect();

        assert_eq!(specs.get("claude-code"), Some(&"@anthropic-ai/claude-code"));
        assert_eq!(specs.get("codex"), Some(&"@openai/codex"));
        assert_eq!(specs.get("opencode"), Some(&"opencode-ai"));
    }

    /// Integration test: verify AI tools are in PATH after installation
    /// Run with: cargo test --bin spuff-agent -- --ignored test_ai_tools_in_path
    #[test]
    #[ignore]
    fn test_ai_tools_in_path_after_install() {
        for spec in AI_TOOL_SPECS {
            let in_path = is_binary_in_path(spec.binary);
            if in_path {
                println!("{}: found in PATH", spec.id);
                let version = get_tool_version(spec.version_cmd);
                println!("  version: {:?}", version);
                assert!(version.is_some(), "{} should report version", spec.id);
            } else {
                println!(
                    "{}: NOT in PATH (install with: npm i -g {})",
                    spec.id, spec.npm_package
                );
            }
        }
    }

    /// Integration test: install and verify a single AI tool
    /// Run with: cargo test --bin spuff-agent -- --ignored test_install_and_verify
    #[test]
    #[ignore]
    fn test_install_and_verify_ai_tool() {
        if !is_binary_in_path("npm") {
            println!("Skipping: npm not installed");
            return;
        }

        for spec in AI_TOOL_SPECS {
            println!("Testing {}", spec.id);

            if is_binary_in_path(spec.binary) {
                println!("  Already installed, verifying version...");
                let version = get_tool_version(spec.version_cmd);
                assert!(version.is_some(), "{} should report version", spec.id);
                println!("  Version: {:?}", version);
                continue;
            }

            println!("  Installing via npm i -g {}...", spec.npm_package);
            let install = std::process::Command::new("npm")
                .args(["install", "-g", spec.npm_package])
                .output();

            match install {
                Ok(output) if output.status.success() => {
                    println!("  Installed successfully");
                    assert!(
                        is_binary_in_path(spec.binary),
                        "{} binary should be in PATH after installation",
                        spec.binary
                    );

                    let version = get_tool_version(spec.version_cmd);
                    assert!(
                        version.is_some(),
                        "{} should report version after install",
                        spec.id
                    );
                    println!("  Version: {:?}", version);
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    println!("  Installation failed: {}", stderr);
                }
                Err(e) => {
                    println!("  Failed to run npm: {}", e);
                }
            }
        }
    }
}
