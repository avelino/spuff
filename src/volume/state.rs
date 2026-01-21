//! Volume mount state tracking
//!
//! Defines types for tracking mounted volumes and their status.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Handle to a mounted volume
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountHandle {
    /// Unique identifier for this mount
    pub id: String,

    /// Driver used for the mount (e.g., "sshfs")
    pub driver: String,

    /// Source path (local, for sync drivers like rsync)
    pub source: String,

    /// Target path on the remote VM
    pub target: String,

    /// Local mount point where the remote directory is mounted
    pub mount_point: String,

    /// VM IP address (needed for unmount)
    pub vm_ip: Option<String>,

    /// SSH user on the VM
    pub ssh_user: Option<String>,

    /// Port used for SSH connection (if applicable)
    pub tunnel_port: Option<u16>,

    /// When the volume was mounted
    pub mounted_at: DateTime<Utc>,

    /// Whether the mount is read-only
    pub read_only: bool,
}

impl MountHandle {
    /// Create a new mount handle
    pub fn new(
        driver: impl Into<String>,
        target: impl Into<String>,
        mount_point: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            driver: driver.into(),
            source: String::new(),
            target: target.into(),
            mount_point: mount_point.into(),
            vm_ip: None,
            ssh_user: None,
            tunnel_port: None,
            mounted_at: Utc::now(),
            read_only: false,
        }
    }

    /// Set the source path (for sync drivers)
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Set the VM connection info
    pub fn with_vm_info(mut self, ip: impl Into<String>, user: impl Into<String>) -> Self {
        self.vm_ip = Some(ip.into());
        self.ssh_user = Some(user.into());
        self
    }

    /// Set the SSH port
    pub fn with_tunnel_port(mut self, port: u16) -> Self {
        self.tunnel_port = Some(port);
        self
    }

    /// Set read-only flag
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}

/// Status of a mounted volume
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MountStatus {
    /// Whether the volume is currently mounted
    pub mounted: bool,

    /// Whether the mount is healthy (accessible)
    pub healthy: bool,

    /// Latency to the mount point in milliseconds
    pub latency_ms: Option<u64>,

    /// Bytes read since mount (if available)
    pub bytes_read: Option<u64>,

    /// Bytes written since mount (if available)
    pub bytes_written: Option<u64>,

    /// Error message if unhealthy
    pub error: Option<String>,
}

impl MountStatus {
    /// Create a healthy mount status
    pub fn healthy() -> Self {
        Self {
            mounted: true,
            healthy: true,
            ..Default::default()
        }
    }

    /// Create an unhealthy mount status with error
    pub fn unhealthy(error: impl Into<String>) -> Self {
        Self {
            mounted: true,
            healthy: false,
            error: Some(error.into()),
            ..Default::default()
        }
    }

    /// Create a not-mounted status
    pub fn not_mounted() -> Self {
        Self::default()
    }

    /// Set latency
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }
}

/// Persistent state for volumes
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumeState {
    /// Currently active mount handles
    pub mounts: Vec<MountHandle>,
}

impl VolumeState {
    /// Load state from the default location
    pub fn load() -> Option<Self> {
        let state_dir = dirs::data_local_dir()?.join("spuff");
        let state_file = state_dir.join("volumes.json");

        if !state_file.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&state_file).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save state to the default location
    pub fn save(&self) -> std::io::Result<()> {
        let state_dir = dirs::data_local_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No data dir"))?
            .join("spuff");

        std::fs::create_dir_all(&state_dir)?;
        let state_file = state_dir.join("volumes.json");

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(state_file, content)
    }

    /// Add a mount handle
    pub fn add_mount(&mut self, handle: MountHandle) {
        self.mounts.push(handle);
    }

    /// Remove a mount handle by target path or mount point
    pub fn remove_mount(&mut self, path: &str) -> Option<MountHandle> {
        if let Some(pos) = self
            .mounts
            .iter()
            .position(|m| m.target == path || m.mount_point == path)
        {
            Some(self.mounts.remove(pos))
        } else {
            None
        }
    }

    /// Find a mount handle by target path or mount point
    pub fn find_mount(&self, path: &str) -> Option<&MountHandle> {
        self.mounts
            .iter()
            .find(|m| m.target == path || m.mount_point == path)
    }

    /// Find a mount handle by local mount point
    pub fn find_by_mount_point(&self, mount_point: &str) -> Option<&MountHandle> {
        self.mounts.iter().find(|m| m.mount_point == mount_point)
    }

    /// Clear all mounts
    pub fn clear(&mut self) {
        self.mounts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_handle_new() {
        // new(driver, target, mount_point)
        let handle = MountHandle::new("sshfs", "/home/dev/project", "/local/mnt");
        assert!(!handle.id.is_empty());
        assert_eq!(handle.driver, "sshfs");
        assert_eq!(handle.target, "/home/dev/project");
        assert_eq!(handle.mount_point, "/local/mnt");
        assert!(handle.source.is_empty());
        assert!(handle.tunnel_port.is_none());
        assert!(!handle.read_only);
    }

    #[test]
    fn test_mount_handle_with_vm_info() {
        let handle = MountHandle::new("sshfs", "/home/dev/project", "/local/mnt")
            .with_vm_info("192.168.1.100", "dev");
        assert_eq!(handle.vm_ip, Some("192.168.1.100".to_string()));
        assert_eq!(handle.ssh_user, Some("dev".to_string()));
    }

    #[test]
    fn test_mount_handle_with_source() {
        let handle =
            MountHandle::new("sshfs", "/home/dev/project", "/local/mnt").with_source("./src");
        assert_eq!(handle.source, "./src");
    }

    #[test]
    fn test_mount_status_healthy() {
        let status = MountStatus::healthy();
        assert!(status.mounted);
        assert!(status.healthy);
        assert!(status.error.is_none());
    }

    #[test]
    fn test_mount_status_unhealthy() {
        let status = MountStatus::unhealthy("Connection lost");
        assert!(status.mounted);
        assert!(!status.healthy);
        assert_eq!(status.error, Some("Connection lost".to_string()));
    }

    #[test]
    fn test_mount_status_with_latency() {
        let status = MountStatus::healthy().with_latency(25);
        assert_eq!(status.latency_ms, Some(25));
    }

    #[test]
    fn test_volume_state_add_remove() {
        let mut state = VolumeState::default();
        assert!(state.mounts.is_empty());

        let handle = MountHandle::new("sshfs", "/home/dev/project", "/local/mnt");
        state.add_mount(handle);
        assert_eq!(state.mounts.len(), 1);

        // Can remove by target
        let removed = state.remove_mount("/home/dev/project");
        assert!(removed.is_some());
        assert!(state.mounts.is_empty());
    }

    #[test]
    fn test_volume_state_remove_by_mount_point() {
        let mut state = VolumeState::default();
        state.add_mount(MountHandle::new("sshfs", "/home/dev/project", "/local/mnt"));

        // Can also remove by mount_point
        let removed = state.remove_mount("/local/mnt");
        assert!(removed.is_some());
        assert!(state.mounts.is_empty());
    }

    #[test]
    fn test_volume_state_find_mount() {
        let mut state = VolumeState::default();
        state.add_mount(MountHandle::new("sshfs", "/home/dev/project", "/local/mnt"));

        // Find by target
        assert!(state.find_mount("/home/dev/project").is_some());
        // Find by mount_point
        assert!(state.find_mount("/local/mnt").is_some());
        assert!(state.find_mount("/nonexistent").is_none());
    }

    #[test]
    fn test_volume_state_find_by_mount_point() {
        let mut state = VolumeState::default();
        state.add_mount(MountHandle::new("sshfs", "/home/dev/project", "/local/mnt"));

        assert!(state.find_by_mount_point("/local/mnt").is_some());
        assert!(state.find_by_mount_point("/home/dev/project").is_none());
    }
}
