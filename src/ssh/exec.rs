//! Remote command execution.
//!
//! Provides non-interactive command execution with stdout/stderr capture.

use russh::client::Handle;
use russh::ChannelMsg;

use crate::error::{Result, SpuffError};
use crate::ssh::client::ClientHandler;

/// Output from a remote command execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Standard output.
    pub stdout: String,

    /// Standard error.
    pub stderr: String,

    /// Whether the command succeeded (exit_code == 0).
    pub success: bool,
}

impl CommandOutput {
    /// Create a new command output.
    fn new(stdout: String, stderr: String, exit_code: u32) -> Self {
        Self {
            stdout,
            stderr,
            success: exit_code == 0,
        }
    }
}

/// Execute a command on the remote host (non-interactive).
pub async fn exec_command(session: &Handle<ClientHandler>, command: &str) -> Result<CommandOutput> {
    let wrapped_command = format!(
        "bash --norc --noprofile -c '{}'",
        command.replace('\'', "'\\''")
    );

    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to open channel: {}", e)))?;

    channel
        .exec(true, wrapped_command.as_bytes())
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to execute command: {}", e)))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = 0u32;

    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                stdout.extend_from_slice(&data);
            }
            Some(ChannelMsg::ExtendedData { data, ext }) => {
                if ext == 1 {
                    stderr.extend_from_slice(&data);
                }
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                exit_code = exit_status;
            }
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                break;
            }
            _ => {}
        }
    }

    let stdout_str = String::from_utf8_lossy(&stdout).to_string();
    let stderr_str = String::from_utf8_lossy(&stderr).to_string();

    if exit_code != 0
        && (stderr_str.contains("Permission denied") || stderr_str.contains("passphrase"))
    {
        return Err(SpuffError::Ssh(
            "SSH key requires passphrase. Run 'ssh-add' first to add your key to the agent."
                .to_string(),
        ));
    }

    Ok(CommandOutput::new(stdout_str, stderr_str, exit_code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_output_success() {
        let output = CommandOutput::new("hello".to_string(), String::new(), 0);
        assert!(output.success);
        assert_eq!(output.stdout, "hello");
    }

    #[test]
    fn test_command_output_failure() {
        let output = CommandOutput::new(String::new(), "error".to_string(), 1);
        assert!(!output.success);
        assert_eq!(output.stderr, "error");
    }
}
