//! Instance provisioning
//!
//! Core provisioning logic for creating and configuring VM instances.

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::environment::cloud_init::generate_cloud_init_with_ai_tools;
use crate::error::Result;
use crate::project_config::{AiToolsConfig, ProjectConfig};
use crate::provider::{create_provider, ImageSpec, InstanceRequest};
use crate::state::{LocalInstance, StateDb};
use crate::tui::{ProgressMessage, StepState};
use crate::volume::{SshfsDriver, SshfsLocalCommands, VolumeState};

use super::agent_upload::{trigger_devtools_installation, upload_local_agent};
use super::bootstrap::wait_for_cloud_init_with_progress;
use super::build::get_linux_agent_path;
use super::volumes::{build_docker_volume_mounts, is_file_volume_async, sync_to_vm};
use super::{
    STEP_BOOTSTRAP, STEP_CLOUD_INIT, STEP_CREATE, STEP_UPLOAD_AGENT, STEP_VOLUMES, STEP_WAIT_READY,
    STEP_WAIT_SSH,
};

/// Parameters for instance provisioning
pub struct ProvisionParams {
    pub config: AppConfig,
    pub size: Option<String>,
    pub snapshot: Option<String>,
    pub region: Option<String>,
    pub project_config: Option<ProjectConfig>,
    pub cli_ai_tools: Option<AiToolsConfig>,
    pub dev: bool,
    pub db: StateDb,
}

pub async fn provision_instance(
    params: ProvisionParams,
    tx: mpsc::Sender<ProgressMessage>,
) -> Result<()> {
    // Extract all needed values upfront to avoid partial move issues
    let config = params.config;
    let project_config = params.project_config;
    let cli_ai_tools = params.cli_ai_tools;
    let dev = params.dev;
    let db = params.db;
    let snapshot = params.snapshot;
    let size = params.size;
    let region = params.region;

    let provider = create_provider(&config)?;
    let is_docker = config.provider == "docker" || config.provider == "local";

    // Step 1: Generate cloud-init (skip for Docker)
    tx.send(ProgressMessage::SetStep(
        STEP_CLOUD_INIT,
        StepState::InProgress,
    ))
    .await
    .ok();

    let user_data = if is_docker {
        tx.send(ProgressMessage::SetDetail(
            "Docker provider - skipping cloud-init".to_string(),
        ))
        .await
        .ok();
        String::new() // Docker doesn't use cloud-init
    } else {
        tx.send(ProgressMessage::SetDetail(
            "Preparing environment configuration...".to_string(),
        ))
        .await
        .ok();
        generate_cloud_init_with_ai_tools(&config, project_config.as_ref(), cli_ai_tools.as_ref())?
    };
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
    let instance_region = region.unwrap_or_else(|| config.region.clone());
    let instance_size = size.unwrap_or_else(|| config.size.clone());
    let image = get_image_spec(&config.provider, snapshot);

    // Build volume mounts for Docker provider
    let volume_mounts = if is_docker {
        build_docker_volume_mounts(&config, project_config.as_ref())
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
        config.provider.clone(),
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
        if let Err(e) = crate::connector::ssh::wait_for_ssh(
            &instance.ip.to_string(),
            22,
            Duration::from_secs(300),
        )
        .await
        {
            tx.send(ProgressMessage::SetStep(STEP_WAIT_SSH, StepState::Failed))
                .await
                .ok();
            tx.send(ProgressMessage::Failed(e.to_string())).await.ok();
            return Err(e);
        }

        // Then wait for user to exist and SSH login to work
        tx.send(ProgressMessage::SetDetail(format!(
            "Waiting for user {}...",
            config.ssh_user
        )))
        .await
        .ok();

        if let Err(e) = crate::connector::ssh::wait_for_ssh_login(
            &instance.ip.to_string(),
            &config,
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
    if dev && !is_docker {
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
        if let Err(e) = upload_local_agent(&ip_str, &config, &agent_path).await {
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
            wait_for_cloud_init_with_progress(&instance.ip.to_string(), &config, &tx).await
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
        mount_cloud_volumes(
            &config,
            project_config.as_ref(),
            &instance_name,
            &instance.ip.to_string(),
            &tx,
        )
        .await;
    }

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

        if let Err(e) = trigger_devtools_installation(&instance.ip.to_string(), &config).await {
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

async fn mount_cloud_volumes(
    config: &AppConfig,
    project_config: Option<&ProjectConfig>,
    instance_name: &str,
    ip: &str,
    tx: &mpsc::Sender<ProgressMessage>,
) {
    // Build merged volume list: global volumes first, then project volumes
    // Project volumes with same target override global ones
    let mut merged_volumes: Vec<(crate::volume::VolumeConfig, Option<&std::path::Path>)> =
        Vec::new();

    // Add global volumes (no base_dir)
    for vol in &config.volumes {
        merged_volumes.push((vol.clone(), None));
    }

    // Add project volumes (with base_dir), overriding globals with same target
    if let Some(pc) = project_config {
        for vol in &pc.volumes {
            // Remove any global volume with the same target
            merged_volumes.retain(|(v, _)| v.target != vol.target);
            merged_volumes.push((vol.clone(), pc.base_dir.as_deref()));
        }
    }

    if merged_volumes.is_empty() {
        tx.send(ProgressMessage::SetDetail(
            "No volumes configured".to_string(),
        ))
        .await
        .ok();
        return;
    }

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
        return;
    }

    let ssh_key_path = crate::ssh::managed_key::get_managed_key_path()
        .unwrap_or_else(|_| PathBuf::from(&config.ssh_key_path));
    let ssh_key_str = ssh_key_path.to_string_lossy().to_string();

    let mut mounted_count = 0;
    let total_volumes = merged_volumes.len();

    for (idx, (vol, base_dir)) in merged_volumes.iter().enumerate() {
        let mount_point = vol.resolve_mount_point(Some(instance_name), *base_dir);
        let source_path = vol.resolve_source(*base_dir);

        // Check if this is a file volume (SSHFS doesn't support mounting files)
        // Use async version that checks remotely via SSH when local source doesn't exist
        let is_file = is_file_volume_async(
            &source_path,
            &vol.target,
            ip,
            &config.ssh_user,
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
                ip,
                &config.ssh_user,
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
            ip,
            &config.ssh_user,
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
                let handle = crate::volume::MountHandle::new("sshfs", &vol.target, &mount_point)
                    .with_vm_info(ip.to_string(), &config.ssh_user)
                    .with_source(&source_path)
                    .with_read_only(vol.read_only);
                state.add_mount(handle);
                if let Err(e) = state.save() {
                    tracing::error!("Failed to save volume state: {}. Mount may not persist.", e);
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

pub fn generate_instance_name() -> String {
    let id = &uuid::Uuid::new_v4().to_string()[..8];
    format!("spuff-{}", id)
}

/// Get the appropriate image specification for the instance.
///
/// If a snapshot ID is provided, uses that. Otherwise, defaults to Ubuntu 24.04.
pub fn get_image_spec(provider: &str, snapshot: Option<String>) -> ImageSpec {
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
