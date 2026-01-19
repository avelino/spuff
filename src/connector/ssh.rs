use std::process::Stdio;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::process::Command;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};

pub async fn wait_for_ssh(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let addr = format!("{}:{}", host, port);

    // Wait for TCP port to be open
    while start.elapsed() < timeout {
        match TcpStream::connect(&addr).await {
            Ok(_) => break,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    if start.elapsed() >= timeout {
        return Err(SpuffError::Ssh(format!(
            "Timeout waiting for SSH port on {}",
            addr
        )));
    }

    Ok(())
}

/// Wait for SSH login to actually work (user exists and key is configured)
/// If BatchMode fails due to passphrase, prompts user interactively
pub async fn wait_for_ssh_login(host: &str, config: &AppConfig, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let mut prompted_for_passphrase = false;

    while start.elapsed() < timeout {
        // Try to run a simple command with BatchMode (no interactive prompts)
        let result = Command::new("ssh")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null")
            .arg("-o")
            .arg("LogLevel=ERROR")
            .arg("-o")
            .arg("ConnectTimeout=5")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-i")
            .arg(&config.ssh_key_path)
            .arg(format!("{}@{}", config.ssh_user, host))
            .arg("echo ok")
            .output()
            .await;

        if let Ok(output) = result {
            if output.status.success() {
                return Ok(());
            }

            // Check if it's a permission/auth issue (not just "user doesn't exist yet")
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !prompted_for_passphrase
                && (stderr.contains("Permission denied")
                    || stderr.contains("passphrase")
                    || stderr.contains("Authentication failed"))
            {
                prompted_for_passphrase = true;

                // Try interactive SSH to allow passphrase entry
                eprintln!("\n🔑 SSH key may need passphrase. Trying interactive connection...\n");

                let interactive_result = Command::new("ssh")
                    .arg("-o")
                    .arg("StrictHostKeyChecking=accept-new")
                    .arg("-o")
                    .arg("UserKnownHostsFile=/dev/null")
                    .arg("-o")
                    .arg("LogLevel=ERROR")
                    .arg("-o")
                    .arg("ConnectTimeout=10")
                    .arg("-i")
                    .arg(&config.ssh_key_path)
                    .arg(format!("{}@{}", config.ssh_user, host))
                    .arg("echo ok")
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await;

                if let Ok(status) = interactive_result {
                    if status.success() {
                        eprintln!();
                        return Ok(());
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Err(SpuffError::Ssh(format!(
        "Timeout waiting for SSH login as {}@{}. Make sure 'ssh-add' was run if your key has a passphrase.",
        config.ssh_user, host
    )))
}

pub async fn connect(host: &str, config: &AppConfig) -> Result<()> {
    let mut cmd = Command::new("ssh");

    cmd.arg("-A") // Enable SSH agent forwarding for git operations
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-i")
        .arg(&config.ssh_key_path)
        .arg(format!("{}@{}", config.ssh_user, host));

    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;

    if !status.success() {
        return Err(SpuffError::Ssh(format!(
            "SSH connection failed with code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Execute a command interactively with TTY support
pub async fn exec_interactive(host: &str, config: &AppConfig, command: &str) -> Result<i32> {
    let mut cmd = Command::new("ssh");

    cmd.arg("-t") // Force pseudo-terminal allocation for interactive commands
        .arg("-A") // Enable SSH agent forwarding
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-i")
        .arg(&config.ssh_key_path)
        .arg(format!("{}@{}", config.ssh_user, host))
        .arg(command);

    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;

    Ok(status.code().unwrap_or(1))
}

/// Upload a file to the remote host via SCP
pub async fn scp_upload(
    host: &str,
    config: &AppConfig,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    let output = Command::new("scp")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("BatchMode=yes") // Fail fast if key needs passphrase
        .arg("-i")
        .arg(&config.ssh_key_path)
        .arg(local_path)
        .arg(format!("{}@{}:{}", config.ssh_user, host, remote_path))
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check for passphrase-related errors
        if stderr.contains("Permission denied") || stderr.contains("passphrase") {
            return Err(SpuffError::Ssh(
                "SSH key requires passphrase. Run 'ssh-add' first to add your key to the agent.".to_string()
            ));
        }
        return Err(SpuffError::Ssh(format!("SCP upload failed: {}", stderr)));
    }

    Ok(())
}

pub async fn run_command(host: &str, config: &AppConfig, command: &str) -> Result<String> {
    // Wrap command in bash --norc to avoid loading .bashrc which may print banners
    let wrapped_command = format!("bash --norc --noprofile -c '{}'", command.replace('\'', "'\\''"));

    let output = Command::new("ssh")
        .arg("-T") // Disable pseudo-terminal allocation
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("BatchMode=yes") // Fail fast if key needs passphrase
        .arg("-i")
        .arg(&config.ssh_key_path)
        .arg(format!("{}@{}", config.ssh_user, host))
        .arg(&wrapped_command)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check for passphrase-related errors
        if stderr.contains("Permission denied") || stderr.contains("passphrase") {
            return Err(SpuffError::Ssh(
                "SSH key requires passphrase. Run 'ssh-add' first to add your key to the agent.".to_string()
            ));
        }
        return Err(SpuffError::Ssh(format!("Command failed: {}", stderr)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_for_ssh_timeout() {
        // Test against localhost on a port that's unlikely to be listening
        let result = wait_for_ssh("127.0.0.1", 59999, Duration::from_millis(100)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Timeout waiting for SSH"));
    }

    #[tokio::test]
    async fn test_wait_for_ssh_success() {
        // Start a TCP listener to simulate SSH port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a task to accept one connection
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let result = wait_for_ssh("127.0.0.1", port, Duration::from_secs(5)).await;
        assert!(result.is_ok());
    }
}
