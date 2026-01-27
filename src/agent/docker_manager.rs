//! Docker container management for the spuff-agent.
//!
//! This module provides APIs for managing Docker containers and docker-compose services
//! running on the VM.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Docker container information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    /// Container ID (short form).
    pub id: String,
    /// Container name.
    pub name: String,
    /// Image name with tag.
    pub image: String,
    /// Container status (running, exited, etc).
    pub status: String,
    /// State (running, exited, created, etc).
    pub state: String,
    /// Ports mapping.
    pub ports: Vec<String>,
    /// When the container was created.
    pub created: String,
}

/// Status of a docker-compose service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Service name.
    pub name: String,
    /// Container ID if running.
    pub container_id: Option<String>,
    /// Current state.
    pub state: String,
    /// Health status if applicable.
    pub health: Option<String>,
    /// Exposed ports.
    pub ports: Vec<String>,
}

/// Result of a docker operation.
#[derive(Debug, Serialize)]
pub struct DockerResult {
    pub success: bool,
    pub message: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl DockerResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            stdout: None,
            stderr: None,
        }
    }

    pub fn with_output(mut self, stdout: String, stderr: String) -> Self {
        self.stdout = if stdout.is_empty() {
            None
        } else {
            Some(stdout)
        };
        self.stderr = if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        };
        self
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            stdout: None,
            stderr: None,
        }
    }
}

/// Docker manager for container operations.
pub struct DockerManager;

impl DockerManager {
    /// Check if Docker is available on the system.
    pub async fn is_available() -> bool {
        tokio::process::Command::new("docker")
            .arg("version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// List all containers (running and stopped).
    pub async fn list_containers() -> Result<Vec<ContainerInfo>, String> {
        let output = tokio::process::Command::new("docker")
            .args([
                "ps",
                "-a",
                "--format",
                "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.State}}\t{{.Ports}}\t{{.CreatedAt}}",
            ])
            .output()
            .await
            .map_err(|e| format!("Failed to execute docker ps: {}", e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let containers: Vec<ContainerInfo> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                ContainerInfo {
                    id: parts.first().unwrap_or(&"").to_string(),
                    name: parts.get(1).unwrap_or(&"").to_string(),
                    image: parts.get(2).unwrap_or(&"").to_string(),
                    status: parts.get(3).unwrap_or(&"").to_string(),
                    state: parts.get(4).unwrap_or(&"").to_string(),
                    ports: parts
                        .get(5)
                        .unwrap_or(&"")
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    created: parts.get(6).unwrap_or(&"").to_string(),
                }
            })
            .collect();

        Ok(containers)
    }

    /// Start a specific container by name or ID.
    pub async fn start_container(name_or_id: &str) -> DockerResult {
        let output = tokio::process::Command::new("docker")
            .args(["start", name_or_id])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                DockerResult::success(format!("Container {} started", name_or_id)).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to start container: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute docker start: {}", e)),
        }
    }

    /// Stop a specific container by name or ID.
    pub async fn stop_container(name_or_id: &str, timeout_secs: Option<u32>) -> DockerResult {
        let timeout = timeout_secs.unwrap_or(10);
        let output = tokio::process::Command::new("docker")
            .args(["stop", "-t", &timeout.to_string(), name_or_id])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                DockerResult::success(format!("Container {} stopped", name_or_id)).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to stop container: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute docker stop: {}", e)),
        }
    }

    /// Restart a specific container by name or ID.
    pub async fn restart_container(name_or_id: &str, timeout_secs: Option<u32>) -> DockerResult {
        let timeout = timeout_secs.unwrap_or(10);
        let output = tokio::process::Command::new("docker")
            .args(["restart", "-t", &timeout.to_string(), name_or_id])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                DockerResult::success(format!("Container {} restarted", name_or_id)).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to restart container: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute docker restart: {}", e)),
        }
    }

    /// Get container logs.
    pub async fn container_logs(
        name_or_id: &str,
        lines: Option<usize>,
        since: Option<&str>,
    ) -> Result<String, String> {
        let mut args = vec!["logs".to_string()];

        if let Some(n) = lines {
            args.push("--tail".to_string());
            args.push(n.to_string());
        }

        if let Some(s) = since {
            args.push("--since".to_string());
            args.push(s.to_string());
        }

        args.push(name_or_id.to_string());

        let output = tokio::process::Command::new("docker")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("Failed to execute docker logs: {}", e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        // Docker logs can output to both stdout and stderr
        let mut logs = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            logs.push_str(&stderr);
        }

        Ok(logs)
    }
}

/// Docker Compose manager for service orchestration.
pub struct ComposeManager {
    /// Working directory for docker-compose commands.
    working_dir: String,
}

impl ComposeManager {
    /// Default compose working directory.
    const DEFAULT_DIR: &'static str = "/opt/spuff/compose";

    /// Create a new compose manager for the given directory.
    pub fn new(working_dir: Option<&str>) -> Self {
        Self {
            working_dir: working_dir.unwrap_or(Self::DEFAULT_DIR).to_string(),
        }
    }

    /// Check if docker-compose or docker compose is available.
    pub async fn is_available() -> bool {
        // Try docker compose (v2) first
        if tokio::process::Command::new("docker")
            .args(["compose", "version"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return true;
        }

        // Fall back to docker-compose (v1)
        tokio::process::Command::new("docker-compose")
            .arg("version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get compose command parts (handles v1 vs v2).
    async fn compose_command() -> Vec<String> {
        // Try docker compose (v2) first
        if tokio::process::Command::new("docker")
            .args(["compose", "version"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            vec!["docker".to_string(), "compose".to_string()]
        } else {
            vec!["docker-compose".to_string()]
        }
    }

    /// Check if compose file exists in working directory.
    pub fn has_compose_file(&self) -> bool {
        let dir = Path::new(&self.working_dir);
        dir.join("docker-compose.yml").exists()
            || dir.join("docker-compose.yaml").exists()
            || dir.join("compose.yml").exists()
            || dir.join("compose.yaml").exists()
    }

    /// List services defined in compose file.
    pub async fn list_services(&self) -> Result<Vec<ServiceStatus>, String> {
        if !self.has_compose_file() {
            return Ok(vec![]);
        }

        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![&cmd[1]])
        } else {
            (&cmd[0], vec![])
        };

        let output = tokio::process::Command::new(program)
            .args(args_prefix.iter().map(|s| s.as_str()))
            .args(["ps", "--format", "json"])
            .current_dir(&self.working_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to execute compose ps: {}", e))?;

        if !output.status.success() {
            // Try without --format json for older versions
            return self.list_services_legacy().await;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse JSON output (docker compose v2 outputs one JSON object per line)
        let services: Vec<ServiceStatus> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .map(|v| ServiceStatus {
                        name: v["Service"].as_str().unwrap_or("").to_string(),
                        container_id: v["ID"].as_str().map(|s| s.to_string()),
                        state: v["State"].as_str().unwrap_or("unknown").to_string(),
                        health: v["Health"].as_str().map(|s| s.to_string()),
                        ports: v["Publishers"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|p| {
                                        let published = p["PublishedPort"].as_u64()?;
                                        let target = p["TargetPort"].as_u64()?;
                                        Some(format!("{}:{}", published, target))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
            })
            .collect();

        Ok(services)
    }

    /// Legacy service listing for older docker-compose versions.
    async fn list_services_legacy(&self) -> Result<Vec<ServiceStatus>, String> {
        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![&cmd[1]])
        } else {
            (&cmd[0], vec![])
        };

        let output = tokio::process::Command::new(program)
            .args(args_prefix.iter().map(|s| s.as_str()))
            .args(["ps"])
            .current_dir(&self.working_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to execute compose ps: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse table output (skip header)
        let services: Vec<ServiceStatus> = stdout
            .lines()
            .skip(1) // Skip header
            .filter(|line| !line.is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                ServiceStatus {
                    name: parts.first().unwrap_or(&"").to_string(),
                    container_id: parts.get(1).map(|s| s.to_string()),
                    state: parts.get(2).unwrap_or(&"unknown").to_string(),
                    health: None,
                    ports: vec![],
                }
            })
            .collect();

        Ok(services)
    }

    /// Start all services or a specific service.
    pub async fn up(&self, service: Option<&str>, detach: bool) -> DockerResult {
        if !self.has_compose_file() {
            return DockerResult::failure("No compose file found in working directory");
        }

        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![cmd[1].as_str()])
        } else {
            (&cmd[0], vec![])
        };

        let mut args: Vec<&str> = args_prefix;
        args.push("up");
        if detach {
            args.push("-d");
        }

        if let Some(svc) = service {
            args.push(svc);
        }

        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&self.working_dir)
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let msg = if let Some(svc) = service {
                    format!("Service {} started", svc)
                } else {
                    "All services started".to_string()
                };
                DockerResult::success(msg).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to start services: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute compose up: {}", e)),
        }
    }

    /// Stop all services or a specific service.
    pub async fn down(&self, service: Option<&str>, remove_volumes: bool) -> DockerResult {
        if !self.has_compose_file() {
            return DockerResult::failure("No compose file found in working directory");
        }

        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![cmd[1].as_str()])
        } else {
            (&cmd[0], vec![])
        };

        let mut args: Vec<&str> = args_prefix;

        if service.is_some() {
            // For a specific service, use stop instead of down
            args.push("stop");
            if let Some(svc) = service {
                args.push(svc);
            }
        } else {
            args.push("down");
            if remove_volumes {
                args.push("-v");
            }
        }

        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&self.working_dir)
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let msg = if let Some(svc) = service {
                    format!("Service {} stopped", svc)
                } else {
                    "All services stopped".to_string()
                };
                DockerResult::success(msg).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to stop services: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute compose down: {}", e)),
        }
    }

    /// Restart a specific service or all services.
    pub async fn restart(&self, service: Option<&str>) -> DockerResult {
        if !self.has_compose_file() {
            return DockerResult::failure("No compose file found in working directory");
        }

        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![cmd[1].as_str()])
        } else {
            (&cmd[0], vec![])
        };

        let mut args: Vec<&str> = args_prefix;
        args.push("restart");

        if let Some(svc) = service {
            args.push(svc);
        }

        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&self.working_dir)
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let msg = if let Some(svc) = service {
                    format!("Service {} restarted", svc)
                } else {
                    "All services restarted".to_string()
                };
                DockerResult::success(msg).with_output(
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string(),
                )
            }
            Ok(o) => DockerResult::failure(format!(
                "Failed to restart services: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => DockerResult::failure(format!("Failed to execute compose restart: {}", e)),
        }
    }

    /// Get logs for a specific service or all services.
    pub async fn logs(
        &self,
        service: Option<&str>,
        lines: Option<usize>,
        since: Option<&str>,
    ) -> Result<String, String> {
        if !self.has_compose_file() {
            return Err("No compose file found in working directory".to_string());
        }

        let cmd = Self::compose_command().await;
        let (program, args_prefix) = if cmd.len() == 2 {
            (&cmd[0], vec![cmd[1].as_str()])
        } else {
            (&cmd[0], vec![])
        };

        let mut args: Vec<String> = args_prefix.iter().map(|s| s.to_string()).collect();
        args.push("logs".to_string());
        args.push("--no-color".to_string());

        if let Some(n) = lines {
            args.push("--tail".to_string());
            args.push(n.to_string());
        }

        if let Some(s) = since {
            args.push("--since".to_string());
            args.push(s.to_string());
        }

        if let Some(svc) = service {
            args.push(svc.to_string());
        }

        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&self.working_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to execute compose logs: {}", e))?;

        let mut logs = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            logs.push_str(&stderr);
        }

        Ok(logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_result_success() {
        let result = DockerResult::success("test message");
        assert!(result.success);
        assert_eq!(result.message, "test message");
    }

    #[test]
    fn test_docker_result_failure() {
        let result = DockerResult::failure("error message");
        assert!(!result.success);
        assert_eq!(result.message, "error message");
    }

    #[test]
    fn test_docker_result_with_output() {
        let result = DockerResult::success("test")
            .with_output("stdout content".to_string(), "stderr content".to_string());
        assert_eq!(result.stdout, Some("stdout content".to_string()));
        assert_eq!(result.stderr, Some("stderr content".to_string()));
    }

    #[test]
    fn test_compose_manager_default_dir() {
        let manager = ComposeManager::new(None);
        assert_eq!(manager.working_dir, "/opt/spuff/compose");
    }

    #[test]
    fn test_compose_manager_custom_dir() {
        let manager = ComposeManager::new(Some("/home/user/project"));
        assert_eq!(manager.working_dir, "/home/user/project");
    }
}
