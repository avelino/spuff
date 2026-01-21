//! Volume configuration types for spuff.yaml
//!
//! Defines the configuration schema for volume mounts, supporting
//! multiple drivers (SSHFS, NFS, etc.) with driver-specific options.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Volume mount configuration from spuff.yaml
///
/// The SSHFS driver mounts the remote VM directory locally, allowing
/// local editors to work with files that live on the VM.
///
/// # Example
/// ```yaml
/// volumes:
///   - source: ./src              # Local path (for initial sync, optional)
///     target: /home/dev/project  # Path on the VM
///     mount_point: ~/mnt/project # Where to mount locally (auto-generated if omitted)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeConfig {
    /// Type of the volume driver (default: sshfs)
    #[serde(default, rename = "type")]
    pub driver_type: VolumeType,

    /// Source path on the local machine (used for initial sync with rsync driver)
    /// For sshfs-only usage, this can be omitted or set to same as mount_point
    #[serde(default)]
    pub source: String,

    /// Target path on the remote VM where files will be accessible
    pub target: String,

    /// Local mount point where the VM directory will appear
    /// If not specified, auto-generated under ~/.local/share/spuff/mounts/
    #[serde(default)]
    pub mount_point: Option<String>,

    /// Mount as read-only
    #[serde(default)]
    pub read_only: bool,

    /// Driver-specific options
    #[serde(default)]
    pub options: VolumeOptions,
}

/// Supported volume driver types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum VolumeType {
    /// SSHFS - SSH Filesystem (default)
    #[default]
    Sshfs,
    // Future drivers:
    // Nfs,
    // NineP,
}

impl std::fmt::Display for VolumeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VolumeType::Sshfs => write!(f, "sshfs"),
        }
    }
}

impl std::str::FromStr for VolumeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sshfs" => Ok(VolumeType::Sshfs),
            _ => Err(format!("Unknown volume type: {}", s)),
        }
    }
}

/// Driver-specific mount options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeOptions {
    /// Auto-reconnect on connection loss (default: true)
    #[serde(default = "default_true")]
    pub reconnect: bool,

    /// Enable compression for data transfer
    #[serde(default)]
    pub compression: bool,

    /// Enable caching for better performance (default: true)
    #[serde(default = "default_true")]
    pub cache: bool,

    /// SSH keep-alive interval in seconds (default: 15)
    #[serde(default = "default_server_alive_interval")]
    pub server_alive_interval: u16,

    /// Number of keep-alive messages before disconnect (default: 3)
    #[serde(default = "default_server_alive_count_max")]
    pub server_alive_count_max: u8,

    /// Extra driver-specific options as key-value pairs
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

fn default_server_alive_interval() -> u16 {
    15
}

fn default_server_alive_count_max() -> u8 {
    3
}

impl Default for VolumeOptions {
    fn default() -> Self {
        Self {
            reconnect: default_true(),
            compression: false,
            cache: default_true(),
            server_alive_interval: default_server_alive_interval(),
            server_alive_count_max: default_server_alive_count_max(),
            extra: HashMap::new(),
        }
    }
}

/// Normalize a path by removing `.` and resolving `..` components
/// Unlike `canonicalize`, this doesn't require the path to exist
fn normalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
    use std::path::Component;

    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {} // Skip `.`
            Component::ParentDir => {
                // Go up one level if possible
                if !components.is_empty() {
                    components.pop();
                }
            }
            c => components.push(c),
        }
    }

    components.iter().collect()
}

impl VolumeConfig {
    /// Create a new volume config with defaults
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            driver_type: VolumeType::default(),
            source: source.into(),
            target: target.into(),
            mount_point: None,
            read_only: false,
            options: VolumeOptions::default(),
        }
    }

    /// Parse a volume spec string in the format "source:target" or "source:target:ro"
    /// For SSHFS: "remote_path:local_mount" or "remote_path:local_mount:ro"
    pub fn from_spec(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split(':').collect();

        match parts.as_slice() {
            [target, mount_point] => {
                let mut config = Self::new("", *target);
                config.mount_point = Some((*mount_point).to_string());
                Ok(config)
            }
            [target, mount_point, "ro"] => {
                let mut config = Self::new("", *target);
                config.mount_point = Some((*mount_point).to_string());
                config.read_only = true;
                Ok(config)
            }
            _ => Err(format!(
                "Invalid volume spec '{}'. Expected format: 'remote_path:local_mount' or 'remote_path:local_mount:ro'",
                spec
            )),
        }
    }

    /// Resolve the source path (expand ~ and relative paths)
    ///
    /// # Arguments
    /// * `project_base_dir` - Base directory of the spuff.yaml file for resolving relative paths
    pub fn resolve_source(&self, project_base_dir: Option<&std::path::Path>) -> String {
        if self.source.is_empty() {
            return String::new();
        }

        let expanded = shellexpand::tilde(&self.source).to_string();

        // If relative, make it absolute from project base directory (where spuff.yaml is)
        if !expanded.starts_with('/') {
            let base = project_base_dir
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let joined = base.join(&expanded);
            return normalize_path(joined).to_string_lossy().to_string();
        }

        expanded
    }

    /// Get the local mount point, generating one if not specified
    ///
    /// Priority:
    /// 1. Explicit mount_point from config
    /// 2. Use source path (mount over local dir for bidirectional editing)
    /// 3. Auto-generate from target path name
    ///
    /// # Arguments
    /// * `instance_name` - Instance name for unique mount point generation
    /// * `project_base_dir` - Base directory of the spuff.yaml file for resolving relative paths
    pub fn resolve_mount_point(
        &self,
        instance_name: Option<&str>,
        project_base_dir: Option<&std::path::Path>,
    ) -> String {
        // 1. Explicit mount_point takes priority
        if let Some(ref mp) = self.mount_point {
            let expanded = shellexpand::tilde(mp).to_string();
            if !expanded.starts_with('/') {
                let base = project_base_dir
                    .map(|p| p.to_path_buf())
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let joined = base.join(&expanded);
                return normalize_path(joined).to_string_lossy().to_string();
            }
            return expanded;
        }

        // 2. If source is defined, use it as mount_point for bidirectional editing
        // This mounts the VM directory over the local source folder
        if !self.source.is_empty() {
            return self.resolve_source(project_base_dir);
        }

        // 3. Auto-generate mount point from target path
        let target_name = std::path::Path::new(&self.target)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "volume".to_string());

        // Use ~/.local/share/spuff/mounts to avoid paths with spaces
        // (macOS's Application Support has spaces which breaks SSHFS)
        let base_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join(".local")
            .join("share")
            .join("spuff")
            .join("mounts");

        // Include instance name if provided for uniqueness
        let mount_dir = if let Some(name) = instance_name {
            base_dir.join(name).join(&target_name)
        } else {
            base_dir.join(&target_name)
        };

        mount_dir.to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_type_default() {
        assert_eq!(VolumeType::default(), VolumeType::Sshfs);
    }

    #[test]
    fn test_volume_type_display() {
        assert_eq!(VolumeType::Sshfs.to_string(), "sshfs");
    }

    #[test]
    fn test_volume_type_from_str() {
        assert_eq!("sshfs".parse::<VolumeType>().unwrap(), VolumeType::Sshfs);
        assert_eq!("SSHFS".parse::<VolumeType>().unwrap(), VolumeType::Sshfs);
        assert!("unknown".parse::<VolumeType>().is_err());
    }

    #[test]
    fn test_volume_config_new() {
        let config = VolumeConfig::new("./src", "/mnt/src");
        assert_eq!(config.source, "./src");
        assert_eq!(config.target, "/mnt/src");
        assert_eq!(config.driver_type, VolumeType::Sshfs);
        assert!(config.mount_point.is_none());
        assert!(!config.read_only);
    }

    #[test]
    fn test_volume_config_from_spec() {
        // Format: remote_path:local_mount
        let config = VolumeConfig::from_spec("/home/dev/project:~/mnt/project").unwrap();
        assert_eq!(config.target, "/home/dev/project");
        assert_eq!(config.mount_point, Some("~/mnt/project".to_string()));
        assert!(!config.read_only);

        let config = VolumeConfig::from_spec("/home/dev/project:~/mnt/project:ro").unwrap();
        assert!(config.read_only);
    }

    #[test]
    fn test_volume_config_from_spec_invalid() {
        assert!(VolumeConfig::from_spec("invalid").is_err());
        assert!(VolumeConfig::from_spec("a:b:c:d").is_err());
    }

    #[test]
    fn test_resolve_mount_point_explicit() {
        let mut config = VolumeConfig::new("", "/home/dev/project");
        config.mount_point = Some("~/my-mount".to_string());

        let resolved = config.resolve_mount_point(None, None);
        assert!(resolved.contains("my-mount"));
    }

    #[test]
    fn test_resolve_mount_point_auto_generated() {
        let config = VolumeConfig::new("", "/home/dev/project");

        let resolved = config.resolve_mount_point(Some("my-vm"), None);
        assert!(resolved.contains("spuff"));
        assert!(resolved.contains("mounts"));
        assert!(resolved.contains("my-vm"));
        assert!(resolved.contains("project"));
    }

    #[test]
    fn test_resolve_mount_point_relative_with_base_dir() {
        let mut config = VolumeConfig::new("./data", "/home/dev/data");
        config.mount_point = Some("./mnt".to_string());

        let base_dir = std::path::Path::new("/projects/myapp");
        let resolved = config.resolve_mount_point(None, Some(base_dir));
        // Path is joined but not normalized, starts with base_dir
        assert!(resolved.starts_with("/projects/myapp"));
        assert!(resolved.contains("mnt"));
    }

    #[test]
    fn test_resolve_source_relative_with_base_dir() {
        let config = VolumeConfig::new("./data", "/home/dev/data");

        let base_dir = std::path::Path::new("/projects/myapp");
        let resolved = config.resolve_source(Some(base_dir));
        // Path is joined but not normalized, starts with base_dir
        assert!(resolved.starts_with("/projects/myapp"));
        assert!(resolved.contains("data"));
    }

    #[test]
    fn test_resolve_mount_point_relative_without_dot() {
        let mut config = VolumeConfig::new("data", "/home/dev/data");
        config.mount_point = Some("mnt".to_string());

        let base_dir = std::path::Path::new("/projects/myapp");
        let resolved = config.resolve_mount_point(None, Some(base_dir));
        assert_eq!(resolved, "/projects/myapp/mnt");
    }

    #[test]
    fn test_resolve_source_relative_without_dot() {
        let config = VolumeConfig::new("data", "/home/dev/data");

        let base_dir = std::path::Path::new("/projects/myapp");
        let resolved = config.resolve_source(Some(base_dir));
        assert_eq!(resolved, "/projects/myapp/data");
    }

    #[test]
    fn test_volume_options_defaults() {
        let options = VolumeOptions::default();
        assert!(options.reconnect);
        assert!(!options.compression);
        assert!(options.cache);
        assert_eq!(options.server_alive_interval, 15);
        assert_eq!(options.server_alive_count_max, 3);
    }

    #[test]
    fn test_parse_yaml_volume() {
        let yaml = r#"
type: sshfs
source: ./src
target: /home/dev/project
mount_point: ~/mnt/project
read_only: true
options:
  compression: true
  reconnect: true
"#;

        let config: VolumeConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.driver_type, VolumeType::Sshfs);
        assert_eq!(config.source, "./src");
        assert_eq!(config.target, "/home/dev/project");
        assert_eq!(config.mount_point, Some("~/mnt/project".to_string()));
        assert!(config.read_only);
        assert!(config.options.compression);
        assert!(config.options.reconnect);
    }

    #[test]
    fn test_parse_yaml_volume_minimal() {
        // Minimal config - just target (remote path)
        let yaml = r#"
target: /home/dev/project
"#;

        let config: VolumeConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.driver_type, VolumeType::Sshfs);
        assert_eq!(config.target, "/home/dev/project");
        assert!(config.mount_point.is_none()); // Auto-generated at runtime
        assert!(!config.read_only);
        assert!(config.options.reconnect);
    }
}
