//! Exec command
//!
//! Execute commands on the remote environment.

use std::io::IsTerminal;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;

use super::http::agent_request_post;
use super::types::AgentExecResponse;

/// Commands known to require interactive TTY (editors, pagers, monitors, REPLs, shells).
const INTERACTIVE_COMMANDS: &[&str] = &[
    // Shells
    "bash", "sh", "zsh", "fish", "dash", "ksh", "csh", "tcsh", // Editors
    "vim", "vi", "nano", "emacs", "nvim", "helix", "hx", // Pagers
    "less", "more", "man", // Monitors
    "htop", "top", "iotop", "btop", "glances", "nmon", // REPLs
    "python", "python3", "node", "irb", "iex", "ghci", "lua", "erl", // Multiplexers
    "tmux", "screen", // Interactive git commands (partial matches handled separately)
    "tig",
];

/// Execute a command on the remote environment.
///
/// Automatically chooses between:
/// - Agent HTTP: faster, lower overhead, good for non-interactive commands
/// - SSH with TTY: required for interactive commands (editors, REPLs, etc.)
///
/// The decision can be overridden with `-t` (force TTY) or `-T` (force no TTY).
pub async fn exec(
    config: &AppConfig,
    command: String,
    force_tty: bool,
    no_tty: bool,
) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    // Clone needed values before dropping db to release the lock
    let instance_id = instance.id.clone();
    let instance_ip = instance.ip.clone();
    let is_docker = instance.provider == "docker" || instance.provider == "local";
    drop(db);

    let needs_tty = match (force_tty, no_tty) {
        (true, _) => true,  // -t flag: force TTY
        (_, true) => false, // -T flag: force no TTY
        _ => detect_needs_tty(&command),
    };

    if is_docker {
        // Docker mode: use docker exec
        if needs_tty {
            let exit_code =
                crate::connector::docker::exec_interactive(&instance_id, &command).await?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        } else {
            let output = crate::connector::docker::run_command(&instance_id, &command).await?;
            print!("{}", output);
        }
    } else {
        // Cloud mode: use SSH
        if needs_tty {
            // Interactive mode via SSH with PTY
            let exit_code =
                crate::connector::ssh::exec_interactive(&instance_ip, config, &command).await?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        } else {
            // Non-interactive mode via agent HTTP (faster)
            exec_via_agent(&instance_ip, config, &command).await?;
        }
    }

    Ok(())
}

/// Detect if a command needs TTY based on:
/// 1. Whether stdout is a terminal (if piped, no TTY needed)
/// 2. Whether the command is in the list of known interactive commands
fn detect_needs_tty(command: &str) -> bool {
    // If stdout is not a terminal (e.g., piped), don't need TTY
    if !std::io::stdout().is_terminal() {
        return false;
    }

    // Check if command is known to be interactive
    is_interactive_command(command)
}

/// Check if a command is known to require interactive TTY.
fn is_interactive_command(command: &str) -> bool {
    // Get the base command (first word)
    let base_cmd = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");

    // Check direct match
    if INTERACTIVE_COMMANDS.contains(&base_cmd) {
        return true;
    }

    // Check for interactive git commands
    if base_cmd == "git" {
        let cmd_lower = command.to_lowercase();
        if cmd_lower.contains(" -i")
            || cmd_lower.contains(" -p")
            || cmd_lower.contains(" add -i")
            || cmd_lower.contains(" add -p")
            || cmd_lower.contains(" rebase -i")
        {
            return true;
        }
    }

    false
}

/// Execute command via agent HTTP endpoint.
async fn exec_via_agent(ip: &str, config: &AppConfig, command: &str) -> Result<()> {
    let response: AgentExecResponse = agent_request_post(
        ip,
        config,
        "/exec",
        &serde_json::json!({ "command": command }),
    )
    .await?;

    // Output stdout/stderr
    if !response.stdout.is_empty() {
        print!("{}", response.stdout);
    }
    if !response.stderr.is_empty() {
        eprint!("{}", response.stderr);
    }

    if response.exit_code != 0 {
        std::process::exit(response.exit_code);
    }

    Ok(())
}
