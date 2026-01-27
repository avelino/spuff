use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use console::style;
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::connector::ssh::{wait_for_ssh, wait_for_ssh_login};
use crate::environment::cloud_init::generate_cloud_init_with_ai_tools;
use crate::error::{Result, SpuffError};
use crate::project_config::{AiToolsConfig, ProjectConfig};
use crate::provider::{create_provider, ImageSpec, InstanceRequest, VolumeMount};
use crate::ssh::{is_key_in_agent, is_ssh_agent_running, key_has_passphrase, SshClient, SshConfig};
use crate::state::{LocalInstance, StateDb};
use crate::tui::{run_progress_ui, ProgressMessage, StepState};
use crate::volume::{get_install_instructions, SshfsDriver, SshfsLocalCommands, VolumeState};

const STEP_CLOUD_INIT: usize = 0;
const STEP_CREATE: usize = 1;
const STEP_WAIT_READY: usize = 2;
const STEP_WAIT_SSH: usize = 3;
const STEP_BOOTSTRAP: usize = 4;
const STEP_VOLUMES: usize = 5;

// Bootstrap sub-steps (indices) - minimal bootstrap, devtools installed via agent
const SUB_PACKAGES: usize = 0;
const SUB_AGENT: usize = 1;

/// Parameters for instance provisioning
struct ProvisionParams {
    config: AppConfig,
    size: Option<String>,
    snapshot: Option<String>,
    region: Option<String>,
    project_config: Option<ProjectConfig>,
    cli_ai_tools: Option<AiToolsConfig>,
    dev: bool,
    db: StateDb,
}

pub async fn execute(
    config: &AppConfig,
    size: Option<String>,
    snapshot: Option<String>,
    region: Option<String>,
    no_connect: bool,
    dev: bool,
    ai_tools: Option<String>,
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

    // Load project config from spuff.yaml (if exists)
    let project_config = ProjectConfig::load_from_cwd().ok().flatten();

    // Apply project config overrides (CLI args take precedence)
    let effective_size = size.or_else(|| {
        project_config
            .as_ref()
            .and_then(|p| p.resources.size.clone())
    });
    let effective_region = region.or_else(|| {
        project_config
            .as_ref()
            .and_then(|p| p.resources.region.clone())
    });

    // Process AI tools configuration
    // Priority: CLI > Project config > Global config > Default (all)
    let cli_ai_tools = match ai_tools.as_deref() {
        Some("ask") => {
            // Interactive mode - prompt user to select AI tools
            println!(
                "{} Select AI coding tools to install:",
                style("?").cyan().bold()
            );
            println!("  1. {} (Anthropic)", style("claude-code").cyan());
            println!("  2. {} (OpenAI)", style("codex").cyan());
            println!("  3. {} (open source)", style("opencode").cyan());
            println!("  a. {} - install all", style("all").green());
            println!("  n. {} - install none", style("none").yellow());
            println!();
            print!("Enter your choice (comma-separated numbers, 'all', or 'none'): ");
            use std::io::Write;
            std::io::stdout().flush().ok();

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            let input = input.trim().to_lowercase();

            Some(match input.as_str() {
                "a" | "all" => AiToolsConfig::All,
                "n" | "none" => AiToolsConfig::None,
                _ => {
                    let tools: Vec<String> = input
                        .split(',')
                        .filter_map(|s| match s.trim() {
                            "1" | "claude-code" => Some("claude-code".to_string()),
                            "2" | "codex" => Some("codex".to_string()),
                            "3" | "opencode" => Some("opencode".to_string()),
                            _ => None,
                        })
                        .collect();
                    if tools.is_empty() {
                        AiToolsConfig::All
                    } else {
                        AiToolsConfig::List(tools)
                    }
                }
            })
        }
        Some(arg) => Some(AiToolsConfig::from_cli_arg(arg)),
        None => None, // Will use project/global config defaults
    };

    let is_docker = config.provider == "docker" || config.provider == "local";

    // Pre-flight checks (skip for Docker which doesn't use SSH)
    if !is_docker {
        // Pre-flight check: verify SSH key is usable
        verify_ssh_key_accessible(config).await?;

        // Pre-flight check: verify SSHFS is available if volumes are configured
        // Check both global config volumes and project volumes
        let has_volumes = !config.volumes.is_empty()
            || project_config
                .as_ref()
                .map(|pc| !pc.volumes.is_empty())
                .unwrap_or(false);
        if has_volumes {
            verify_sshfs_available().await?;
        }
    }

    // In dev mode, build agent for Linux and upload (skip for Docker)
    if dev && !is_docker {
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
                println!("{} Failed to build agent: {}", style("✕").red().bold(), e);
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
        "Mounting volumes".to_string(),
    ];

    if dev {
        steps.push("Uploading local agent".to_string());
    }

    // Print project config summary if found
    if let Some(ref pc) = project_config {
        print_project_summary(pc);
    }

    // Clone config values for the async task
    let params = ProvisionParams {
        config: config.clone(),
        size: effective_size.clone(),
        snapshot: snapshot.clone(),
        region: effective_region.clone(),
        project_config: project_config.clone(),
        cli_ai_tools: cli_ai_tools.clone(),
        dev,
        db,
    };

    // Spawn the provisioning task
    let provision_task = tokio::spawn(async move { provision_instance(params, tx).await });

    // Run the TUI
    let tui_result = run_progress_ui(steps, rx).await;

    // Wait for the provisioning task
    let provision_result = provision_task.await;

    // Handle results
    let is_docker = config.provider == "docker" || config.provider == "local";

    match (tui_result, provision_result) {
        (Ok(Some((name, ip_or_id))), Ok(Ok(()))) => {
            // Success - print final message
            super::ssh::print_banner();
            println!(
                "  {} {} {}",
                style("✓").green().bold(),
                style(&name).white().bold(),
                style(format!("({})", &ip_or_id)).dim()
            );
            println!();

            if is_docker {
                println!(
                    "  {}  docker exec -it {} /bin/bash",
                    style("Shell").dim(),
                    &ip_or_id[..12.min(ip_or_id.len())]
                );
                println!(
                    "  {}  container runs until 'spuff down'",
                    style("Note").dim()
                );
            } else {
                println!(
                    "  {}  ssh {}@{}",
                    style("SSH").dim(),
                    config.ssh_user,
                    ip_or_id
                );
                println!(
                    "  {}  auto-destroy after {}",
                    style("Idle").dim(),
                    style(&config.idle_timeout).yellow()
                );
            }

            if !no_connect {
                println!();
                if is_docker {
                    // For Docker, get the container ID from state
                    let db = StateDb::open()?;
                    if let Some(instance) = db.get_active_instance()? {
                        crate::connector::docker::connect(&instance.id).await?;
                    }
                } else {
                    crate::connector::ssh::connect(&ip_or_id, config).await?;
                }
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

const STEP_UPLOAD_AGENT: usize = 6;

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
        (
            cargo.clone(),
            vec![
                "zigbuild",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
    } else if has_cross().await {
        // cross: requires Docker but works well
        let cross_path = if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.cargo/bin/cross", home);
            if std::path::Path::new(&path).exists() {
                path
            } else {
                "cross".to_string()
            }
        } else {
            "cross".to_string()
        };
        (
            cross_path,
            vec![
                "build",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
    } else {
        // Fallback to cargo (requires linker setup)
        (
            cargo.clone(),
            vec![
                "build",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
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

/// Print project configuration summary
fn print_project_summary(project_config: &ProjectConfig) {
    println!();
    println!(
        "  {}",
        style("╭──────────────────────────────────────────────────────────╮").dim()
    );
    println!(
        "  {}  {:<56} {}",
        style("│").dim(),
        style("Project Setup (spuff.yaml)").cyan().bold(),
        style("│").dim()
    );

    if !project_config.bundles.is_empty() {
        let bundles_str = project_config.bundles.join(", ");
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Bundles: {}", bundles_str),
            style("│").dim()
        );
    }

    if !project_config.packages.is_empty() {
        let count = project_config.packages.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Packages: {} to install", count),
            style("│").dim()
        );
    }

    if project_config.services.enabled {
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Services: {}", project_config.services.compose_file),
            style("│").dim()
        );
    }

    if !project_config.repositories.is_empty() {
        let count = project_config.repositories.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Repositories: {} to clone", count),
            style("│").dim()
        );
    }

    if !project_config.ports.is_empty() {
        let ports_str = project_config
            .ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Ports: {} (tunnel via spuff ssh)", ports_str),
            style("│").dim()
        );
    }

    if !project_config.setup.is_empty() {
        let count = project_config.setup.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Setup scripts: {} commands", count),
            style("│").dim()
        );
    }

    // Show volumes if configured
    if !project_config.volumes.is_empty() {
        let count = project_config.volumes.len();
        println!(
            "  {}  {:<56} {}",
            style("│").dim(),
            format!("Volumes: {} mount(s)", count),
            style("│").dim()
        );
    }

    // Show AI tools config if not default (all)
    match &project_config.ai_tools {
        AiToolsConfig::None => {
            println!(
                "  {}  {:<56} {}",
                style("│").dim(),
                format!("AI tools: {}", style("none").yellow()),
                style("│").dim()
            );
        }
        AiToolsConfig::List(tools) => {
            let tools_str = tools.join(", ");
            println!(
                "  {}  {:<56} {}",
                style("│").dim(),
                format!("AI tools: {}", tools_str),
                style("│").dim()
            );
        }
        AiToolsConfig::All => {
            // Default - don't print anything
        }
    }

    println!(
        "  {}",
        style("╰──────────────────────────────────────────────────────────╯").dim()
    );
    println!();
}

async fn provision_instance(
    params: ProvisionParams,
    tx: mpsc::Sender<ProgressMessage>,
) -> Result<()> {
    let db = params.db;
    let provider = create_provider(&params.config)?;

    // Step 1: Generate cloud-init
    tx.send(ProgressMessage::SetStep(
        STEP_CLOUD_INIT,
        StepState::InProgress,
    ))
    .await
    .ok();
    tx.send(ProgressMessage::SetDetail(
        "Preparing environment configuration...".to_string(),
    ))
    .await
    .ok();

    let user_data = generate_cloud_init_with_ai_tools(
        &params.config,
        params.project_config.as_ref(),
        params.cli_ai_tools.as_ref(),
    )?;
    tx.send(ProgressMessage::SetStep(STEP_CLOUD_INIT, StepState::Done))
        .await
        .ok();

    // Step 2: Create instance
    tx.send(ProgressMessage::SetStep(STEP_CREATE, StepState::InProgress))
        .await
        .ok();
    tx.send(ProgressMessage::SetDetail(
        "Requesting VM from provider...".to_string(),
    ))
    .await
    .ok();

    let instance_name = generate_instance_name();
    let instance_region = params
        .region
        .unwrap_or_else(|| params.config.region.clone());
    let instance_size = params.size.unwrap_or_else(|| params.config.size.clone());
    let image = get_image_spec(&params.config.provider, params.snapshot);
    let is_docker = params.config.provider == "docker" || params.config.provider == "local";

    // Build volume mounts for Docker provider
    let volume_mounts = if is_docker {
        build_docker_volume_mounts(&params.config, params.project_config.as_ref())
    } else {
        Vec::new()
    };

    let request = InstanceRequest::new(
        instance_name.clone(),
        instance_region.clone(),
        instance_size.clone(),
    )
    .with_image(image)
    .with_user_data(user_data)
    .with_label("spuff", "true")
    .with_label("managed-by", "spuff-cli")
    .with_volumes(volume_mounts);

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
    tx.send(ProgressMessage::SetStep(
        STEP_WAIT_READY,
        StepState::InProgress,
    ))
    .await
    .ok();
    tx.send(ProgressMessage::SetDetail(
        "Waiting for VM to be assigned an IP...".to_string(),
    ))
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
        params.config.provider.clone(),
        instance_region.clone(),
        instance_size.clone(),
    );
    db.save_instance(&local_instance)?;

    tx.send(ProgressMessage::SetStep(STEP_WAIT_READY, StepState::Done))
        .await
        .ok();

    // Step 4: Wait for SSH (or Docker container ready)
    tx.send(ProgressMessage::SetStep(
        STEP_WAIT_SSH,
        StepState::InProgress,
    ))
    .await
    .ok();

    let is_docker = params.config.provider == "docker" || params.config.provider == "local";

    if is_docker {
        // For Docker, just verify container is running
        tx.send(ProgressMessage::SetDetail(
            "Waiting for container to be ready...".to_string(),
        ))
        .await
        .ok();

        if let Err(e) = crate::connector::docker::wait_for_ready(&instance.id).await {
            tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
            return Err(e);
        }
    } else {
        // For cloud providers, wait for SSH
        tx.send(ProgressMessage::SetDetail(format!(
            "Waiting for SSH port on {}...",
            instance.ip
        )))
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
        tx.send(ProgressMessage::SetDetail(format!(
            "Waiting for user {}...",
            params.config.ssh_user
        )))
        .await
        .ok();

        if let Err(e) = wait_for_ssh_login(
            &instance.ip.to_string(),
            &params.config,
            Duration::from_secs(120),
        )
        .await
        {
            tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
            return Err(e);
        }
    }

    tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Done))
        .await
        .ok();

    // In dev mode, upload local agent binary (skip for Docker)
    if params.dev && !is_docker {
        tx.send(ProgressMessage::SetStep(
            STEP_UPLOAD_AGENT,
            StepState::InProgress,
        ))
        .await
        .ok();
        tx.send(ProgressMessage::SetDetail(
            "Uploading local spuff-agent...".to_string(),
        ))
        .await
        .ok();

        let agent_path = get_linux_agent_path();
        let ip_str = instance.ip.to_string();

        // Upload the binary
        if let Err(e) = upload_local_agent(&ip_str, &params.config, &agent_path).await {
            tx.send(ProgressMessage::SetStep(
                STEP_UPLOAD_AGENT,
                StepState::Failed,
            ))
            .await
            .ok();
            tx.send(ProgressMessage::SetDetail(format!(
                "Agent upload failed: {}",
                e
            )))
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

    // Step 5/6: Wait for cloud-init with sub-steps (skip for Docker)
    tx.send(ProgressMessage::SetStep(
        STEP_BOOTSTRAP,
        StepState::InProgress,
    ))
    .await
    .ok();

    if is_docker {
        // Docker containers are ready immediately - no cloud-init needed
        tx.send(ProgressMessage::SetDetail(
            "Container ready (no bootstrap needed)".to_string(),
        ))
        .await
        .ok();
    } else {
        // Define sub-steps for bootstrap (minimal - devtools installed via agent)
        let sub_steps = vec![
            "Updating packages".to_string(),
            "Installing spuff-agent".to_string(),
        ];

        tx.send(ProgressMessage::SetSubSteps(STEP_BOOTSTRAP, sub_steps))
            .await
            .ok();

        // Wait for cloud-init with progress tracking
        if let Err(e) =
            wait_for_cloud_init_with_progress(&instance.ip.to_string(), &params.config, &tx).await
        {
            tracing::warn!("Cloud-init wait failed: {}", e);
            // Don't fail the whole process, just warn
        }
    }

    tx.send(ProgressMessage::SetStep(STEP_BOOTSTRAP, StepState::Done))
        .await
        .ok();

    // Step: Mount volumes (if configured)
    // For Docker: volumes are bind-mounted at container creation, skip SSHFS
    // For cloud: use SSHFS to mount remote directories locally
    tx.send(ProgressMessage::SetStep(
        STEP_VOLUMES,
        StepState::InProgress,
    ))
    .await
    .ok();

    if is_docker {
        // Docker volumes are handled via bind mounts at container creation
        tx.send(ProgressMessage::SetDetail(
            "Volumes mounted via Docker bind mounts".to_string(),
        ))
        .await
        .ok();
    } else {
        // Build merged volume list: global volumes first, then project volumes
        // Project volumes with same target override global ones
        let mut merged_volumes: Vec<(crate::volume::VolumeConfig, Option<&std::path::Path>)> =
            Vec::new();

        // Add global volumes (no base_dir)
        for vol in &params.config.volumes {
            merged_volumes.push((vol.clone(), None));
        }

        // Add project volumes (with base_dir), overriding globals with same target
        if let Some(ref pc) = params.project_config {
            for vol in &pc.volumes {
                // Remove any global volume with the same target
                merged_volumes.retain(|(v, _)| v.target != vol.target);
                merged_volumes.push((vol.clone(), pc.base_dir.as_deref()));
            }
        }

        if !merged_volumes.is_empty() {
            // Check SSHFS availability
            let sshfs_available = SshfsDriver::check_sshfs_installed().await;
            let fuse_available = SshfsDriver::check_fuse_available().await;

            if !sshfs_available || !fuse_available {
                tx.send(ProgressMessage::SetDetail(
                    "SSHFS not available - skipping volume mount".to_string(),
                ))
                .await
                .ok();
                tracing::warn!("SSHFS not available, skipping volume mount");
            } else {
                let ssh_key_path = crate::ssh::managed_key::get_managed_key_path()
                    .unwrap_or_else(|_| PathBuf::from(&params.config.ssh_key_path));
                let ssh_key_str = ssh_key_path.to_string_lossy().to_string();

                let mut mounted_count = 0;
                let total_volumes = merged_volumes.len();

                for (idx, (vol, base_dir)) in merged_volumes.iter().enumerate() {
                    let mount_point = vol.resolve_mount_point(Some(&instance_name), *base_dir);
                    let source_path = vol.resolve_source(*base_dir);

                    // Check if this is a file volume (SSHFS doesn't support mounting files)
                    // Use async version that checks remotely via SSH when local source doesn't exist
                    let is_file = is_file_volume_async(
                        &source_path,
                        &vol.target,
                        &instance.ip.to_string(),
                        &params.config.ssh_user,
                        &ssh_key_str,
                    )
                    .await;

                    tx.send(ProgressMessage::SetDetail(format!(
                        "{} volume {}/{}: {}",
                        if is_file { "Syncing" } else { "Mounting" },
                        idx + 1,
                        total_volumes,
                        vol.target
                    )))
                    .await
                    .ok();

                    // Sync source to VM if defined
                    if !source_path.is_empty() && std::path::Path::new(&source_path).exists() {
                        if let Err(e) = sync_to_vm(
                            &instance.ip.to_string(),
                            &params.config.ssh_user,
                            &ssh_key_str,
                            &source_path,
                            &vol.target,
                        )
                        .await
                        {
                            tracing::warn!("Failed to sync {} to VM: {}", source_path, e);
                        } else if is_file {
                            // For files, sync counts as success (no SSHFS mount)
                            mounted_count += 1;
                        }
                    }

                    // Skip SSHFS mount for files (not supported)
                    if is_file {
                        tracing::debug!(
                        "Skipping SSHFS mount for file volume: {} (SSHFS only supports directories)",
                        vol.target
                    );
                        continue;
                    }

                    // Mount the directory volume
                    match SshfsLocalCommands::mount(
                        &instance.ip.to_string(),
                        &params.config.ssh_user,
                        &ssh_key_str,
                        &vol.target,
                        &mount_point,
                        vol.read_only,
                        &vol.options,
                    )
                    .await
                    {
                        Ok(_) => {
                            mounted_count += 1;
                            // Save to volume state
                            let mut state = VolumeState::load_or_default();
                            let handle =
                                crate::volume::MountHandle::new("sshfs", &vol.target, &mount_point)
                                    .with_vm_info(instance.ip.to_string(), &params.config.ssh_user)
                                    .with_source(&source_path)
                                    .with_read_only(vol.read_only);
                            state.add_mount(handle);
                            if let Err(e) = state.save() {
                                tracing::error!(
                                    "Failed to save volume state: {}. Mount may not persist.",
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to mount {}: {}", vol.target, e);
                        }
                    }
                }

                tx.send(ProgressMessage::SetDetail(format!(
                    "Mounted {}/{} volume(s)",
                    mounted_count, total_volumes
                )))
                .await
                .ok();
            }
        } else {
            tx.send(ProgressMessage::SetDetail(
                "No volumes configured".to_string(),
            ))
            .await
            .ok();
        }
    } // end of else (cloud provider volumes)

    tx.send(ProgressMessage::SetStep(STEP_VOLUMES, StepState::Done))
        .await
        .ok();

    // Trigger devtools installation via agent (async, non-blocking)
    // Skip for Docker as it doesn't have the agent
    if !is_docker {
        tx.send(ProgressMessage::SetDetail(
            "Triggering devtools installation...".to_string(),
        ))
        .await
        .ok();

        if let Err(e) =
            trigger_devtools_installation(&instance.ip.to_string(), &params.config).await
        {
            tracing::warn!("Failed to trigger devtools installation: {}", e);
            // Don't fail - user can trigger manually with `spuff agent devtools install`
        }
    }

    // Complete!
    // For Docker: send container ID instead of IP (used for shell connection)
    let display_id = if is_docker {
        instance.id.clone()
    } else {
        instance.ip.to_string()
    };

    tx.send(ProgressMessage::Complete(instance_name.clone(), display_id))
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
    tx.send(ProgressMessage::SetSubStep(
        STEP_BOOTSTRAP,
        SUB_PACKAGES,
        StepState::InProgress,
    ))
    .await
    .ok();
    tx.send(ProgressMessage::SetDetail(
        "Updating system packages...".to_string(),
    ))
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
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_PACKAGES,
                    StepState::Done,
                ))
                .await
                .ok();
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_AGENT,
                    StepState::InProgress,
                ))
                .await
                .ok();
                tx.send(ProgressMessage::SetDetail(
                    "Installing spuff-agent...".to_string(),
                ))
                .await
                .ok();
            }

            // Check for spuff-agent installation
            if !agent_done
                && log.contains("spuff-agent")
                && (log.contains("systemctl enable spuff-agent")
                    || log.contains("systemctl start spuff-agent"))
            {
                agent_done = true;
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    SUB_AGENT,
                    StepState::Done,
                ))
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
                    tx.send(ProgressMessage::SetSubStep(
                        STEP_BOOTSTRAP,
                        i,
                        StepState::Done,
                    ))
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
                tx.send(ProgressMessage::SetSubStep(
                    STEP_BOOTSTRAP,
                    i,
                    StepState::Done,
                ))
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

/// Verify SSH key is accessible for authentication.
///
/// Uses pure Rust SSH libraries - no external binaries required.
///
/// Checks:
/// 1. SSH agent is running (SSH_AUTH_SOCK exists)
/// 2. Key is either in the agent OR has no passphrase
///
/// If the key requires a passphrase and is not in the agent, returns an error
/// with instructions for the user to manually add the key.
async fn verify_ssh_key_accessible(config: &AppConfig) -> Result<()> {
    let key_path = PathBuf::from(&config.ssh_key_path);

    // Check if key exists
    if !key_path.exists() {
        return Err(SpuffError::Ssh(format!(
            "SSH key not found: {}",
            config.ssh_key_path
        )));
    }

    // Check if key has no passphrase (can be used directly)
    match key_has_passphrase(&key_path) {
        Ok(false) => {
            // Key has no passphrase - can be used directly
            return Ok(());
        }
        Ok(true) => {
            // Key has passphrase - need agent
        }
        Err(e) => {
            return Err(SpuffError::Ssh(format!(
                "Failed to check SSH key: {}. Make sure the key format is supported.",
                e
            )));
        }
    }

    // Key has passphrase - check if agent is running
    if !is_ssh_agent_running() {
        return Err(SpuffError::Ssh(
            "SSH key requires passphrase but ssh-agent is not running.\n\
             Start ssh-agent and add your key:\n  \
             eval \"$(ssh-agent -s)\"\n  \
             ssh-add ~/.ssh/id_ed25519"
                .to_string(),
        ));
    }

    // Check if key is loaded in the agent
    if is_key_in_agent(&key_path).await {
        return Ok(());
    }

    // Key has passphrase but not in agent
    Err(SpuffError::Ssh(format!(
        "SSH key requires passphrase but is not loaded in the agent.\n\
         Add your key to the agent:\n  \
         ssh-add {}\n\n\
         On macOS, you can also use Keychain:\n  \
         ssh-add --apple-use-keychain {}",
        config.ssh_key_path, config.ssh_key_path
    )))
}

/// Verify SSHFS is available before creating a VM.
///
/// This is a pre-flight check to fail fast with helpful instructions
/// rather than creating a VM that won't work as expected.
async fn verify_sshfs_available() -> Result<()> {
    let sshfs_installed = SshfsDriver::check_sshfs_installed().await;
    let fuse_available = SshfsDriver::check_fuse_available().await;

    if !sshfs_installed || !fuse_available {
        return Err(SpuffError::Volume(format!(
            "SSHFS is required for volume mounts but is not available.\n\n{}",
            get_install_instructions()
        )));
    }

    Ok(())
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

/// Sync local file or directory to VM using rsync
async fn sync_to_vm(
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    use tokio::process::Command;

    let local_metadata = std::fs::metadata(local_path).map_err(|e| {
        SpuffError::Volume(format!(
            "Failed to get metadata for '{}': {}",
            local_path, e
        ))
    })?;

    let is_file = local_metadata.is_file();

    // Build rsync command with SSH options
    let ssh_cmd = format!(
        "ssh -i \"{}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
        ssh_key_path
    );

    // For directories: add trailing / to sync contents, not the directory itself
    // For files: use the path as-is
    let local_source = if is_file || local_path.ends_with('/') {
        local_path.to_string()
    } else {
        format!("{}/", local_path)
    };

    let remote_dest = format!("{}@{}:{}", ssh_user, vm_ip, remote_path);

    // First ensure remote directory/parent exists
    let path_to_create = if is_file {
        // For files, create the parent directory
        std::path::Path::new(remote_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        remote_path.to_string()
    };

    if !path_to_create.is_empty() && path_to_create != "/" {
        // Use internal SSH implementation
        let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
        match SshClient::connect(vm_ip, 22, &config).await {
            Ok(client) => {
                let command = format!("mkdir -p '{}'", path_to_create.replace('\'', "'\\''"));
                match client.exec(&command).await {
                    Ok(output) => {
                        if !output.success {
                            tracing::warn!("Failed to create remote directory: {}", output.stderr);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create remote directory: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to connect via SSH: {}", e);
            }
        }
    }

    // Build rsync args - only use --delete for directories
    let mut rsync_args = vec!["-avz"];
    if !is_file {
        rsync_args.push("--delete");
    }
    rsync_args.extend(["-e", &ssh_cmd, &local_source, &remote_dest]);

    // Run rsync
    let output = Command::new("rsync")
        .args(&rsync_args)
        .output()
        .await
        .map_err(|e| SpuffError::Volume(format!("Failed to execute rsync: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SpuffError::Volume(format!(
            "Failed to sync to VM: {}",
            stderr
        )));
    }

    tracing::debug!(
        "Synced {} ({}) to {}:{}",
        local_path,
        if is_file { "file" } else { "directory" },
        vm_ip,
        remote_path
    );
    Ok(())
}

/// Build volume mounts for Docker provider from config and project config.
///
/// Merges global volumes with project-specific volumes, converting them to
/// Docker bind mount format for container creation.
fn build_docker_volume_mounts(
    config: &AppConfig,
    project_config: Option<&ProjectConfig>,
) -> Vec<VolumeMount> {
    let mut mounts = Vec::new();

    // Add global volumes from config
    for vol in &config.volumes {
        let source = vol.resolve_source(None);
        if !source.is_empty() {
            mounts.push(VolumeMount {
                source,
                target: vol.target.clone(),
                read_only: vol.read_only,
            });
        }
    }

    // Add project volumes (override globals with same target)
    if let Some(pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            mounts.retain(|m| m.target != vol.target);

            let source = vol.resolve_source(pc.base_dir.as_deref());
            if !source.is_empty() {
                mounts.push(VolumeMount {
                    source,
                    target: vol.target.clone(),
                    read_only: vol.read_only,
                });
            }
        }
    }

    mounts
}

/// Check if a source path is a file (not directory)
/// If source exists locally, use its metadata.
/// If source doesn't exist, check remotely via SSH.
async fn is_file_volume_async(
    source_path: &str,
    target: &str,
    vm_ip: &str,
    ssh_user: &str,
    ssh_key_path: &str,
) -> bool {
    // If source exists locally, use its metadata
    if !source_path.is_empty() && std::path::Path::new(source_path).exists() {
        return std::fs::metadata(source_path)
            .map(|m| m.is_file())
            .unwrap_or(false);
    }

    // If target ends with /, it's definitely a directory
    if target.ends_with('/') {
        return false;
    }

    // Check remotely via SSH if the target exists and is a directory
    let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
    if let Ok(client) = SshClient::connect(vm_ip, 22, &config).await {
        // Use test -d to check if path is a directory, test -f to check if it's a file
        let escaped_target = target.replace('\'', "'\\''");
        let command = format!(
            "if [ -d '{}' ]; then echo 'dir'; elif [ -f '{}' ]; then echo 'file'; else echo 'unknown'; fi",
            escaped_target, escaped_target
        );
        if let Ok(output) = client.exec(&command).await {
            let result = output.stdout.trim();
            match result {
                "dir" => return false,
                "file" => return true,
                _ => {
                    // Path doesn't exist yet - default to directory (safer for SSHFS)
                    return false;
                }
            }
        }
    }

    // Fallback: if SSH check fails, default to directory (safer for SSHFS)
    // This allows the mount to proceed and the user can see if it fails
    false
}
