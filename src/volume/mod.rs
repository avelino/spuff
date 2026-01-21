//! Volume mounting module for spuff
//!
//! Provides an extensible architecture for mounting local directories on remote VMs.
//! Currently supports SSHFS with plans for NFS, 9P, and other protocols.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      VolumeManager                           │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │                  trait VolumeDriver                   │  │
//! │  │  + mount(config) -> MountHandle                       │  │
//! │  │  + unmount(handle) -> Result<()>                      │  │
//! │  │  + status(handle) -> MountStatus                      │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! │                              │                               │
//! │         ┌────────────────────┼────────────────────┐         │
//! │         ▼                    ▼                    ▼         │
//! │  ┌─────────────┐     ┌─────────────┐      ┌─────────────┐   │
//! │  │ SshfsDriver │     │  NfsDriver  │      │  9PDriver   │   │
//! │  │ (v1 - now)  │     │  (future)   │      │  (future)   │   │
//! │  └─────────────┘     └─────────────┘      └─────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```yaml
//! # spuff.yaml
//! volumes:
//!   - source: ./src
//!     target: /home/dev/project
//!   - source: ~/.config/nvim
//!     target: /home/dev/.config/nvim
//!     read_only: true
//! ```

pub mod config;
pub mod driver;
pub mod drivers;
pub mod state;

pub use config::{VolumeConfig, VolumeOptions, VolumeType};
pub use driver::{DriverInfo, VolumeDriver};
pub use drivers::{get_install_instructions, SshfsDriver, SshfsLocalCommands};
pub use state::{MountHandle, MountStatus, VolumeState};

use std::collections::HashMap;

use crate::error::{Result, SpuffError};

/// Manages volume mounting operations
///
/// The VolumeManager maintains a registry of available drivers and
/// provides a unified interface for mounting/unmounting volumes.
pub struct VolumeManager {
    /// Registered volume drivers
    drivers: HashMap<VolumeType, Box<dyn VolumeDriver>>,

    /// Current volume state (persisted)
    state: VolumeState,
}

impl Default for VolumeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeManager {
    /// Create a new VolumeManager with default drivers
    pub fn new() -> Self {
        let mut drivers: HashMap<VolumeType, Box<dyn VolumeDriver>> = HashMap::new();

        // Register default drivers
        drivers.insert(VolumeType::Sshfs, Box::new(SshfsDriver::new()));

        // Load persisted state or create new (log errors but don't fail)
        let state = VolumeState::load_or_default();

        Self { drivers, state }
    }

    /// Create a new VolumeManager, returning error if state loading fails
    pub fn new_strict() -> Result<Self> {
        let mut drivers: HashMap<VolumeType, Box<dyn VolumeDriver>> = HashMap::new();
        drivers.insert(VolumeType::Sshfs, Box::new(SshfsDriver::new()));

        let state = VolumeState::load()?;

        Ok(Self { drivers, state })
    }

    /// Get a driver by volume type
    pub fn get_driver(&self, volume_type: &VolumeType) -> Option<&dyn VolumeDriver> {
        self.drivers.get(volume_type).map(|d| d.as_ref())
    }

    /// Get information about all registered drivers
    pub async fn get_driver_info(&self) -> Vec<DriverInfo> {
        let mut infos = Vec::new();

        for driver in self.drivers.values() {
            infos.push(DriverInfo::from_driver(driver.as_ref()).await);
        }

        infos
    }

    /// Get required packages for a list of volume configs
    pub fn get_required_packages(&self, volumes: &[VolumeConfig]) -> Vec<&'static str> {
        let mut packages = Vec::new();

        for volume in volumes {
            if let Some(driver) = self.drivers.get(&volume.driver_type) {
                packages.extend(driver.required_packages());
            }
        }

        packages.sort();
        packages.dedup();
        packages
    }

    /// Mount a single volume
    pub async fn mount(
        &mut self,
        config: &VolumeConfig,
        tunnel_port: Option<u16>,
        remote_user: &str,
    ) -> Result<MountHandle> {
        let driver = self.drivers.get(&config.driver_type).ok_or_else(|| {
            SpuffError::Volume(format!("Unknown driver type: {:?}", config.driver_type))
        })?;

        let handle = driver.mount(config, tunnel_port, remote_user).await?;

        // Save to state - propagate errors instead of ignoring
        self.state.add_mount(handle.clone());
        if let Err(e) = self.state.save() {
            tracing::error!("Failed to save volume state: {}. Mount may not persist.", e);
            // Still return success since mount itself succeeded
            // But log the error so it's visible
        }

        Ok(handle)
    }

    /// Mount all volumes from a list of configs
    pub async fn mount_all(
        &mut self,
        volumes: &[VolumeConfig],
        tunnel_port: Option<u16>,
        remote_user: &str,
    ) -> Vec<Result<MountHandle>> {
        let mut results = Vec::new();

        for volume in volumes {
            let result = self.mount(volume, tunnel_port, remote_user).await;

            if let Err(ref e) = result {
                tracing::warn!("Failed to mount {}: {}", volume.target, e);
            }

            results.push(result);
        }

        results
    }

    /// Unmount a volume by target path
    pub async fn unmount(&mut self, target: &str) -> Result<()> {
        let handle = self
            .state
            .find_mount(target)
            .ok_or_else(|| SpuffError::Volume(format!("Mount not found: {}", target)))?
            .clone();

        let driver_type: VolumeType = handle.driver.parse().unwrap_or_default();
        let driver = self
            .drivers
            .get(&driver_type)
            .ok_or_else(|| SpuffError::Volume(format!("Unknown driver type: {}", handle.driver)))?;

        driver.unmount(&handle).await?;

        // Remove from state - log errors but don't fail the unmount
        self.state.remove_mount(target);
        if let Err(e) = self.state.save() {
            tracing::error!("Failed to save volume state after unmount: {}", e);
        }

        Ok(())
    }

    /// Unmount all volumes
    pub async fn unmount_all(&mut self) -> Vec<Result<()>> {
        let targets: Vec<String> = self.state.mounts.iter().map(|m| m.target.clone()).collect();

        let mut results = Vec::new();

        for target in targets {
            results.push(self.unmount(&target).await);
        }

        results
    }

    /// Get status of all mounts
    pub async fn status_all(&self) -> Vec<(MountHandle, MountStatus)> {
        let mut statuses = Vec::new();

        for handle in &self.state.mounts {
            let driver_type: VolumeType = handle.driver.parse().unwrap_or_default();

            let status = if let Some(driver) = self.drivers.get(&driver_type) {
                driver.status(handle).await.unwrap_or_default()
            } else {
                MountStatus::default()
            };

            statuses.push((handle.clone(), status));
        }

        statuses
    }

    /// Get current mount handles
    pub fn get_mounts(&self) -> &[MountHandle] {
        &self.state.mounts
    }

    /// Clear all mount state (for cleanup)
    pub fn clear_state(&mut self) -> Result<()> {
        self.state.clear();
        self.state.save()
    }

    /// Clear all mount state, logging errors but not failing
    pub fn clear_state_silent(&mut self) {
        self.state.clear();
        if let Err(e) = self.state.save() {
            tracing::error!("Failed to clear volume state: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_manager_new() {
        let manager = VolumeManager::new();
        assert!(manager.get_driver(&VolumeType::Sshfs).is_some());
    }

    #[test]
    fn test_get_required_packages() {
        let manager = VolumeManager::new();
        let volumes = vec![VolumeConfig::new("./src", "/mnt/src")];

        let packages = manager.get_required_packages(&volumes);
        // With the new Local -> VM architecture, SSHFS runs locally
        // so no packages are needed on the VM
        assert!(packages.is_empty());
    }

    #[test]
    fn test_get_required_packages_dedup() {
        let manager = VolumeManager::new();
        let volumes = vec![
            VolumeConfig::new("./src", "/mnt/src"),
            VolumeConfig::new("./lib", "/mnt/lib"),
        ];

        let packages = manager.get_required_packages(&volumes);
        // Should not have duplicates (even though list is empty with SSHFS)
        let unique: std::collections::HashSet<_> = packages.iter().collect();
        assert_eq!(packages.len(), unique.len());
    }
}
