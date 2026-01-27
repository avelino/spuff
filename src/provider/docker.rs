//! Docker provider implementation.
//!
//! This module implements the Provider trait for local Docker containers,
//! enabling local development environments using the Docker API via bollard.

use std::collections::HashMap;

use async_trait::async_trait;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::image::{CommitContainerOptions, CreateImageOptions, ListImagesOptions};
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;

use super::config::{ImageSpec, InstanceRequest, ProviderTimeouts, ProviderType};
use super::error::{ProviderError, ProviderResult};
use super::registry::ProviderFactory;
use super::{InstanceStatus, Provider, ProviderInstance, Snapshot};

/// Docker provider implementation using bollard.
///
/// Communicates with Docker daemon via Unix socket (`/var/run/docker.sock`)
/// for local container management.
pub struct DockerProvider {
    client: Docker,
    #[allow(dead_code)]
    timeouts: ProviderTimeouts,
}

impl DockerProvider {
    /// Create a new Docker provider connecting via Unix socket.
    pub fn new(timeouts: ProviderTimeouts) -> ProviderResult<Self> {
        let client = Docker::connect_with_socket_defaults().map_err(|e| ProviderError::Other {
            message: format!("Failed to connect to Docker socket: {}", e),
        })?;

        Ok(Self { client, timeouts })
    }

    /// Resolve ImageSpec to Docker image name.
    fn resolve_image(&self, spec: &ImageSpec) -> String {
        match spec {
            ImageSpec::Ubuntu(version) => format!("ubuntu:{}", version),
            ImageSpec::Debian(version) => format!("debian:{}", version),
            ImageSpec::Custom(id) => id.clone(),
            ImageSpec::Snapshot(id) => id.clone(),
        }
    }

    /// Pull an image if not present locally.
    async fn ensure_image(&self, image: &str) -> ProviderResult<()> {
        // Check if image exists locally
        let filters: HashMap<String, Vec<String>> =
            HashMap::from([("reference".to_string(), vec![image.to_string()])]);

        let images = self
            .client
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to list images: {}", e),
            })?;

        if !images.is_empty() {
            tracing::debug!("Image {} already exists locally", image);
            return Ok(());
        }

        // Pull the image
        tracing::info!("Pulling image {}...", image);
        let options = CreateImageOptions {
            from_image: image,
            ..Default::default()
        };

        self.client
            .create_image(Some(options), None, None)
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to pull image {}: {}", image, e),
            })?;

        tracing::info!("Image {} pulled successfully", image);
        Ok(())
    }

    /// Convert container info to ProviderInstance.
    fn container_to_instance(
        &self,
        id: String,
        state: Option<&str>,
        created: Option<i64>,
    ) -> ProviderInstance {
        let status = match state {
            Some("running") => InstanceStatus::Active,
            Some("created") => InstanceStatus::New,
            Some("exited") | Some("dead") => InstanceStatus::Off,
            Some(s) => InstanceStatus::Unknown(s.to_string()),
            None => InstanceStatus::Unknown("unknown".to_string()),
        };

        let created_at = created
            .and_then(|ts| DateTime::from_timestamp(ts, 0))
            .unwrap_or_else(Utc::now);

        ProviderInstance {
            id,
            ip: "127.0.0.1".parse().unwrap(),
            status,
            created_at,
        }
    }
}

#[async_trait]
impl Provider for DockerProvider {
    fn name(&self) -> &'static str {
        "docker"
    }

    async fn create_instance(&self, request: &InstanceRequest) -> ProviderResult<ProviderInstance> {
        let name = format!("spuff-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let image = self.resolve_image(&request.image);

        // Ensure image is available
        self.ensure_image(&image).await?;

        // Build labels
        let mut labels = HashMap::new();
        labels.insert("spuff".to_string(), "true".to_string());
        labels.insert("managed-by".to_string(), "spuff-cli".to_string());
        for (k, v) in &request.labels {
            labels.insert(k.clone(), v.clone());
        }

        // Port bindings for agent (7575)
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            "7575/tcp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some("7575".to_string()),
            }]),
        );

        // Exposed ports
        let mut exposed_ports = HashMap::new();
        exposed_ports.insert("7575/tcp".to_string(), HashMap::new());

        // Create container config
        let config = Config {
            image: Some(image),
            labels: Some(labels),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                ..Default::default()
            }),
            // Keep container running with sleep infinity
            cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
            tty: Some(true),
            ..Default::default()
        };

        // Create container
        let container = self
            .client
            .create_container(
                Some(CreateContainerOptions {
                    name: &name,
                    platform: None,
                }),
                config,
            )
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to create container: {}", e),
            })?;

        // Start container
        self.client
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to start container: {}", e),
            })?;

        tracing::info!("Container {} started (id: {})", name, &container.id[..12]);

        Ok(ProviderInstance {
            id: container.id,
            ip: "127.0.0.1".parse().unwrap(),
            status: InstanceStatus::Active,
            created_at: Utc::now(),
        })
    }

    async fn destroy_instance(&self, id: &str) -> ProviderResult<()> {
        // Stop container first (ignore errors if already stopped)
        let _ = self
            .client
            .stop_container(id, Some(StopContainerOptions { t: 5 }))
            .await;

        // Remove container
        self.client
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .or_else(|e| {
                // 404 is OK - container already gone
                if e.to_string().contains("404") || e.to_string().contains("No such container") {
                    Ok(())
                } else {
                    Err(ProviderError::Other {
                        message: format!("Failed to remove container: {}", e),
                    })
                }
            })?;

        tracing::info!("Container {} destroyed", &id[..12.min(id.len())]);
        Ok(())
    }

    async fn get_instance(&self, id: &str) -> ProviderResult<Option<ProviderInstance>> {
        match self.client.inspect_container(id, None).await {
            Ok(info) => {
                let state = info.state.as_ref().and_then(|s| {
                    s.status
                        .as_ref()
                        .map(|status| format!("{:?}", status).to_lowercase())
                });
                let created = info
                    .created
                    .as_ref()
                    .and_then(|c| DateTime::parse_from_rfc3339(c).ok())
                    .map(|dt| dt.timestamp());

                Ok(Some(self.container_to_instance(
                    info.id.unwrap_or_else(|| id.to_string()),
                    state.as_deref(),
                    created,
                )))
            }
            Err(e) => {
                if e.to_string().contains("404") || e.to_string().contains("No such container") {
                    Ok(None)
                } else {
                    Err(ProviderError::Other {
                        message: format!("Failed to inspect container: {}", e),
                    })
                }
            }
        }
    }

    async fn list_instances(&self) -> ProviderResult<Vec<ProviderInstance>> {
        let filters: HashMap<String, Vec<String>> =
            HashMap::from([("label".to_string(), vec!["spuff=true".to_string()])]);

        let containers = self
            .client
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to list containers: {}", e),
            })?;

        Ok(containers
            .into_iter()
            .map(|c| {
                self.container_to_instance(c.id.unwrap_or_default(), c.state.as_deref(), c.created)
            })
            .collect())
    }

    async fn wait_ready(&self, id: &str) -> ProviderResult<ProviderInstance> {
        // Docker containers become ready almost instantly
        // Just verify it's running
        match self.get_instance(id).await? {
            Some(instance) if instance.status == InstanceStatus::Active => Ok(instance),
            Some(instance) => Err(ProviderError::Other {
                message: format!("Container not running, status: {}", instance.status),
            }),
            None => Err(ProviderError::not_found("container", id)),
        }
    }

    async fn create_snapshot(&self, instance_id: &str, name: &str) -> ProviderResult<Snapshot> {
        // Docker commit creates an image from a container
        let commit = self
            .client
            .commit_container(
                CommitContainerOptions {
                    container: instance_id,
                    repo: "spuff",
                    tag: name,
                    ..Default::default()
                },
                Config::<String>::default(),
            )
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to create snapshot: {}", e),
            })?;

        tracing::info!("Snapshot created: spuff:{}", name);

        Ok(Snapshot {
            id: commit.id.unwrap_or_else(|| format!("spuff:{}", name)),
            name: name.to_string(),
            created_at: Some(Utc::now()),
        })
    }

    async fn list_snapshots(&self) -> ProviderResult<Vec<Snapshot>> {
        let filters: HashMap<String, Vec<String>> =
            HashMap::from([("reference".to_string(), vec!["spuff:*".to_string()])]);

        let images = self
            .client
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("Failed to list images: {}", e),
            })?;

        Ok(images
            .into_iter()
            .map(|i| {
                let name = i
                    .repo_tags
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                Snapshot {
                    id: i.id,
                    name,
                    created_at: DateTime::from_timestamp(i.created, 0),
                }
            })
            .collect())
    }

    async fn delete_snapshot(&self, id: &str) -> ProviderResult<()> {
        self.client
            .remove_image(id, None, None)
            .await
            .map_err(|e| {
                if e.to_string().contains("404") || e.to_string().contains("No such image") {
                    return ProviderError::not_found("image", id);
                }
                ProviderError::Other {
                    message: format!("Failed to delete snapshot: {}", e),
                }
            })?;

        Ok(())
    }

    fn supports_snapshots(&self) -> bool {
        true
    }
}

/// Factory for creating Docker providers.
pub struct DockerFactory;

impl ProviderFactory for DockerFactory {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Docker
    }

    fn create(
        &self,
        _token: &str,
        timeouts: ProviderTimeouts,
    ) -> ProviderResult<Box<dyn Provider>> {
        // Docker doesn't need a token - it uses the local socket
        Ok(Box::new(DockerProvider::new(timeouts)?))
    }

    fn is_implemented(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn test_resolve_image_ubuntu() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        assert_eq!(
            provider.resolve_image(&ImageSpec::ubuntu("24.04")),
            "ubuntu:24.04"
        );
        assert_eq!(
            provider.resolve_image(&ImageSpec::ubuntu("22.04")),
            "ubuntu:22.04"
        );
    }

    #[test]
    fn test_resolve_image_debian() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        assert_eq!(
            provider.resolve_image(&ImageSpec::debian("12")),
            "debian:12"
        );
    }

    #[test]
    fn test_resolve_image_custom() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        assert_eq!(
            provider.resolve_image(&ImageSpec::custom("nginx:alpine")),
            "nginx:alpine"
        );
    }

    #[test]
    fn test_factory_type() {
        let factory = DockerFactory;
        assert_eq!(factory.provider_type(), ProviderType::Docker);
        assert!(factory.is_implemented());
    }

    #[test]
    fn test_container_to_instance_active() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        let instance = provider.container_to_instance(
            "abc123".to_string(),
            Some("running"),
            Some(1704067200), // 2024-01-01 00:00:00 UTC
        );

        assert_eq!(instance.id, "abc123");
        assert_eq!(instance.status, InstanceStatus::Active);
        assert_eq!(instance.ip, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_container_to_instance_off() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        let instance = provider.container_to_instance("def456".to_string(), Some("exited"), None);

        assert_eq!(instance.status, InstanceStatus::Off);
    }

    #[test]
    fn test_container_to_instance_new() {
        let provider = DockerProvider {
            client: Docker::connect_with_socket_defaults().unwrap(),
            timeouts: ProviderTimeouts::default(),
        };

        let instance = provider.container_to_instance("ghi789".to_string(), Some("created"), None);

        assert_eq!(instance.status, InstanceStatus::New);
    }
}
