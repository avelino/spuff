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
];

#[derive(Debug, Deserialize)]
struct DevToolState {
    tools: std::collections::HashMap<String, ToolStatus>,
}

#[derive(Debug, Deserialize)]
struct ToolStatus {
    status: String,
    #[serde(default)]
    error: Option<String>,
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

    let state: DevToolState = agent_request(&instance.ip, config, "/devtools").await?;

    println!("{}", style("AI Tools Status").bold().cyan());
    println!();

    for (name, _, _) in AI_TOOLS {
        let (status_text, status_style): (String, fn(String) -> _) =
            if let Some(tool) = state.tools.get(*name) {
                match tool.status.as_str() {
                    "installed" => ("installed".to_string(), |s| style(s).green()),
                    "installing" => ("installing".to_string(), |s| style(s).yellow()),
                    "failed" => {
                        let msg = tool.error.as_deref().unwrap_or("unknown error");
                        (format!("failed: {}", msg), |s| style(s).red())
                    }
                    "skipped" => ("skipped".to_string(), |s| style(s).dim()),
                    "pending" => ("pending".to_string(), |s| style(s).dim()),
                    _ => (tool.status.clone(), |s| style(s).white()),
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
    let config_json = match tool.as_str() {
        "claude-code" => r#"{"claude_code":true,"codex":false,"opencode":false}"#,
        "codex" => r#"{"claude_code":false,"codex":true,"opencode":false}"#,
        "opencode" => r#"{"claude_code":false,"codex":false,"opencode":true}"#,
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
