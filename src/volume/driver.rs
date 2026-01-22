//! Volume driver trait definition
//!
//! Defines the interface that all volume drivers must implement.

use async_trait::async_trait;

use crate::error::Result;

use super::config::VolumeConfig;
use super::state::{MountHandle, MountStatus};

/// Trait that all volume drivers must implement
///
/// Volume drivers are responsible for mounting and unmounting
/// volumes using their specific protocols (SSHFS, NFS, etc.).
#[async_trait]
pub trait VolumeDriver: Send + Sync {
    /// Returns the driver name (e.g., "sshfs", "nfs")
    fn name(&self) -> &'static str;

    /// Returns a human-readable description of the driver
    fn description(&self) -> &'static str;

    /// Check if this driver is available on the system
    ///
    /// This checks for required binaries and dependencies.
    async fn is_available(&self) -> bool;

    /// Mount a volume according to the configuration
    ///
    /// # Arguments
    /// * `config` - The volume configuration
    /// * `tunnel_port` - Optional port for reverse SSH tunnel (used by SSHFS)
    /// * `remote_user` - Username on the remote VM
    ///
    /// # Returns
    /// A mount handle that can be used to manage the mount
    async fn mount(
        &self,
        config: &VolumeConfig,
        tunnel_port: Option<u16>,
        remote_user: &str,
    ) -> Result<MountHandle>;

    /// Unmount a volume
    ///
    /// # Arguments
    /// * `handle` - The mount handle returned from `mount()`
    async fn unmount(&self, handle: &MountHandle) -> Result<()>;

    /// Get the current status of a mount
    ///
    /// # Arguments
    /// * `handle` - The mount handle to check
    async fn status(&self, handle: &MountHandle) -> Result<MountStatus>;

    /// Remount a volume (useful after connection issues)
    ///
    /// This is a convenience method that unmounts and remounts.
    /// Drivers may override this with a more efficient implementation.
    async fn remount(&self, handle: &MountHandle, config: &VolumeConfig) -> Result<MountHandle> {
        // Default implementation: unmount and mount again
        self.unmount(handle).await.ok(); // Ignore unmount errors

        self.mount(config, handle.tunnel_port, "dev").await
    }

    /// Returns the packages required on the remote VM for this driver
    ///
    /// These packages will be installed via cloud-init or the agent.
    fn required_packages(&self) -> Vec<&'static str>;

    /// Returns the packages required on the local machine for this driver
    fn required_local_packages(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Check if a mount is still active
    ///
    /// Default implementation checks the mount status.
    async fn is_mounted(&self, handle: &MountHandle) -> bool {
        match self.status(handle).await {
            Ok(status) => status.mounted,
            Err(_) => false,
        }
    }
}

/// Information about a volume driver
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// Driver name
    pub name: &'static str,

    /// Human-readable description
    pub description: &'static str,

    /// Required packages on remote VM
    pub remote_packages: Vec<&'static str>,

    /// Required packages on local machine
    pub local_packages: Vec<&'static str>,

    /// Whether the driver is available on this system
    pub available: bool,
}

impl DriverInfo {
    /// Create driver info from a driver instance
    pub async fn from_driver<D: VolumeDriver + ?Sized>(driver: &D) -> Self {
        Self {
            name: driver.name(),
            description: driver.description(),
            remote_packages: driver.required_packages(),
            local_packages: driver.required_local_packages(),
            available: driver.is_available().await,
        }
    }
}
