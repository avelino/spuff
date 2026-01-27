//! Docker connector for executing commands in containers.
//!
//! This module provides command execution capabilities inside Docker containers
//! using the bollard library's exec API, as an alternative to SSH for local
//! Docker-based development environments.

use bollard::exec::{CreateExecOptions, ResizeExecOptions, StartExecOptions, StartExecResults};
use bollard::Docker;
use crossterm::event::{Event, EventStream, KeyModifiers};
use crossterm::terminal;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, SpuffError};

/// RAII guard for raw terminal mode.
/// Enables raw mode on creation, restores on drop.
struct RawModeGuard {
    was_enabled: bool,
}

impl RawModeGuard {
    fn new() -> Self {
        let was_enabled = terminal::enable_raw_mode().is_ok();
        Self { was_enabled }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.was_enabled {
            let _ = terminal::disable_raw_mode();
        }
    }
}

/// Output from a command execution.
#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Docker connector for executing commands in containers.
pub struct DockerConnector {
    client: Docker,
    container_id: String,
    user: String,
}

impl DockerConnector {
    /// Create a new Docker connector for the specified container.
    pub fn new(container_id: &str) -> Result<Self> {
        Self::with_user(container_id, "root")
    }

    /// Create a new Docker connector with a specific user.
    pub fn with_user(container_id: &str, user: &str) -> Result<Self> {
        let client = Docker::connect_with_socket_defaults().map_err(|e| {
            SpuffError::Provider(format!("Failed to connect to Docker socket: {}", e))
        })?;

        Ok(Self {
            client,
            container_id: container_id.to_string(),
            user: user.to_string(),
        })
    }

    /// Check if the container is running and ready.
    pub async fn wait_ready(&self) -> Result<()> {
        match self
            .client
            .inspect_container(&self.container_id, None)
            .await
        {
            Ok(info) => {
                let running = info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
                if running {
                    Ok(())
                } else {
                    Err(SpuffError::Provider("Container is not running".to_string()))
                }
            }
            Err(e) => Err(SpuffError::Provider(format!(
                "Failed to inspect container: {}",
                e
            ))),
        }
    }

    /// Execute a non-interactive command and return the output.
    pub async fn exec(&self, cmd: &str) -> Result<CommandOutput> {
        let exec = self
            .client
            .create_exec(
                &self.container_id,
                CreateExecOptions {
                    cmd: Some(vec!["sh", "-c", cmd]),
                    user: Some(&self.user),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to create exec: {}", e)))?;

        let output = self
            .client
            .start_exec(&exec.id, None::<StartExecOptions>)
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to start exec: {}", e)))?;

        let (stdout, stderr) = match output {
            StartExecResults::Attached { mut output, .. } => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();

                while let Some(chunk) = output.next().await {
                    match chunk {
                        Ok(bollard::container::LogOutput::StdOut { message }) => {
                            stdout.extend(message);
                        }
                        Ok(bollard::container::LogOutput::StdErr { message }) => {
                            stderr.extend(message);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("Error reading exec output: {}", e);
                        }
                    }
                }

                (
                    String::from_utf8_lossy(&stdout).to_string(),
                    String::from_utf8_lossy(&stderr).to_string(),
                )
            }
            StartExecResults::Detached => {
                return Err(SpuffError::Provider("Unexpected detached exec".to_string()));
            }
        };

        // Get exit code to determine success
        let inspect = self
            .client
            .inspect_exec(&exec.id)
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to inspect exec: {}", e)))?;

        let success = inspect.exit_code.unwrap_or(0) == 0;

        Ok(CommandOutput {
            stdout,
            stderr,
            success,
        })
    }

    /// Execute an interactive command with TTY support.
    pub async fn exec_interactive(&self, cmd: &str) -> Result<i32> {
        use std::io::Write as _;

        // Set terminal to raw mode FIRST, before anything else
        let _raw_guard = RawModeGuard::new();

        // Get terminal size before creating exec
        let (term_width, term_height) = term_size::dimensions().unwrap_or((80, 24));

        let exec = self
            .client
            .create_exec(
                &self.container_id,
                CreateExecOptions {
                    cmd: Some(vec!["sh", "-c", cmd]),
                    user: Some(&self.user),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    tty: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to create exec: {}", e)))?;

        let exec_id = exec.id.clone();
        let client = self.client.clone();

        let output = self
            .client
            .start_exec(
                &exec_id,
                Some(StartExecOptions {
                    detach: false,
                    tty: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to start exec: {}", e)))?;

        match output {
            StartExecResults::Attached {
                mut output,
                mut input,
            } => {
                // Resize immediately after exec starts
                let _ = client
                    .resize_exec(
                        &exec_id,
                        ResizeExecOptions {
                            width: term_width as u16,
                            height: term_height as u16,
                        },
                    )
                    .await;

                // Small delay to let resize take effect
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                // Create event stream for keyboard input (must be after raw mode)
                let mut event_stream = EventStream::new();

                // Create channel to signal shutdown
                let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

                // Task to read from container and write to stdout
                let output_handle = tokio::spawn(async move {
                    while let Some(chunk) = output.next().await {
                        match chunk {
                            Ok(bollard::container::LogOutput::StdOut { message })
                            | Ok(bollard::container::LogOutput::StdErr { message })
                            | Ok(bollard::container::LogOutput::Console { message }) => {
                                // Use sync write + flush for lower latency with TUI apps
                                let mut stdout = std::io::stdout();
                                if stdout.write_all(&message).is_err() {
                                    break;
                                }
                                let _ = stdout.flush();
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    let _ = shutdown_tx.send(true);
                });

                // Main loop: read keyboard events and send to container
                loop {
                    tokio::select! {
                        biased;

                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        maybe_event = event_stream.next() => {
                            match maybe_event {
                                Some(Ok(Event::Key(key_event))) => {
                                    let bytes = key_event_to_bytes(key_event);
                                    if !bytes.is_empty() {
                                        if input.write_all(&bytes).await.is_err() {
                                            break;
                                        }
                                        let _ = input.flush().await;
                                    }
                                }
                                Some(Ok(Event::Paste(text))) => {
                                    if input.write_all(text.as_bytes()).await.is_err() {
                                        break;
                                    }
                                    let _ = input.flush().await;
                                }
                                Some(Ok(Event::Resize(width, height))) => {
                                    // Handle terminal resize
                                    let _ = client
                                        .resize_exec(
                                            &exec_id,
                                            ResizeExecOptions {
                                                width,
                                                height,
                                            },
                                        )
                                        .await;
                                }
                                Some(Err(_)) | None => break,
                                _ => {}
                            }
                        }
                    }
                }

                // Wait for output task to finish
                let _ = output_handle.await;
            }
            StartExecResults::Detached => {
                return Err(SpuffError::Provider("Unexpected detached exec".to_string()));
            }
        }

        // Get exit code
        let inspect = self
            .client
            .inspect_exec(&exec_id)
            .await
            .map_err(|e| SpuffError::Provider(format!("Failed to inspect exec: {}", e)))?;

        Ok(inspect.exit_code.unwrap_or(0) as i32)
    }

    /// Open an interactive shell in the container.
    pub async fn shell(&self) -> Result<()> {
        self.exec_interactive("/bin/bash || /bin/sh").await?;
        Ok(())
    }
}

/// Run a command in a Docker container (non-interactive).
///
/// This is the Docker equivalent of `connector::ssh::run_command`.
pub async fn run_command(container_id: &str, command: &str) -> Result<String> {
    let connector = DockerConnector::new(container_id)?;
    let output = connector.exec(command).await?;

    if !output.success {
        return Err(SpuffError::Provider(format!(
            "Command failed: {}",
            output.stderr
        )));
    }

    Ok(output.stdout)
}

/// Connect to a Docker container with interactive shell.
///
/// This is the Docker equivalent of `connector::ssh::connect`.
pub async fn connect(container_id: &str) -> Result<()> {
    let connector = DockerConnector::new(container_id)?;
    connector.shell().await
}

/// Execute an interactive command in a Docker container.
///
/// This is the Docker equivalent of `connector::ssh::exec_interactive`.
pub async fn exec_interactive(container_id: &str, command: &str) -> Result<i32> {
    let connector = DockerConnector::new(container_id)?;
    connector.exec_interactive(command).await
}

/// Wait for a Docker container to be ready.
///
/// This is the Docker equivalent of `connector::ssh::wait_for_ssh`.
/// For Docker, this just verifies the container is running.
pub async fn wait_for_ready(container_id: &str) -> Result<()> {
    let connector = DockerConnector::new(container_id)?;
    connector.wait_ready().await
}

/// Convert a crossterm key event to bytes to send to the terminal.
fn key_event_to_bytes(event: crossterm::event::KeyEvent) -> Vec<u8> {
    use crossterm::event::KeyCode::*;

    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        Char(c) => {
            if ctrl {
                // Ctrl+letter produces ASCII control codes (1-26)
                let code = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a' - 1);
                vec![code]
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.as_bytes().to_vec()
            }
        }
        Enter => vec![b'\r'],
        Tab => vec![b'\t'],
        Backspace => vec![0x7f], // DEL
        Esc => vec![0x1b],
        Up => vec![0x1b, b'[', b'A'],
        Down => vec![0x1b, b'[', b'B'],
        Right => vec![0x1b, b'[', b'C'],
        Left => vec![0x1b, b'[', b'D'],
        Home => vec![0x1b, b'[', b'H'],
        End => vec![0x1b, b'[', b'F'],
        PageUp => vec![0x1b, b'[', b'5', b'~'],
        PageDown => vec![0x1b, b'[', b'6', b'~'],
        Delete => vec![0x1b, b'[', b'3', b'~'],
        Insert => vec![0x1b, b'[', b'2', b'~'],
        F(n) => {
            // F1-F4 use different codes than F5-F12
            match n {
                1 => vec![0x1b, b'O', b'P'],
                2 => vec![0x1b, b'O', b'Q'],
                3 => vec![0x1b, b'O', b'R'],
                4 => vec![0x1b, b'O', b'S'],
                5 => vec![0x1b, b'[', b'1', b'5', b'~'],
                6 => vec![0x1b, b'[', b'1', b'7', b'~'],
                7 => vec![0x1b, b'[', b'1', b'8', b'~'],
                8 => vec![0x1b, b'[', b'1', b'9', b'~'],
                9 => vec![0x1b, b'[', b'2', b'0', b'~'],
                10 => vec![0x1b, b'[', b'2', b'1', b'~'],
                11 => vec![0x1b, b'[', b'2', b'3', b'~'],
                12 => vec![0x1b, b'[', b'2', b'4', b'~'],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState};

    fn make_key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_command_output() {
        let output = CommandOutput {
            stdout: "hello".to_string(),
            stderr: "".to_string(),
            success: true,
        };

        assert!(output.success);
        assert_eq!(output.stdout, "hello");
    }

    #[test]
    fn test_command_output_failure() {
        let output = CommandOutput {
            stdout: "".to_string(),
            stderr: "error".to_string(),
            success: false,
        };

        assert!(!output.success);
        assert_eq!(output.stderr, "error");
    }

    #[test]
    fn test_key_event_to_bytes_char() {
        let event = make_key_event(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![b'a']);

        let event = make_key_event(KeyCode::Char('Z'), KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![b'Z']);
    }

    #[test]
    fn test_key_event_to_bytes_ctrl() {
        // Ctrl+C = ASCII 3
        let event = make_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_event_to_bytes(event), vec![3]);

        // Ctrl+D = ASCII 4
        let event = make_key_event(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(key_event_to_bytes(event), vec![4]);

        // Ctrl+Z = ASCII 26
        let event = make_key_event(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(key_event_to_bytes(event), vec![26]);
    }

    #[test]
    fn test_key_event_to_bytes_special() {
        let event = make_key_event(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![b'\r']);

        let event = make_key_event(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![b'\t']);

        let event = make_key_event(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x7f]);

        let event = make_key_event(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b]);
    }

    #[test]
    fn test_key_event_to_bytes_arrows() {
        let event = make_key_event(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'A']);

        let event = make_key_event(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'B']);

        let event = make_key_event(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'C']);

        let event = make_key_event(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'D']);
    }

    #[test]
    fn test_key_event_to_bytes_function_keys() {
        let event = make_key_event(KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'O', b'P']);

        let event = make_key_event(KeyCode::F(5), KeyModifiers::NONE);
        assert_eq!(
            key_event_to_bytes(event),
            vec![0x1b, b'[', b'1', b'5', b'~']
        );

        let event = make_key_event(KeyCode::F(12), KeyModifiers::NONE);
        assert_eq!(
            key_event_to_bytes(event),
            vec![0x1b, b'[', b'2', b'4', b'~']
        );
    }

    #[test]
    fn test_key_event_to_bytes_navigation() {
        let event = make_key_event(KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'H']);

        let event = make_key_event(KeyCode::End, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'F']);

        let event = make_key_event(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'5', b'~']);

        let event = make_key_event(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'6', b'~']);

        let event = make_key_event(KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'3', b'~']);

        let event = make_key_event(KeyCode::Insert, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(event), vec![0x1b, b'[', b'2', b'~']);
    }

    #[test]
    fn test_key_event_to_bytes_unicode() {
        // Test UTF-8 characters
        let event = make_key_event(KeyCode::Char('é'), KeyModifiers::NONE);
        let bytes = key_event_to_bytes(event);
        assert_eq!(bytes, "é".as_bytes());

        let event = make_key_event(KeyCode::Char('中'), KeyModifiers::NONE);
        let bytes = key_event_to_bytes(event);
        assert_eq!(bytes, "中".as_bytes());
    }
}
