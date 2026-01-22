//! Volume management for the spuff-agent.
//!
//! Provides functionality to list, monitor, and manage SSHFS mounts on the VM.
//! This is a simplified implementation for the agent side - the full VolumeManager
//! with driver abstraction lives in the CLI.

use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;

/// Information about a mounted volume
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    /// Mount source (e.g., user@localhost:/path)
    pub source: String,
    /// Mount target path on this VM
    pub target: String,
    /// Filesystem type (e.g., fuse.sshfs)
    pub fs_type: String,
    /// Mount options
    pub options: Vec<String>,
}

/// Status of a volume mount
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeStatus {
    /// Mount information
    pub mount: MountInfo,
    /// Whether the mount is currently accessible
    pub accessible: bool,
    /// Latency in milliseconds to access the mount (if accessible)
    pub latency_ms: Option<u64>,
    /// Error message if not accessible
    pub error: Option<String>,
}

/// Agent-side volume manager
///
/// Manages volume mounts on the remote VM. This is a simplified implementation
/// that interacts with the local filesystem and mount commands.
pub struct AgentVolumeManager;

impl AgentVolumeManager {
    /// Create a new agent volume manager
    pub fn new() -> Self {
        Self
    }

    /// List all SSHFS mounts on this VM
    pub async fn list_mounts(&self) -> Vec<MountInfo> {
        self.parse_mounts()
            .await
            .into_iter()
            .filter(|m| m.fs_type == "fuse.sshfs")
            .collect()
    }

    /// Get detailed status of all SSHFS mounts
    pub async fn get_status(&self) -> Vec<VolumeStatus> {
        let mounts = self.list_mounts().await;
        let mut statuses = Vec::with_capacity(mounts.len());

        for mount in mounts {
            let status = self.check_mount_status(&mount).await;
            statuses.push(status);
        }

        statuses
    }

    /// Check if a specific target is mounted
    pub async fn is_mounted(&self, target: &str) -> bool {
        let mounts = self.list_mounts().await;
        mounts.iter().any(|m| m.target == target)
    }

    /// Unmount a volume
    pub async fn unmount(&self, target: &str) -> Result<(), String> {
        if !self.is_mounted(target).await {
            return Err(format!("Mount not found: {}", target));
        }

        // Try fusermount first, then regular umount
        let output = Command::new("fusermount")
            .args(["-u", target])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                tracing::info!("Successfully unmounted {} with fusermount", target);
                Ok(())
            }
            _ => {
                // Fallback to sudo umount
                let output = Command::new("sudo")
                    .args(["umount", target])
                    .output()
                    .await
                    .map_err(|e| format!("Failed to execute umount: {}", e))?;

                if output.status.success() {
                    tracing::info!("Successfully unmounted {} with sudo umount", target);
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!("Failed to unmount: {}", stderr.trim()))
                }
            }
        }
    }

    /// Parse /proc/mounts to get mount information
    async fn parse_mounts(&self) -> Vec<MountInfo> {
        let content = match tokio::fs::read_to_string("/proc/mounts").await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read /proc/mounts: {}", e);
                return Vec::new();
            }
        };

        content
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    Some(MountInfo {
                        source: parts[0].to_string(),
                        target: parts[1].to_string(),
                        fs_type: parts[2].to_string(),
                        options: parts[3].split(',').map(String::from).collect(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check the status of a mount
    async fn check_mount_status(&self, mount: &MountInfo) -> VolumeStatus {
        let start = Instant::now();

        // Try to stat the mount point to check accessibility
        let result = Command::new("stat")
            .arg(&mount.target)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await;

        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) if output.status.success() => VolumeStatus {
                mount: mount.clone(),
                accessible: true,
                latency_ms: Some(latency_ms),
                error: None,
            },
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                VolumeStatus {
                    mount: mount.clone(),
                    accessible: false,
                    latency_ms: None,
                    error: Some(stderr.trim().to_string()),
                }
            }
            Err(e) => VolumeStatus {
                mount: mount.clone(),
                accessible: false,
                latency_ms: None,
                error: Some(format!("Failed to check status: {}", e)),
            },
        }
    }
}

impl Default for AgentVolumeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_info_serialization() {
        let info = MountInfo {
            source: "user@localhost:/home/user/project".to_string(),
            target: "/mnt/project".to_string(),
            fs_type: "fuse.sshfs".to_string(),
            options: vec!["rw".to_string(), "user_id=1000".to_string()],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("user@localhost"));
        assert!(json.contains("/mnt/project"));
        assert!(json.contains("fuse.sshfs"));
    }

    #[test]
    fn test_volume_status_serialization() {
        let status = VolumeStatus {
            mount: MountInfo {
                source: "user@localhost:/path".to_string(),
                target: "/mnt/test".to_string(),
                fs_type: "fuse.sshfs".to_string(),
                options: vec!["rw".to_string()],
            },
            accessible: true,
            latency_ms: Some(25),
            error: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"accessible\":true"));
        assert!(json.contains("\"latency_ms\":25"));
    }
}
