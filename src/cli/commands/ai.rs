use console::style;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::{AiToolsConfig, ProjectConfig};
use crate::state::StateDb;

/// Available AI coding tools
const AI_TOOLS: &[(&str, &str, &str)] = &[
    (
        "claude-code",
        "Anthropic's Claude Code CLI",
        "npm install -g @anthropic-ai/claude-code",
    ),
    ("codex", "OpenAI Codex CLI", "npm install -g @openai/codex"),
    (
        "opencode",
        "Open-source AI coding assistant",
        "npm i -g opencode-ai",
    ),
    (
        "copilot",
        "GitHub Copilot CLI",
        "npm install -g @github/copilot",
    ),
    (
        "cursor",
        "Cursor AI coding assistant CLI",
        "npm install -g @anthropics/cursor-cli",
    ),
    (
        "cody",
        "Sourcegraph Cody AI assistant",
        "npm install -g @sourcegraph/cody",
    ),
    (
        "aider",
        "AI pair programming with git integration",
        "pipx install aider-chat",
    ),
    (
        "gemini",
        "Google Gemini AI CLI",
        "npm install -g @anthropics/gemini-cli",
    ),
];

/// Response from agent's /devtools endpoint
#[derive(Debug, Deserialize)]
struct DevToolsResponse {
    tools: Vec<DevTool>,
}

/// A single devtool from the agent response
#[derive(Debug, Deserialize)]
struct DevTool {
    id: String,
    status: ToolStatus,
    #[serde(default)]
    version: Option<String>,
}

/// Status of a single devtool installation (matches agent's ToolStatus enum)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolStatus {
    Pending,
    Installing,
    Done,
    Failed(String),
    Skipped,
}

/// List available AI coding tools
pub async fn list() -> Result<()> {
    println!("{}", style("Available AI Coding Tools").bold().cyan());
    println!();

    // Load current config to show which tools are enabled
    let project_config = ProjectConfig::load_from_cwd().ok().flatten();
    let app_config = AppConfig::load().ok();

    // Determine effective AI tools config
    let ai_config = project_config
        .as_ref()
        .map(|pc| &pc.ai_tools)
        .or(app_config.as_ref().and_then(|ac| ac.ai_tools.as_ref()))
        .cloned()
        .unwrap_or(AiToolsConfig::All);

    let enabled_tools = ai_config.tools_to_install();

    for (name, description, install_cmd) in AI_TOOLS {
        let is_enabled = enabled_tools.contains(name);
        let status = if is_enabled {
            style("enabled").green()
        } else {
            style("disabled").dim()
        };

        println!(
            "  {} {} [{}]",
            style(name).green().bold(),
            style("-").dim(),
            status
        );
        println!("    {}", description);
        println!("    Install: {}", style(install_cmd).dim());
        println!();
    }

    println!(
        "{} Use {} to configure which tools to install",
        style("Tip:").cyan(),
        style("spuff.yaml").yellow()
    );
    println!(
        "     Or use {} flag with spuff up",
        style("--ai-tools").yellow()
    );

    Ok(())
}

/// Map CLI tool names to agent tool IDs
fn cli_to_agent_id(cli_name: &str) -> &str {
    match cli_name {
        "claude-code" => "claude_code",
        other => other,
    }
}

/// Show AI tools installation status on remote environment
pub async fn status(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!(
        "{} Fetching AI tools status from {}...\n",
        style("→").cyan().bold(),
        style(&instance.name).cyan()
    );

    let response: DevToolsResponse = agent_request(&instance.ip, config, "/devtools").await?;

    // Build a map from tool id to tool for quick lookup
    let tools_map: std::collections::HashMap<&str, &DevTool> =
        response.tools.iter().map(|t| (t.id.as_str(), t)).collect();

    println!("{}", style("AI Tools Status").bold().cyan());
    println!();

    for (name, _, _) in AI_TOOLS {
        let agent_id = cli_to_agent_id(name);
        let (status_text, status_style): (String, fn(String) -> _) =
            if let Some(tool) = tools_map.get(agent_id) {
                match &tool.status {
                    ToolStatus::Done => {
                        let version_info = tool
                            .version
                            .as_ref()
                            .map(|v| format!(" ({})", v))
                            .unwrap_or_default();
                        (format!("installed{}", version_info), |s| style(s).green())
                    }
                    ToolStatus::Installing => ("installing".to_string(), |s| style(s).yellow()),
                    ToolStatus::Failed(msg) => (format!("failed: {}", msg), |s| style(s).red()),
                    ToolStatus::Skipped => ("skipped".to_string(), |s| style(s).dim()),
                    ToolStatus::Pending => ("pending".to_string(), |s| style(s).dim()),
                }
            } else {
                ("not configured".to_string(), |s| style(s).dim())
            };

        println!(
            "  {:<15} {}",
            style(*name).white(),
            status_style(status_text)
        );
    }

    Ok(())
}

/// Install a specific AI tool on the remote environment
pub async fn install(config: &AppConfig, tool: String) -> Result<()> {
    // Validate tool name
    let valid_tools: Vec<&str> = AI_TOOLS.iter().map(|(name, _, _)| *name).collect();
    if !valid_tools.contains(&tool.as_str()) {
        return Err(SpuffError::Config(format!(
            "Unknown AI tool '{}'. Available: {}",
            tool,
            valid_tools.join(", ")
        )));
    }

    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    println!(
        "{} Installing {} on {}...",
        style("→").cyan().bold(),
        style(&tool).green(),
        style(&instance.name).cyan()
    );

    // Build config with only the requested tool enabled
    // Explicitly disable non-AI devtools to prevent reinstallation (serde defaults are true)
    // Base config disables all AI tools and non-AI devtools
    let base = r#""docker":false,"shell_tools":false,"nodejs":false"#;
    let all_ai_false = r#""claude_code":false,"codex":false,"opencode":false,"copilot":false,"cursor":false,"cody":false,"aider":false,"gemini":false"#;

    let config_json = match tool.as_str() {
        "claude-code" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"claude_code\":false", "\"claude_code\":true"),
                base
            )
        }
        "codex" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"codex\":false", "\"codex\":true"),
                base
            )
        }
        "opencode" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"opencode\":false", "\"opencode\":true"),
                base
            )
        }
        "copilot" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"copilot\":false", "\"copilot\":true"),
                base
            )
        }
        "cursor" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"cursor\":false", "\"cursor\":true"),
                base
            )
        }
        "cody" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"cody\":false", "\"cody\":true"),
                base
            )
        }
        "aider" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"aider\":false", "\"aider\":true"),
                base
            )
        }
        "gemini" => {
            format!(
                r#"{{{},{}}}"#,
                all_ai_false.replace("\"gemini\":false", "\"gemini\":true"),
                base
            )
        }
        _ => unreachable!(),
    };

    // POST to /devtools/install
    let output = crate::connector::ssh::run_command(
        &instance.ip,
        config,
        &format!(
            "curl -s -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:7575/devtools/install",
            config_json
        ),
    )
    .await?;

    // Check response
    if output.contains("error") && !output.contains("installation started") {
        println!(
            "{} Installation request failed: {}",
            style("✗").red().bold(),
            output.trim()
        );
    } else {
        println!(
            "{} Installation started. Use {} to check progress.",
            style("✓").green().bold(),
            style("spuff ai status").yellow()
        );
    }

    Ok(())
}

/// Show information about a specific AI tool
pub async fn info(tool: &str) -> Result<()> {
    let tool_info = AI_TOOLS
        .iter()
        .find(|(name, _, _)| *name == tool)
        .ok_or_else(|| {
            let valid_tools: Vec<&str> = AI_TOOLS.iter().map(|(name, _, _)| *name).collect();
            SpuffError::Config(format!(
                "Unknown AI tool '{}'. Available: {}",
                tool,
                valid_tools.join(", ")
            ))
        })?;

    let (name, description, install_cmd) = tool_info;

    println!("{}", style(*name).bold().cyan());
    println!();
    println!("  {}", description);
    println!();
    println!("  {}: {}", style("Install command").dim(), install_cmd);
    println!();

    // Tool-specific details
    match *name {
        "claude-code" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires ANTHROPIC_API_KEY environment variable");
            println!("    Documentation: https://docs.anthropic.com/claude-code");
        }
        "codex" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires OPENAI_API_KEY environment variable");
            println!("    Documentation: https://github.com/openai/codex-cli");
        }
        "opencode" => {
            println!("  {}", style("Configuration").bold());
            println!("    Open-source, supports multiple AI providers");
            println!("    Documentation: https://opencode.ai");
        }
        "copilot" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires active GitHub Copilot subscription");
            println!("    Auth: /login command or GH_TOKEN/GITHUB_TOKEN env var");
            println!("    Documentation: https://github.com/github/copilot-cli");
        }
        "cursor" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires Cursor account and API key");
            println!("    Set CURSOR_API_KEY environment variable");
            println!("    Documentation: https://cursor.sh/docs");
        }
        "cody" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires Sourcegraph account");
            println!("    Set SRC_ACCESS_TOKEN and SRC_ENDPOINT env vars");
            println!("    Documentation: https://sourcegraph.com/docs/cody");
        }
        "aider" => {
            println!("  {}", style("Configuration").bold());
            println!("    Works with multiple AI providers (OpenAI, Anthropic, etc.)");
            println!("    Set OPENAI_API_KEY or ANTHROPIC_API_KEY env var");
            println!("    Excellent git integration for pair programming");
            println!("    Documentation: https://aider.chat");
        }
        "gemini" => {
            println!("  {}", style("Configuration").bold());
            println!("    Requires Google AI API key");
            println!("    Set GOOGLE_API_KEY environment variable");
            println!("    Documentation: https://ai.google.dev/docs");
        }
        _ => {}
    }

    Ok(())
}

/// Extract JSON from output that may contain banner text
fn extract_json(output: &str) -> &str {
    if let Some(start) = output.find('{') {
        let mut depth = 0;
        for (i, c) in output[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &output[start..=start + i];
                    }
                }
                _ => {}
            }
        }
    }
    output.trim()
}

async fn agent_request<T: serde::de::DeserializeOwned>(
    ip: &str,
    config: &AppConfig,
    endpoint: &str,
) -> Result<T> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        &format!("curl -s http://127.0.0.1:7575{}", endpoint),
    )
    .await?;

    let json_str = extract_json(&output);

    serde_json::from_str(json_str).map_err(|e| {
        SpuffError::Provider(format!(
            "Failed to parse agent response: {}. Response: {}",
            e, output
        ))
    })
}
