//! SSH command implementation using pure Rust.
//!
//! This module provides SSH connectivity with optional port forwarding,
//! using pure Rust libraries instead of external SSH binaries.

use std::path::PathBuf;

use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::project_config::ProjectConfig;
use crate::ssh::{SshClient, SshConfig};
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

/// Convert AppConfig to SshConfig for SSH operations.
fn app_config_to_ssh_config(config: &AppConfig) -> SshConfig {
    SshConfig {
        user: config.ssh_user.clone(),
        key_path: PathBuf::from(&config.ssh_key_path),
        host_key_policy: crate::ssh::config::HostKeyPolicy::AcceptNew,
    }
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

/// Connect to remote host with SSH tunnels using pure Rust.
///
/// This starts the SSH connection with port forwarding and an interactive shell.
async fn connect_with_tunnels(host: &str, config: &AppConfig, ports: &[u16]) -> Result<()> {
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(host, 22, &ssh_config).await?;

    // Create port forwards
    let _forwards = client.forward_ports(ports).await?;

    // Start interactive shell (forwards remain active during shell session)
    client.shell().await
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

    // Create SSH connection and tunnels using pure Rust
    let ssh_config = app_config_to_ssh_config(config);
    let client = SshClient::connect(&instance.ip, 22, &ssh_config).await?;

    // Create port forwards
    let forwards = client.forward_ports(&ports).await?;

    // Save state so we can stop later
    save_tunnel_state(&ports)?;

    println!(
        "  {} Tunnels active (pure Rust, no external processes)",
        style("✓").green().bold()
    );
    println!();
    println!(
        "  {} {}",
        style("Stop:").dim(),
        style("Ctrl+C or spuff tunnel --stop").cyan()
    );
    println!();

    // Keep running until interrupted
    println!(
        "  {} Tunnels running. Press Ctrl+C to stop...",
        style("→").dim()
    );

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to wait for signal: {}", e)))?;

    // Stop forwards gracefully
    for forward in &forwards {
        forward.stop().await;
    }

    // Clean up state file
    let _ = std::fs::remove_file(get_tunnel_state_file()?);

    println!();
    println!(
        "  {} Tunnels stopped.",
        style("✓").green().bold()
    );

    Ok(())
}

/// Stop background tunnels
async fn stop_tunnels() -> Result<()> {
    let state_file = get_tunnel_state_file()?;

    if !state_file.exists() {
        println!(
            "  {} No active tunnel state found.",
            style("○").dim()
        );
        println!(
            "  {} If tunnels are running in another terminal, press Ctrl+C there.",
            style("i").blue()
        );
        return Ok(());
    }

    // Remove state file to indicate stop
    let _ = std::fs::remove_file(&state_file);

    println!(
        "  {} Tunnel state cleared. If tunnels are running, they will continue",
        style("✓").green().bold()
    );
    println!(
        "  {} until you press Ctrl+C in that terminal.",
        style("i").blue()
    );

    Ok(())
}

fn get_tunnel_state_file() -> Result<std::path::PathBuf> {
    let config_dir = crate::config::AppConfig::config_dir()?;
    Ok(config_dir.join("tunnel.state"))
}

fn save_tunnel_state(ports: &[u16]) -> Result<()> {
    let state_file = get_tunnel_state_file()?;
    let content = ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(&state_file, content)?;
    Ok(())
}
