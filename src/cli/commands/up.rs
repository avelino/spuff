use std::process::Stdio;
use std::time::Duration;

use console::style;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::connector::ssh::{wait_for_ssh, wait_for_ssh_login};
use crate::environment::cloud_init::generate_cloud_init;
use crate::error::{Result, SpuffError};
use crate::provider::{create_provider, ImageSpec, InstanceRequest};
use crate::state::{LocalInstance, StateDb};
use crate::tui::{run_progress_ui, ProgressMessage, StepState};

const STEP_CLOUD_INIT: usize = 0;
const STEP_CREATE: usize = 1;
const STEP_WAIT_READY: usize = 2;
const STEP_WAIT_SSH: usize = 3;
const STEP_BOOTSTRAP: usize = 4;

// Bootstrap sub-steps (indices) - minimal bootstrap, devtools installed via agent
const SUB_PACKAGES: usize = 0;
const SUB_AGENT: usize = 1;

pub async fn execute(
    config: &AppConfig,
    size: Option<String>,
    snapshot: Option<String>,
    region: Option<String>,
    no_connect: bool,
    dev: bool,
) -> Result<()> {
    let db = StateDb::open()?;

    if let Some(instance) = db.get_active_instance()? {
        println!(
            "{} Active instance found: {} ({})",
            style("!").yellow().bold(),
            style(&instance.name).cyan(),
            style(&instance.ip).dim()
        );
        println!(
            "Run {} first or {} to connect.",
            style("spuff down").yellow(),
            style("spuff ssh").cyan()
        );
        return Ok(());
    }

    // Pre-flight check: verify SSH key is usable
    verify_ssh_key_accessible(config).await?;

    // In dev mode, build agent for Linux and upload
    if dev {
        println!(
            "{} Dev mode: building spuff-agent for Linux ({})",
            style("*").cyan().bold(),
            style(LINUX_TARGET).dim()
        );

        match build_linux_agent().await {
            Ok(path) => {
                println!(
                    "{} Built successfully: {}",
                    style("✓").green().bold(),
                    style(&path).dim()
                );
            }
            Err(e) => {
                println!(
                    "{} Failed to build agent: {}",
                    style("✕").red().bold(),
                    e
                );
                return Err(e);
            }
        }
    }

    // Create channel for progress updates
    let (tx, rx) = mpsc::channel::<ProgressMessage>(32);

    // Define the steps
    let mut steps = vec![
        "Generating cloud-init config".to_string(),
        "Creating instance".to_string(),
        "Waiting for instance".to_string(),
        "Waiting for SSH".to_string(),
        "Running bootstrap".to_string(),
    ];

    if dev {
        steps.push("Uploading local agent".to_string());
    }

    // Clone config values for the async task
    let config_clone = config.clone();
    let region_clone = region.clone();
    let size_clone = size.clone();
    let snapshot_clone = snapshot.clone();

    // Spawn the provisioning task
    let provision_task = tokio::spawn(async move {
        provision_instance(
            &config_clone,
            size_clone,
            snapshot_clone,
            region_clone,
            dev,
            tx,
        )
        .await
    });

    // Run the TUI
    let tui_result = run_progress_ui(steps, rx).await;

    // Wait for the provisioning task
    let provision_result = provision_task.await;

    // Handle results
    match (tui_result, provision_result) {
        (Ok(Some((name, ip))), Ok(Ok(()))) => {
            // Success - print final message
            super::ssh::print_banner();
            println!(
                "  {} {} {}",
                style("✓").green().bold(),
                style(&name).white().bold(),
                style(format!("({})", &ip)).dim()
            );
            println!();
            println!("  {}  ssh {}@{}", style("SSH").dim(), config.ssh_user, ip);
            println!(
                "  {}  auto-destroy after {}",
                style("Idle").dim(),
                style(&config.idle_timeout).yellow()
            );

            if !no_connect {
                println!();
                crate::connector::ssh::connect(&ip, config).await?;
            }
        }
        (Ok(None), _) => {
            // User cancelled
            println!("{} Cancelled by user.", style("!").yellow().bold());
        }
        (Err(e), _) => {
            println!("{} TUI error: {}", style("✕").red().bold(), e);
        }
        (_, Ok(Err(e))) => {
            println!("{} Provisioning failed: {}", style("✕").red().bold(), e);
        }
        (_, Err(e)) => {
            println!("{} Task error: {}", style("✕").red().bold(), e);
        }
    }

    Ok(())
}

const STEP_UPLOAD_AGENT: usize = 5;

/// Linux x86_64 target for cross-compilation (musl for static linking)
const LINUX_TARGET: &str = "x86_64-unknown-linux-musl";

fn get_linux_agent_path() -> String {
    format!("target/{}/release/spuff-agent", LINUX_TARGET)
}

/// Get the cargo binary path, checking common locations
fn get_cargo_path() -> String {
    // Check if cargo is in PATH
    if let Ok(path) = which::which("cargo") {
        return path.to_string_lossy().to_string();
    }

    // Check common locations
    if let Ok(home) = std::env::var("HOME") {
        let cargo_home = format!("{}/.cargo/bin/cargo", home);
        if std::path::Path::new(&cargo_home).exists() {
            return cargo_home;
        }
    }

    // Fallback to just "cargo" and let it fail if not found
    "cargo".to_string()
}

/// Check if cargo-zigbuild is installed
async fn has_cargo_zigbuild() -> bool {
    let cargo = get_cargo_path();
    // cargo zigbuild doesn't have --version, use --help instead
    tokio::process::Command::new(&cargo)
        .args(["zigbuild", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if cross is installed
async fn has_cross() -> bool {
    // Check common location
    if let Ok(home) = std::env::var("HOME") {
        let cross_path = format!("{}/.cargo/bin/cross", home);
        if std::path::Path::new(&cross_path).exists() {
            return tokio::process::Command::new(&cross_path)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false);
        }
    }

    // Try PATH
    tokio::process::Command::new("cross")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build the agent for Linux x86_64 using cross-compilation
async fn build_linux_agent() -> Result<String> {
    let agent_path = get_linux_agent_path();
    let cargo = get_cargo_path();

    // Determine which build tool to use (in order of preference)
    let (cmd, args): (String, Vec<&str>) = if has_cargo_zigbuild().await {
        // cargo-zigbuild: best option for macOS, no Docker needed
        (cargo.clone(), vec!["zigbuild", "--release", "--bin", "spuff-agent", "--target", LINUX_TARGET])
    } else if has_cross().await {
        // cross: requires Docker but works well
        let cross_path = if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.cargo/bin/cross", home);
            if std::path::Path::new(&path).exists() { path } else { "cross".to_string() }
        } else {
            "cross".to_string()
        };
        (cross_path, vec!["build", "--release", "--bin", "spuff-agent", "--target", LINUX_TARGET])
    } else {
        // Fallback to cargo (requires linker setup)
        (cargo.clone(), vec!["build", "--release", "--bin", "spuff-agent", "--target", LINUX_TARGET])
    };

    println!(
        "  {} {} {}",
        console::style("→").cyan(),
        console::style(&cmd).white(),
        console::style(args.join(" ")).dim()
    );

    let status = tokio::process::Command::new(&cmd)
        .args(&args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;

    if !status.success() {
        return Err(crate::error::SpuffError::Build(format!(
            "Failed to build spuff-agent for {}.\n\
             Install cross-compilation support:\n  \
             cargo install cargo-zigbuild && brew install zig\n  \
             Or: cargo install cross (requires Docker)",
            LINUX_TARGET
        )));
    }

    if !std::path::Path::new(&agent_path).exists() {
        return Err(crate::error::SpuffError::Build(format!(
            "Build succeeded but binary not found at {}",
            agent_path
        )));
    }

    Ok(agent_path)
}

async fn provision_instance(
    config: &AppConfig,
    size: Option<String>,
    snapshot: Option<String>,
    region: Option<String>,
    dev: bool,
    tx: mpsc::Sender<ProgressMessage>,
) -> Result<()> {
    let db = StateDb::open()?;
    let provider = create_provider(config)?;

    // Step 1: Generate cloud-init
    tx.send(ProgressMessage::SetStep(STEP_CLOUD_INIT, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail("Preparing environment configuration...".to_string()))
        .await
        .ok();

    let user_data = generate_cloud_init(config)?;
    tx.send(ProgressMessage::SetStep(STEP_CLOUD_INIT, StepState::Done))
        .await
        .ok();

    // Step 2: Create instance
    tx.send(ProgressMessage::SetStep(STEP_CREATE, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail("Requesting VM from provider...".to_string()))
        .await
        .ok();

    let instance_name = generate_instance_name();
    let instance_region = region.unwrap_or_else(|| config.region.clone());
    let instance_size = size.unwrap_or_else(|| config.size.clone());
    let image = get_image_spec(&config.provider, snapshot);

    let request = InstanceRequest::new(instance_name.clone(), instance_region.clone(), instance_size.clone())
        .with_image(image)
        .with_user_data(user_data)
        .with_label("spuff", "true")
        .with_label("managed-by", "spuff-cli");

    let instance = match provider.create_instance(&request).await {
        Ok(i) => i,
        Err(e) => {
            tx.send(ProgressMessage::SetStep(STEP_CREATE, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
            return Err(e.into());
        }
    };

    tx.send(ProgressMessage::SetStep(STEP_CREATE, StepState::Done))
        .await
        .ok();

    // Step 3: Wait for instance to be ready
    tx.send(ProgressMessage::SetStep(STEP_WAIT_READY, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail("Waiting for VM to be assigned an IP...".to_string()))
        .await
        .ok();

    let instance = match provider.wait_ready(&instance.id).await {
        Ok(i) => i,
        Err(e) => {
            tx.send(ProgressMessage::SetStep(STEP_WAIT_READY, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
            return Err(e.into());
        }
    };

    // Save instance early so we don't lose track if something fails
    let local_instance = LocalInstance::from_provider(
        &instance,
        instance_name.clone(),
        config.provider.clone(),
        instance_region.clone(),
        instance_size.clone(),
    );
    db.save_instance(&local_instance)?;

    tx.send(ProgressMessage::SetStep(STEP_WAIT_READY, StepState::Done))
        .await
        .ok();

    // Step 4: Wait for SSH
    tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail(format!("Waiting for SSH port on {}...", instance.ip)))
        .await
        .ok();

    // First wait for SSH port to be open
    if let Err(e) = wait_for_ssh(&instance.ip.to_string(), 22, Duration::from_secs(300)).await {
        tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Failed))
            .await
            .ok();
        tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
        return Err(e);
    }

    // Then wait for user to exist and SSH login to work
    tx.send(ProgressMessage::SetDetail(format!("Waiting for user {}...", config.ssh_user)))
        .await
        .ok();

    if let Err(e) = wait_for_ssh_login(&instance.ip.to_string(), config, Duration::from_secs(120)).await {
        tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Failed))
            .await
            .ok();
        tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
        return Err(e);
    }

    tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Done))
        .await
        .ok();

    // In dev mode, upload local agent binary
    if dev {
        tx.send(ProgressMessage::SetStep(STEP_UPLOAD_AGENT, StepState::InProgress))
            .await
            .ok();
        tx.send(ProgressMessage::SetDetail("Uploading local spuff-agent...".to_string()))
            .await
            .ok();

        let agent_path = get_linux_agent_path();
        let ip_str = instance.ip.to_string();

        // Upload the binary
        if let Err(e) = upload_local_agent(&ip_str, config, &agent_path).await {
            tx.send(ProgressMessage::SetStep(STEP_UPLOAD_AGENT, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::SetDetail(format!("Agent upload failed: {}", e)))
                .await
                .ok();
            // Don't fail the whole process, just warn
            tracing::warn!("Failed to upload local agent: {}", e);
        } else {
            tx.send(ProgressMessage::SetStep(STEP_UPLOAD_AGENT, StepState::Done))
                .await
                .ok();
        }
    }

    // Step 5/6: Wait for cloud-init with sub-steps
    tx.send(ProgressMessage::SetStep(STEP_BOOTSTRAP, StepState::InProgress))
        .await
        .ok();

    // Define sub-steps for bootstrap (minimal - devtools installed via agent)
    let sub_steps = vec![
        "Updating packages".to_string(),
        "Installing spuff-agent".to_string(),
    ];

    tx.send(ProgressMessage::SetSubSteps(STEP_BOOTSTRAP, sub_steps))
        .await
        .ok();

    // Wait for cloud-init with progress tracking
    if let Err(e) = wait_for_cloud_init_with_progress(&instance.ip.to_string(), config, &tx).await {
        tracing::warn!("Cloud-init wait failed: {}", e);
        // Don't fail the whole process, just warn
    }

    tx.send(ProgressMessage::SetStep(STEP_BOOTSTRAP, StepState::Done))
        .await
        .ok();

    // Trigger devtools installation via agent (async, non-blocking)
    tx.send(ProgressMessage::SetDetail("Triggering devtools installation...".to_string()))
        .await
        .ok();

    if let Err(e) = trigger_devtools_installation(&instance.ip.to_string(), config).await {
        tracing::warn!("Failed to trigger devtools installation: {}", e);
        // Don't fail - user can trigger manually with `spuff agent devtools install`
    }

    // Complete!
    tx.send(ProgressMessage::Complete(
        instance_name.clone(),
        instance.ip.to_string(),
    ))
    .await
    .ok();

    // Small delay to let the UI update
    tokio::time::sleep(Duration::from_millis(100)).await;

    tx.send(ProgressMessage::Close).await.ok();

    Ok(())
}

fn generate_instance_name() -> String {
    let id = &uuid::Uuid::new_v4().to_string()[..8];
    format!("spuff-{}", id)
}

/// Get the appropriate image specification for the instance.
///
/// If a snapshot ID is provided, uses that. Otherwise, defaults to Ubuntu 24.04.
fn get_image_spec(provider: &str, snapshot: Option<String>) -> ImageSpec {
    if let Some(snapshot_id) = snapshot {
        // User provided a snapshot ID
        return ImageSpec::snapshot(snapshot_id);
    }

    // Default to Ubuntu 24.04 for all providers
    match provider {
        "aws" => {
            // AWS uses AMI IDs - this is a placeholder, should come from config
            ImageSpec::custom("ami-0c55b159cbfafe1f0")
        }
        _ => {
            // Most providers support Ubuntu slug
            ImageSpec::ubuntu("24.04")
        }
    }
}

async fn wait_for_cloud_init_with_progress(
    ip: &str,
    config: &AppConfig,
    tx: &mpsc::Sender<ProgressMessage>,
) -> Result<()> {
    let max_attempts = 60; // 5 minutes max (faster now with minimal bootstrap)
    let delay = Duration::from_secs(5);

    // Track which sub-steps have been completed
    let mut packages_done = false;
    let mut agent_done = false;

    // Start first sub-step
    tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, SUB_PACKAGES, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail("Updating system packages...".to_string()))
        .await
        .ok();

    for _ in 0..max_attempts {
        // Check cloud-init log for progress
        if let Ok(log) = get_cloud_init_log(ip, config).await {
            // Check for package updates completion
            if !packages_done
                && (log.contains("apt-get update") || log.contains("package_update"))
                && (log.contains("Setting up") || log.contains("Unpacking"))
            {
                packages_done = true;
                tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, SUB_PACKAGES, StepState::Done))
                    .await
                    .ok();
                tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, SUB_AGENT, StepState::InProgress))
                    .await
                    .ok();
                tx.send(ProgressMessage::SetDetail("Installing spuff-agent...".to_string()))
                    .await
                    .ok();
            }

            // Check for spuff-agent installation
            if !agent_done
                && log.contains("spuff-agent")
                && (log.contains("systemctl enable spuff-agent") || log.contains("systemctl start spuff-agent"))
            {
                agent_done = true;
                tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, SUB_AGENT, StepState::Done))
                    .await
                    .ok();
                tx.send(ProgressMessage::SetDetail("Finalizing...".to_string()))
                    .await
                    .ok();
            }

            // Check if cloud-init is done
            if log.contains("spuff environment ready") || log.contains("final_message") {
                // Mark any remaining sub-steps as done
                for i in 0..2 {
                    tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, i, StepState::Done))
                        .await
                        .ok();
                }
                return Ok(());
            }
        }

        // Also check cloud-init status
        if let Ok(true) = check_cloud_init_status(ip, config).await {
            // Mark any remaining sub-steps as done
            for i in 0..2 {
                tx.send(ProgressMessage::SetSubStep(STEP_BOOTSTRAP, i, StepState::Done))
                    .await
                    .ok();
            }
            return Ok(());
        }

        tokio::time::sleep(delay).await;
    }

    Err(crate::error::SpuffError::CloudInit(
        "Timeout waiting for cloud-init".to_string(),
    ))
}

async fn get_cloud_init_log(ip: &str, config: &AppConfig) -> Result<String> {
    crate::connector::ssh::run_command(
        ip,
        config,
        "tail -200 /var/log/cloud-init-output.log 2>/dev/null || echo ''",
    )
    .await
}

async fn check_cloud_init_status(ip: &str, config: &AppConfig) -> Result<bool> {
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        "cloud-init status --format=json 2>/dev/null || echo '{\"status\": \"running\"}'",
    )
    .await?;

    Ok(output.contains("\"status\": \"done\"") || output.contains("\"status\":\"done\""))
}

/// Check if SSH key is loaded in the agent
async fn is_key_in_agent(key_path: &str) -> bool {
    // Get the key fingerprint
    let fp_result = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(key_path)
        .output()
        .await;

    let fingerprint = match fp_result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.split_whitespace().nth(1).map(|s| s.to_string())
        }
        Err(_) => None,
    };

    let Some(fp) = fingerprint else {
        return false;
    };

    // Check if fingerprint is in agent
    let agent_result = Command::new("ssh-add")
        .arg("-l")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await;

    match agent_result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains(&fp)
        }
        Err(_) => false,
    }
}

/// Check if SSH key has a passphrase
async fn key_has_passphrase(key_path: &str) -> bool {
    // Try to read the key with empty passphrase
    let result = Command::new("ssh-keygen")
        .arg("-y")
        .arg("-P")
        .arg("")
        .arg("-f")
        .arg(key_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    // If it fails, the key has a passphrase
    !matches!(result, Ok(status) if status.success())
}

/// Check if ssh-agent is running and accessible
async fn is_ssh_agent_running() -> bool {
    // Check if SSH_AUTH_SOCK is set and the socket exists
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        if std::path::Path::new(&sock).exists() {
            // Try to actually connect to it
            let result = Command::new("ssh-add")
                .arg("-l")
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .await;

            if let Ok(output) = result {
                // exit code 2 means agent not running, 1 means no keys but agent is running
                let stderr = String::from_utf8_lossy(&output.stderr);
                return !stderr.contains("Could not open") && !stderr.contains("Connection refused");
            }
        }
    }
    false
}

/// Start ssh-agent and set environment variables
async fn start_ssh_agent() -> Result<()> {
    // Start ssh-agent and capture its output
    let output = Command::new("ssh-agent")
        .arg("-s") // Bourne shell output format
        .output()
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to start ssh-agent: {}", e)))?;

    if !output.status.success() {
        return Err(SpuffError::Ssh("Failed to start ssh-agent".to_string()));
    }

    // Parse the output to get SSH_AUTH_SOCK and SSH_AGENT_PID
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("SSH_AUTH_SOCK=") {
            if let Some(value) = line.strip_prefix("SSH_AUTH_SOCK=") {
                let value = value.trim_end_matches("; export SSH_AUTH_SOCK;");
                std::env::set_var("SSH_AUTH_SOCK", value);
            }
        } else if line.starts_with("SSH_AGENT_PID=") {
            if let Some(value) = line.strip_prefix("SSH_AGENT_PID=") {
                let value = value.trim_end_matches("; export SSH_AGENT_PID;");
                std::env::set_var("SSH_AGENT_PID", value);
            }
        }
    }

    Ok(())
}

/// Verify SSH key is accessible, prompting for passphrase if needed
async fn verify_ssh_key_accessible(config: &AppConfig) -> Result<()> {
    // Check if key is already in agent
    if is_key_in_agent(&config.ssh_key_path).await {
        return Ok(());
    }

    // Check if key has passphrase
    if !key_has_passphrase(&config.ssh_key_path).await {
        // Key has no passphrase - good to go
        return Ok(());
    }

    // Key has passphrase and is not in agent - need to add it
    add_key_to_agent(config).await
}

/// Add SSH key to agent interactively
async fn add_key_to_agent(config: &AppConfig) -> Result<()> {
    // First, ensure ssh-agent is running
    if !is_ssh_agent_running().await {
        println!(
            "{} Starting ssh-agent...",
            style("→").cyan().bold()
        );
        start_ssh_agent().await?;
    }

    // Prompt user to add the key
    println!(
        "\n{} SSH key requires passphrase: {}",
        style("🔑").cyan(),
        style(&config.ssh_key_path).yellow()
    );
    println!(
        "{} Adding key to ssh-agent...\n",
        style("→").cyan().bold()
    );

    // On macOS, use Keychain to persist the key across sessions
    let is_macos = cfg!(target_os = "macos");

    // Run ssh-add interactively so user can enter passphrase
    let mut cmd = Command::new("ssh-add");
    if is_macos {
        cmd.arg("--apple-use-keychain"); // Store passphrase in macOS Keychain
    }
    cmd.arg(&config.ssh_key_path);

    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            println!(
                "\n{} Key added successfully!\n",
                style("✓").green().bold()
            );
            Ok(())
        }
        Ok(_) => Err(SpuffError::Ssh(
            "Failed to add SSH key to agent. Make sure you entered the correct passphrase."
                .to_string(),
        )),
        Err(e) => Err(SpuffError::Ssh(format!(
            "Failed to run ssh-add: {}",
            e
        ))),
    }
}

/// Upload local spuff-agent binary and ensure the service is running
async fn upload_local_agent(ip: &str, config: &AppConfig, agent_path: &str) -> Result<()> {
    // Ensure /opt/spuff exists
    crate::connector::ssh::run_command(ip, config, "sudo mkdir -p /opt/spuff").await?;

    // Upload to a temp location first
    crate::connector::ssh::scp_upload(ip, config, agent_path, "/tmp/spuff-agent").await?;

    // Move to final location and set permissions
    crate::connector::ssh::run_command(
        ip,
        config,
        "sudo mv /tmp/spuff-agent /opt/spuff/spuff-agent && sudo chmod +x /opt/spuff/spuff-agent",
    )
    .await?;

    // Ensure the service is enabled and start/restart it
    // The service file should exist from cloud-init write_files
    crate::connector::ssh::run_command(
        ip,
        config,
        "sudo systemctl daemon-reload && sudo systemctl enable spuff-agent 2>/dev/null; sudo systemctl restart spuff-agent 2>/dev/null || sudo systemctl start spuff-agent 2>/dev/null || true",
    )
    .await?;

    Ok(())
}

/// Trigger devtools installation via the agent
///
/// Reads the devtools config from /opt/spuff/devtools.json and sends it to the agent's
/// /devtools/install endpoint. The agent will install tools asynchronously in the background.
async fn trigger_devtools_installation(ip: &str, config: &AppConfig) -> Result<()> {
    // Read devtools config from the VM
    let devtools_json = crate::connector::ssh::run_command(
        ip,
        config,
        "cat /opt/spuff/devtools.json 2>/dev/null || echo '{}'",
    )
    .await?;

    // Parse to validate it's valid JSON
    let devtools_config: serde_json::Value = serde_json::from_str(&devtools_json)
        .map_err(|e| SpuffError::Provider(format!("Invalid devtools.json: {}", e)))?;

    // Skip if config is empty
    if devtools_config.as_object().is_none_or(|o| o.is_empty()) {
        tracing::debug!("No devtools config found, skipping installation trigger");
        return Ok(());
    }

    // Call the agent's devtools install endpoint via SSH tunnel
    let curl_cmd = format!(
        r#"curl -s -X POST http://127.0.0.1:7575/devtools/install \
           -H "Content-Type: application/json" \
           -d '{}'"#,
        devtools_json.trim()
    );

    let result = crate::connector::ssh::run_command(ip, config, &curl_cmd).await?;

    // Check for success
    if result.contains("\"started\":true") || result.contains("already in progress") {
        tracing::info!("Devtools installation triggered successfully");
        Ok(())
    } else {
        tracing::warn!("Devtools install response: {}", result);
        // Don't fail even if the response is unexpected
        Ok(())
    }
}
