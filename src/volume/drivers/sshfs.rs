//! SSHFS volume driver implementation
//!
//! Mounts remote VM directories locally using SSHFS.
//! This allows local editors to work with files that live on the VM.
//!
//! # Flow
//! 1. User runs `spuff volume mount`
//! 2. Local machine runs SSHFS to mount VM directory locally
//! 3. Edits in local editor go directly to VM
//!
//! # Requirements
//! - macOS: macFUSE + SSHFS (brew install macfuse sshfs)
//! - Linux: fuse + sshfs (apt install sshfs)

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{Result, SpuffError};
use crate::ssh::{SshClient, SshConfig};
use crate::volume::config::VolumeConfig;
use crate::volume::driver::VolumeDriver;
use crate::volume::state::{MountHandle, MountStatus};

/// Maximum number of retries for network operations
const MAX_RETRIES: u32 = 3;

/// Delay between retries (exponential backoff base)
const RETRY_BASE_DELAY_MS: u64 = 1000;

/// Validate a path is safe for shell interpolation
///
/// Prevents command injection by rejecting paths containing shell metacharacters.
/// This is a defense-in-depth measure - paths should also be properly quoted.
fn validate_shell_safe_path(path: &str) -> Result<()> {
    // Check for shell metacharacters that could enable command injection
    let dangerous_chars = [
        '"', '`', '$', '\n', '\r', '\0', '\'', ';', '|', '&', '>', '<',
    ];

    for ch in dangerous_chars {
        if path.contains(ch) {
            return Err(SpuffError::Volume(format!(
                "Path contains invalid character '{}': {}",
                ch.escape_default(),
                path
            )));
        }
    }

    // Also check for command substitution patterns
    if path.contains("$(") || path.contains("${") {
        return Err(SpuffError::Volume(format!(
            "Path contains shell expansion pattern: {}",
            path
        )));
    }

    Ok(())
}

/// Validate an IP address format (basic check)
fn validate_ip_address(ip: &str) -> Result<()> {
    // Allow IPv4 and IPv6 addresses, as well as hostnames
    // Reject anything with shell metacharacters
    validate_shell_safe_path(ip)?;

    // Basic format validation
    if ip.is_empty() {
        return Err(SpuffError::Volume("Empty IP address".to_string()));
    }

    Ok(())
}

/// Validate a username format
fn validate_username(user: &str) -> Result<()> {
    validate_shell_safe_path(user)?;

    if user.is_empty() {
        return Err(SpuffError::Volume("Empty username".to_string()));
    }

    // Usernames should be alphanumeric with underscores/hyphens
    if !user
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(SpuffError::Volume(format!(
            "Invalid characters in username: {}",
            user
        )));
    }

    Ok(())
}

/// Execute an async operation with retry logic and exponential backoff
async fn with_retry<T, F, Fut>(operation_name: &str, max_retries: u32, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_error = None;

    for attempt in 0..max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);

                if attempt + 1 < max_retries {
                    let delay = Duration::from_millis(RETRY_BASE_DELAY_MS * 2u64.pow(attempt));
                    tracing::debug!(
                        "{} failed (attempt {}/{}), retrying in {:?}...",
                        operation_name,
                        attempt + 1,
                        max_retries,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| SpuffError::Volume(format!("{} failed", operation_name))))
}

/// SSHFS volume driver
///
/// Mounts remote VM directories locally using SSHFS over SSH.
#[derive(Debug, Clone, Default)]
pub struct SshfsDriver;

impl SshfsDriver {
    /// Create a new SSHFS driver instance
    pub fn new() -> Self {
        Self
    }

    /// Check if SSHFS is installed locally
    pub async fn check_sshfs_installed() -> bool {
        Command::new("which")
            .arg("sshfs")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Check if macFUSE/FUSE is available
    pub async fn check_fuse_available() -> bool {
        #[cfg(target_os = "macos")]
        {
            // Check for macFUSE
            std::path::Path::new("/Library/Filesystems/macfuse.fs").exists()
                || std::path::Path::new("/usr/local/lib/libfuse.dylib").exists()
        }

        #[cfg(target_os = "linux")]
        {
            std::path::Path::new("/dev/fuse").exists()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            false
        }
    }
}

#[async_trait]
impl VolumeDriver for SshfsDriver {
    fn name(&self) -> &'static str {
        "sshfs"
    }

    fn description(&self) -> &'static str {
        "SSH Filesystem - mount remote VM directories locally"
    }

    async fn is_available(&self) -> bool {
        Self::check_sshfs_installed().await && Self::check_fuse_available().await
    }

    async fn mount(
        &self,
        config: &VolumeConfig,
        _tunnel_port: Option<u16>,
        _remote_user: &str,
    ) -> Result<MountHandle> {
        // This method prepares the mount handle
        // The actual mount is done by SshfsLocalCommands::mount()
        // Note: project_base_dir is None here as this is called without project context
        let mount_point = config.resolve_mount_point(None, None);

        let handle = MountHandle::new(self.name(), &config.target, &mount_point)
            .with_read_only(config.read_only);

        tracing::debug!(
            "Prepared SSHFS mount: VM:{} -> Local:{}",
            config.target,
            mount_point
        );

        Ok(handle)
    }

    async fn unmount(&self, handle: &MountHandle) -> Result<()> {
        SshfsLocalCommands::unmount(&handle.mount_point).await
    }

    async fn status(&self, handle: &MountHandle) -> Result<MountStatus> {
        SshfsLocalCommands::check_status(&handle.mount_point).await
    }

    fn required_packages(&self) -> Vec<&'static str> {
        // No packages needed on the VM for this direction
        vec![]
    }

    fn required_local_packages(&self) -> Vec<&'static str> {
        #[cfg(target_os = "macos")]
        {
            vec!["macfuse", "sshfs"]
        }

        #[cfg(target_os = "linux")]
        {
            vec!["sshfs", "fuse3"]
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            vec!["sshfs"]
        }
    }
}

/// Commands to execute locally for SSHFS operations
pub struct SshfsLocalCommands;

impl SshfsLocalCommands {
    /// Mount a remote directory locally using SSHFS
    ///
    /// # Arguments
    /// * `vm_ip` - IP address of the VM
    /// * `ssh_user` - SSH username on the VM
    /// * `ssh_key_path` - Path to SSH private key
    /// * `remote_path` - Path on the VM to mount
    /// * `local_mount_point` - Local directory to mount to
    /// * `read_only` - Mount as read-only
    /// * `options` - Additional mount options
    pub async fn mount(
        vm_ip: &str,
        ssh_user: &str,
        ssh_key_path: &str,
        remote_path: &str,
        local_mount_point: &str,
        read_only: bool,
        options: &crate::volume::config::VolumeOptions,
    ) -> Result<()> {
        // Validate all inputs to prevent command injection
        validate_ip_address(vm_ip)?;
        validate_username(ssh_user)?;
        validate_shell_safe_path(ssh_key_path)?;
        validate_shell_safe_path(remote_path)?;
        validate_shell_safe_path(local_mount_point)?;

        // Verify SSH key exists
        if !std::path::Path::new(ssh_key_path).exists() {
            return Err(SpuffError::Volume(format!(
                "SSH key not found: {}",
                ssh_key_path
            )));
        }

        // Create mount point directory using tokio::fs for async
        tokio::fs::create_dir_all(local_mount_point)
            .await
            .map_err(|e| {
                SpuffError::Volume(format!(
                    "Failed to create mount point {}: {}",
                    local_mount_point, e
                ))
            })?;

        // Ensure remote directory exists with retry logic
        with_retry("Create remote directory", MAX_RETRIES, || {
            Self::ensure_remote_dir_exists_inner(vm_ip, ssh_user, ssh_key_path, remote_path)
        })
        .await?;

        // SSHFS 2.x has issues with paths containing spaces in IdentityFile option.
        // Create a wrapper script to handle SSH options properly.
        let wrapper_path = Self::create_ssh_wrapper(ssh_key_path, options)?;

        let mut args = vec![
            "-o".to_string(),
            format!("ssh_command={}", wrapper_path.display()),
        ];

        // Add reconnect option
        if options.reconnect {
            args.extend(["-o".to_string(), "reconnect".to_string()]);
        }

        // Add compression
        if options.compression {
            args.extend(["-o".to_string(), "compression=yes".to_string()]);
        }

        // Add read-only
        if read_only {
            args.extend(["-o".to_string(), "ro".to_string()]);
        }

        // Add caching options
        if options.cache {
            args.extend([
                "-o".to_string(),
                "cache=yes".to_string(),
                "-o".to_string(),
                "kernel_cache".to_string(),
                "-o".to_string(),
                "auto_cache".to_string(),
            ]);
        }

        // macOS specific options
        #[cfg(target_os = "macos")]
        {
            // Disable extended attributes to avoid issues
            args.extend(["-o".to_string(), "noapplexattr".to_string()]);

            // Set volume name for Finder
            let volume_name = std::path::Path::new(remote_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "spuff".to_string());
            args.extend(["-o".to_string(), format!("volname={}", volume_name)]);
        }

        // Source (remote path)
        args.push(format!("{}@{}:{}", ssh_user, vm_ip, remote_path));

        // Target (local mount point)
        args.push(local_mount_point.to_string());

        tracing::debug!("Running: sshfs {:?}", args);

        let output = Command::new("sshfs")
            .args(&args)
            .output()
            .await
            .map_err(|e| SpuffError::Volume(format!("Failed to execute sshfs: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Provide helpful error messages
            if stderr.contains("mount_macfuse") || stderr.contains("fuse") {
                return Err(SpuffError::Volume(
                    "macFUSE is not installed or not loaded.\n\n\
                     Install with: brew install macfuse\n\
                     Then restart your computer.\n\n\
                     If already installed, you may need to allow the kernel extension in:\n\
                     System Settings > Privacy & Security"
                        .to_string(),
                ));
            }

            if stderr.contains("Permission denied") {
                return Err(SpuffError::Volume(format!(
                    "SSH authentication failed. Check that:\n\
                     - The VM is running\n\
                     - SSH key is correct: {}\n\
                     - User '{}' exists on the VM\n\n\
                     Error: {}",
                    ssh_key_path, ssh_user, stderr
                )));
            }

            return Err(SpuffError::Volume(format!(
                "Failed to mount SSHFS: {}",
                stderr
            )));
        }

        tracing::debug!(
            "Mounted {}@{}:{} at {}",
            ssh_user,
            vm_ip,
            remote_path,
            local_mount_point
        );

        Ok(())
    }

    /// Create a wrapper script for SSH to handle paths with spaces
    /// SSHFS 2.x cannot properly handle IdentityFile paths with spaces
    fn create_ssh_wrapper(
        ssh_key_path: &str,
        options: &crate::volume::config::VolumeOptions,
    ) -> Result<std::path::PathBuf> {
        // Input validation already done in mount(), but double-check for safety
        validate_shell_safe_path(ssh_key_path)?;

        // Use ~/.spuff/ssh-wrappers to avoid paths with spaces
        // IMPORTANT: SSHFS cannot handle spaces in the ssh_command path
        // macOS dirs::data_local_dir() returns "~/Library/Application Support" which has a space
        // Using home_dir ensures no spaces (home directories rarely have spaces)
        let wrapper_dir = dirs::home_dir()
            .ok_or_else(|| {
                SpuffError::Volume("Could not determine home directory for SSH wrapper".into())
            })?
            .join(".spuff")
            .join("ssh-wrappers");

        // Create directory with restricted permissions
        std::fs::create_dir_all(&wrapper_dir).map_err(|e| {
            SpuffError::Volume(format!("Failed to create SSH wrapper directory: {}", e))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Set directory permissions to 700 (owner only)
            std::fs::set_permissions(&wrapper_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| {
                    SpuffError::Volume(format!("Failed to secure SSH wrapper directory: {}", e))
                })?;
        }

        // Use a hash of the key path to create a unique wrapper per key
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            ssh_key_path.hash(&mut hasher);
            hasher.finish()
        };

        let wrapper_path = wrapper_dir.join(format!("ssh-wrapper-{:x}.sh", hash));

        // Use single quotes for the path in the script to prevent expansion
        // The path has already been validated to not contain single quotes
        let script_content = format!(
            r#"#!/bin/bash
exec ssh -i '{}' -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o ServerAliveInterval={} -o ServerAliveCountMax={} "$@"
"#,
            ssh_key_path, options.server_alive_interval, options.server_alive_count_max
        );

        std::fs::write(&wrapper_path, &script_content).map_err(|e| {
            SpuffError::Volume(format!("Failed to write SSH wrapper script: {}", e))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Set script permissions to 700 (owner execute only)
            std::fs::set_permissions(&wrapper_path, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| {
                    SpuffError::Volume(format!(
                        "Failed to set permissions on SSH wrapper script: {}",
                        e
                    ))
                })?;
        }

        tracing::debug!("Created SSH wrapper script: {}", wrapper_path.display());

        Ok(wrapper_path)
    }

    /// Ensure the remote directory exists before mounting (public API with validation)
    /// SSHFS fails silently with "remote host has disconnected" if the directory doesn't exist
    pub async fn ensure_remote_dir_exists(
        vm_ip: &str,
        ssh_user: &str,
        ssh_key_path: &str,
        remote_path: &str,
    ) -> Result<()> {
        // Validate all inputs
        validate_ip_address(vm_ip)?;
        validate_username(ssh_user)?;
        validate_shell_safe_path(ssh_key_path)?;
        validate_shell_safe_path(remote_path)?;

        Self::ensure_remote_dir_exists_inner(vm_ip, ssh_user, ssh_key_path, remote_path).await
    }

    /// Inner implementation without validation (for use with retry logic)
    async fn ensure_remote_dir_exists_inner(
        vm_ip: &str,
        ssh_user: &str,
        ssh_key_path: &str,
        remote_path: &str,
    ) -> Result<()> {
        // Skip if path is empty or just root
        if remote_path.is_empty() || remote_path == "/" {
            return Ok(());
        }

        // Use internal SSH implementation
        let config = SshConfig::new(ssh_user, PathBuf::from(ssh_key_path));
        let client = SshClient::connect(vm_ip, 22, &config)
            .await
            .map_err(|e| SpuffError::Volume(format!("Failed to connect via SSH: {}", e)))?;

        let command = format!("mkdir -p '{}'", remote_path.replace('\'', "'\\''"));
        let output = client.exec(&command).await.map_err(|e| {
            SpuffError::Volume(format!("Failed to create remote directory via SSH: {}", e))
        })?;

        if !output.success {
            return Err(SpuffError::Volume(format!(
                "Failed to create remote directory '{}': {}",
                remote_path, output.stderr
            )));
        }

        tracing::debug!("Ensured remote directory exists: {}", remote_path);
        Ok(())
    }

    /// Unmount a local SSHFS mount point
    pub async fn unmount(mount_point: &str) -> Result<()> {
        // Validate mount point to prevent command injection
        validate_shell_safe_path(mount_point)?;

        // Check if mounted first
        if !Self::is_mounted(mount_point).await {
            tracing::debug!(
                "Mount point {} is not mounted, skipping unmount",
                mount_point
            );
            return Ok(());
        }

        // Try normal unmount first
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("umount")
                .arg(mount_point)
                .output()
                .await
                .map_err(|e| SpuffError::Volume(format!("Failed to execute umount: {}", e)))?;

            if output.status.success() {
                tracing::debug!("Unmounted {}", mount_point);
                return Ok(());
            }

            // Try force unmount with umount -f
            let force_output = Command::new("umount")
                .args(["-f", mount_point])
                .output()
                .await;

            if let Ok(out) = force_output {
                if out.status.success() {
                    tracing::debug!("Force unmounted {} with umount -f", mount_point);
                    return Ok(());
                }
            }

            // Last resort: diskutil unmount force
            let diskutil_output = Command::new("diskutil")
                .args(["unmount", "force", mount_point])
                .output()
                .await;

            if let Ok(out) = diskutil_output {
                if out.status.success() {
                    tracing::debug!("Force unmounted {} with diskutil", mount_point);
                    return Ok(());
                }
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SpuffError::Volume(format!(
                "Failed to unmount {}: {}",
                mount_point, stderr
            )))
        }

        #[cfg(target_os = "linux")]
        {
            // Try normal unmount first
            let output = Command::new("fusermount")
                .args(["-u", mount_point])
                .output()
                .await
                .map_err(|e| SpuffError::Volume(format!("Failed to execute fusermount: {}", e)))?;

            if output.status.success() {
                tracing::debug!("Unmounted {}", mount_point);
                return Ok(());
            }

            // Try lazy unmount (detaches immediately, cleans up when not busy)
            let lazy_output = Command::new("fusermount")
                .args(["-uz", mount_point])
                .output()
                .await;

            if let Ok(out) = lazy_output {
                if out.status.success() {
                    tracing::debug!("Lazy unmounted {} with fusermount -uz", mount_point);
                    return Ok(());
                }
            }

            // Try umount -l as last resort
            let umount_lazy = Command::new("umount")
                .args(["-l", mount_point])
                .output()
                .await;

            if let Ok(out) = umount_lazy {
                if out.status.success() {
                    tracing::debug!("Lazy unmounted {} with umount -l", mount_point);
                    return Ok(());
                }
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SpuffError::Volume(format!(
                "Failed to unmount {}: {}",
                mount_point, stderr
            )))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(SpuffError::Volume(
                "Unmount not supported on this platform".to_string(),
            ))
        }
    }

    /// Check if a path is currently mounted
    pub async fn is_mounted(mount_point: &str) -> bool {
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("mount").output().await.ok();

            if let Some(out) = output {
                let stdout = String::from_utf8_lossy(&out.stdout);
                return stdout.contains(mount_point);
            }
        }

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("mountpoint")
                .arg("-q")
                .arg(mount_point)
                .status()
                .await
                .ok();

            if let Some(status) = output {
                return status.success();
            }
        }

        false
    }

    /// Check mount status and health
    pub async fn check_status(mount_point: &str) -> Result<MountStatus> {
        if !Self::is_mounted(mount_point).await {
            return Ok(MountStatus::not_mounted());
        }

        // Try to access the mount point to check health
        let start = Instant::now();
        let accessible = tokio::fs::read_dir(mount_point).await.is_ok();
        let latency = start.elapsed().as_millis() as u64;

        if accessible {
            Ok(MountStatus::healthy().with_latency(latency))
        } else {
            Ok(MountStatus::unhealthy("Mount point not accessible"))
        }
    }
}

/// Get installation instructions for SSHFS
pub fn get_install_instructions() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "SSHFS is not installed.\n\n\
         Install with Homebrew:\n\
         brew install macfuse sshfs\n\n\
         After installation, restart your computer.\n\
         You may need to allow the kernel extension in:\n\
         System Settings > Privacy & Security"
    }

    #[cfg(target_os = "linux")]
    {
        "SSHFS is not installed.\n\n\
         Install with your package manager:\n\
         Ubuntu/Debian: sudo apt install sshfs\n\
         Fedora: sudo dnf install fuse-sshfs\n\
         Arch: sudo pacman -S sshfs"
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "SSHFS is not available on this platform."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sshfs_driver_name() {
        let driver = SshfsDriver::new();
        assert_eq!(driver.name(), "sshfs");
    }

    #[test]
    fn test_sshfs_driver_description() {
        let driver = SshfsDriver::new();
        assert!(driver.description().contains("remote"));
    }

    #[test]
    fn test_sshfs_required_local_packages() {
        let driver = SshfsDriver::new();
        let packages = driver.required_local_packages();
        assert!(packages.contains(&"sshfs"));
    }

    #[tokio::test]
    async fn test_check_sshfs_installed() {
        // This test just verifies the function runs without panic
        let _ = SshfsDriver::check_sshfs_installed().await;
    }

    #[tokio::test]
    async fn test_check_fuse_available() {
        // This test just verifies the function runs without panic
        let _ = SshfsDriver::check_fuse_available().await;
    }

    #[test]
    fn test_mount_status_not_mounted() {
        let status = MountStatus::not_mounted();
        assert!(!status.mounted);
        assert!(!status.healthy);
    }

    #[test]
    fn test_mount_status_healthy_with_latency() {
        let status = MountStatus::healthy().with_latency(42);
        assert!(status.mounted);
        assert!(status.healthy);
        assert_eq!(status.latency_ms, Some(42));
    }

    // Security validation tests
    #[test]
    fn test_validate_shell_safe_path_valid() {
        assert!(validate_shell_safe_path("/home/user/project").is_ok());
        assert!(validate_shell_safe_path("/tmp/test-file_123").is_ok());
        assert!(validate_shell_safe_path("./relative/path").is_ok());
        assert!(validate_shell_safe_path("~/.ssh/id_ed25519").is_ok());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_double_quotes() {
        assert!(validate_shell_safe_path("/path/with\"quote").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_backticks() {
        assert!(validate_shell_safe_path("/path/with`command`").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_dollar_sign() {
        assert!(validate_shell_safe_path("/path/with$var").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_command_substitution() {
        assert!(validate_shell_safe_path("/path/$(whoami)").is_err());
        assert!(validate_shell_safe_path("/path/${HOME}").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_semicolon() {
        assert!(validate_shell_safe_path("/path; rm -rf /").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_pipe() {
        assert!(validate_shell_safe_path("/path | cat /etc/passwd").is_err());
    }

    #[test]
    fn test_validate_shell_safe_path_rejects_newlines() {
        assert!(validate_shell_safe_path("/path\nwith\nnewlines").is_err());
    }

    #[test]
    fn test_validate_ip_address_valid() {
        assert!(validate_ip_address("192.168.1.100").is_ok());
        assert!(validate_ip_address("10.0.0.1").is_ok());
        assert!(validate_ip_address("example.com").is_ok());
    }

    #[test]
    fn test_validate_ip_address_rejects_empty() {
        assert!(validate_ip_address("").is_err());
    }

    #[test]
    fn test_validate_ip_address_rejects_injection() {
        assert!(validate_ip_address("192.168.1.1; rm -rf /").is_err());
    }

    #[test]
    fn test_validate_username_valid() {
        assert!(validate_username("dev").is_ok());
        assert!(validate_username("user_name").is_ok());
        assert!(validate_username("user-name").is_ok());
        assert!(validate_username("user.name").is_ok());
    }

    #[test]
    fn test_validate_username_rejects_empty() {
        assert!(validate_username("").is_err());
    }

    #[test]
    fn test_validate_username_rejects_special_chars() {
        assert!(validate_username("user;root").is_err());
        assert!(validate_username("user`whoami`").is_err());
        assert!(validate_username("user$USER").is_err());
    }

    #[test]
    fn test_validate_username_rejects_spaces() {
        assert!(validate_username("user name").is_err());
    }
}
