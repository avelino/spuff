//! PTY (pseudo-terminal) handling for interactive SSH sessions.
//!
//! Provides full terminal emulation for shell sessions.

use russh::client::Handle;
use russh::ChannelMsg;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{Result, SpuffError};
use crate::ssh::client::ClientHandler;

/// Start an interactive shell session.
///
/// This allocates a PTY and starts a shell, forwarding all I/O.
pub async fn interactive_shell(session: &Handle<ClientHandler>) -> Result<()> {
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to open channel: {}", e)))?;

    // Get terminal size
    let (width, height) = get_terminal_size();

    // Request PTY
    channel
        .request_pty(
            true,
            "xterm-256color",
            width as u32,
            height as u32,
            0,
            0,
            &[], // No special modes
        )
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to request PTY: {}", e)))?;

    // Request shell
    channel
        .request_shell(true)
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to request shell: {}", e)))?;

    // Set up raw terminal mode
    let _raw_guard = setup_raw_terminal()?;

    // I/O loop
    io_loop(&mut channel).await
}

/// Execute a command interactively with PTY.
///
/// Returns the exit code.
pub async fn exec_interactive(session: &Handle<ClientHandler>, command: &str) -> Result<i32> {
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to open channel: {}", e)))?;

    // Get terminal size
    let (width, height) = get_terminal_size();

    // Request PTY
    channel
        .request_pty(
            true,
            "xterm-256color",
            width as u32,
            height as u32,
            0,
            0,
            &[],
        )
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to request PTY: {}", e)))?;

    // Execute command
    channel
        .exec(true, command.as_bytes())
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to execute command: {}", e)))?;

    // Set up raw terminal mode
    let _raw_guard = setup_raw_terminal()?;

    // I/O loop
    let exit_code = io_loop_with_exit(&mut channel).await?;

    Ok(exit_code as i32)
}

/// Main I/O loop for interactive sessions.
async fn io_loop(channel: &mut russh::Channel<russh::client::Msg>) -> Result<()> {
    io_loop_with_exit(channel).await?;
    Ok(())
}

/// Main I/O loop that returns exit code.
#[cfg(unix)]
async fn io_loop_with_exit(channel: &mut russh::Channel<russh::client::Msg>) -> Result<u32> {
    let mut exit_code = 0u32;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // Input buffer
    let mut input_buf = [0u8; 1024];

    // Set up signal handler for Ctrl+C
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| SpuffError::Ssh(format!("Failed to set up signal handler: {}", e)))?;

    loop {
        tokio::select! {
            // Handle Ctrl+C signal - break cleanly to allow Drop to run
            _ = sigint.recv() => {
                tracing::debug!("Received SIGINT, closing session");
                break;
            }

            // Read from stdin and send to remote
            result = stdin.read(&mut input_buf) => {
                match result {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        channel.data(&input_buf[..n]).await
                            .map_err(|e| SpuffError::Ssh(format!("Failed to send data: {}", e)))?;
                    }
                    Err(e) => {
                        tracing::warn!("stdin read error: {}", e);
                        break;
                    }
                }
            }

            // Read from remote and write to stdout
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        stdout.write_all(&data).await
                            .map_err(|e| SpuffError::Ssh(format!("Failed to write to stdout: {}", e)))?;
                        stdout.flush().await.ok();
                    }
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                        // stderr
                        let mut stderr = tokio::io::stderr();
                        stderr.write_all(&data).await.ok();
                        stderr.flush().await.ok();
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
        }
    }

    Ok(exit_code)
}

/// Main I/O loop that returns exit code (non-Unix fallback).
#[cfg(not(unix))]
async fn io_loop_with_exit(channel: &mut russh::Channel<russh::client::Msg>) -> Result<u32> {
    let mut exit_code = 0u32;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // Input buffer
    let mut input_buf = [0u8; 1024];

    loop {
        tokio::select! {
            // Read from stdin and send to remote
            result = stdin.read(&mut input_buf) => {
                match result {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        channel.data(&input_buf[..n]).await
                            .map_err(|e| SpuffError::Ssh(format!("Failed to send data: {}", e)))?;
                    }
                    Err(e) => {
                        tracing::warn!("stdin read error: {}", e);
                        break;
                    }
                }
            }

            // Read from remote and write to stdout
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        stdout.write_all(&data).await
                            .map_err(|e| SpuffError::Ssh(format!("Failed to write to stdout: {}", e)))?;
                        stdout.flush().await.ok();
                    }
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                        // stderr
                        let mut stderr = tokio::io::stderr();
                        stderr.write_all(&data).await.ok();
                        stderr.flush().await.ok();
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
        }
    }

    Ok(exit_code)
}

/// Get current terminal size.
fn get_terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

/// Set up raw terminal mode.
fn setup_raw_terminal() -> Result<RawModeGuard> {
    // Create guard first to save original terminal state
    let guard = RawModeGuard::new();

    crossterm::terminal::enable_raw_mode()
        .map_err(|e| SpuffError::Ssh(format!("Failed to enable raw mode: {}", e)))?;

    Ok(guard)
}

/// RAII guard to restore terminal mode on drop.
struct RawModeGuard {
    #[cfg(unix)]
    original_termios: Option<nix::sys::termios::Termios>,
}

impl RawModeGuard {
    fn new() -> Self {
        #[cfg(unix)]
        {
            use nix::sys::termios;

            // Save original terminal settings
            let original = termios::tcgetattr(std::io::stdin()).ok();
            Self {
                original_termios: original,
            }
        }
        #[cfg(not(unix))]
        {
            Self {}
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // First disable crossterm's raw mode
        let _ = crossterm::terminal::disable_raw_mode();

        #[cfg(unix)]
        {
            use nix::sys::termios;

            // Restore original terminal settings if we have them
            if let Some(ref original) = self.original_termios {
                let _ = termios::tcsetattr(std::io::stdin(), termios::SetArg::TCSANOW, original);
            }

            // Run stty sane to ensure terminal is in a clean state
            // This is what you'd do manually if the terminal is messed up
            let _ = std::process::Command::new("stty")
                .arg("sane")
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_terminal_size() {
        let (width, height) = get_terminal_size();
        // Should return some reasonable values
        assert!(width > 0);
        assert!(height > 0);
    }
}
