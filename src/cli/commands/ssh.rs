use std::process::Stdio;

use console::style;
use tokio::process::Command;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::state::StateDb;

const BANNER: &str = r#"
╔═══════════════════════════════╗
║  s p u f f                    ║
║  ephemeral dev env            ║
╚═══════════════════════════════╝
"#;

pub fn print_banner() {
    println!("{}", style(BANNER).cyan());
}

pub async fn execute(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    print_banner();
    println!(
        "  {} {} {}",
        style("→").bold(),
        style(&instance.name).white().bold(),
        style(format!("({})", &instance.ip)).dim()
    );
    println!();

    // Check if there's a project config with ports
    let ports = get_project_ports();

    if !ports.is_empty() {
        print_tunnel_info(&ports);
        connect_with_tunnels(&instance.ip, config, &ports).await
    } else {
        crate::connector::ssh::connect(&instance.ip, config).await
    }
}

/// Get ports from spuff.yaml if it exists
fn get_project_ports() -> Vec<u16> {
    ProjectConfig::load_from_cwd()
        .ok()
        .flatten()
        .map(|c| c.ports)
        .unwrap_or_default()
}

/// Print tunnel information box
fn print_tunnel_info(ports: &[u16]) {
    if ports.is_empty() {
        return;
    }

    println!("  {}", style("╭──────────────────────────────────────────────────────────╮").dim());
    println!("  {}  {:<56} {}", style("│").dim(), style("SSH Tunnels (from spuff.yaml)").cyan(), style("│").dim());

    for port in ports {
        let tunnel_str = format!("localhost:{} → vm:{}", port, port);
        println!("  {}  {:<56} {}", style("│").dim(), tunnel_str, style("│").dim());
    }

    println!("  {}", style("╰──────────────────────────────────────────────────────────╯").dim());
    println!();
}

/// Connect to remote host with SSH tunnels
async fn connect_with_tunnels(host: &str, config: &AppConfig, ports: &[u16]) -> Result<()> {
    let mut cmd = Command::new("ssh");

    // Enable SSH agent forwarding
    cmd.arg("-A");

    // Add tunnel arguments for each port
    for port in ports {
        cmd.arg("-L").arg(format!("{}:localhost:{}", port, port));
    }

    // Standard SSH options
    cmd.arg("-o")
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

/// Create SSH tunnels in background (without shell)
pub async fn tunnel(config: &AppConfig, specific_port: Option<u16>, stop: bool) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    if stop {
        return stop_tunnels().await;
    }

    // Get ports from project config or use specific port
    let ports: Vec<u16> = if let Some(port) = specific_port {
        vec![port]
    } else {
        let project_ports = get_project_ports();
        if project_ports.is_empty() {
            return Err(SpuffError::Config(
                "No ports configured in spuff.yaml. Use --port to specify a port.".to_string(),
            ));
        }
        project_ports
    };

    print_banner();
    println!(
        "  {} Creating tunnels to {} {}",
        style("→").bold(),
        style(&instance.name).white().bold(),
        style(format!("({})", &instance.ip)).dim()
    );
    println!();

    print_tunnel_info(&ports);

    // Create background SSH process with tunnels
    let mut cmd = Command::new("ssh");

    // No shell, just tunnels
    cmd.arg("-N");

    // Keep connection alive
    cmd.arg("-o").arg("ServerAliveInterval=60");
    cmd.arg("-o").arg("ServerAliveCountMax=3");

    // Add tunnel arguments for each port
    for port in &ports {
        cmd.arg("-L").arg(format!("{}:localhost:{}", port, port));
    }

    // Standard SSH options
    cmd.arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-i")
        .arg(&config.ssh_key_path)
        .arg(format!("{}@{}", config.ssh_user, instance.ip));

    // Run in background
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Save PID for later
    let pid = child.id().unwrap_or(0);
    save_tunnel_pid(pid)?;

    println!(
        "  {} Tunnels running in background (PID: {})",
        style("✓").green().bold(),
        pid
    );
    println!();
    println!(
        "  {} {}",
        style("Stop:").dim(),
        style("spuff tunnel --stop").cyan()
    );
    println!();

    Ok(())
}

/// Stop background tunnels
async fn stop_tunnels() -> Result<()> {
    let pid_file = get_tunnel_pid_file()?;

    if !pid_file.exists() {
        println!(
            "  {} No active tunnels found.",
            style("○").dim()
        );
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_file)?;
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);

    if pid > 0 {
        // Try to kill the process
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }

        println!(
            "  {} Stopped tunnel process (PID: {})",
            style("✓").green().bold(),
            pid
        );
    }

    // Remove PID file
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

fn get_tunnel_pid_file() -> Result<std::path::PathBuf> {
    let config_dir = crate::config::AppConfig::config_dir()?;
    Ok(config_dir.join("tunnel.pid"))
}

fn save_tunnel_pid(pid: u32) -> Result<()> {
    let pid_file = get_tunnel_pid_file()?;
    std::fs::write(&pid_file, pid.to_string())?;
    Ok(())
}
