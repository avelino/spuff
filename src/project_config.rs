//! Project configuration from spuff.yaml
//!
//! Defines the per-project configuration schema for spuff environments.
//! This allows declarative configuration of dev environments per repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, SpuffError};

/// Main project configuration loaded from spuff.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Spec version for future compatibility
    #[serde(default = "default_version")]
    pub version: String,

    /// Project/environment name (default: directory name)
    #[serde(default)]
    pub name: Option<String>,

    /// Resource overrides (VM size, region)
    #[serde(default)]
    pub resources: ResourcesConfig,

    /// Language bundles to install
    #[serde(default)]
    pub bundles: Vec<String>,

    /// Individual system packages to install
    #[serde(default)]
    pub packages: Vec<String>,

    /// Docker services configuration
    #[serde(default)]
    pub services: ServicesConfig,

    /// Repositories to clone
    #[serde(default)]
    pub repositories: Vec<Repository>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Setup scripts to run after bundles/packages
    #[serde(default)]
    pub setup: Vec<String>,

    /// Ports for SSH tunnel
    #[serde(default)]
    pub ports: Vec<u16>,

    /// Lifecycle hooks
    #[serde(default)]
    pub hooks: HooksConfig,

    /// AI coding tools to install (claude-code, codex, opencode)
    /// Can be a list of tools, "all", or "none"
    #[serde(default)]
    pub ai_tools: AiToolsConfig,
}

/// AI tools configuration
/// Supports: list of specific tools, "all", or "none"
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AiToolsConfig {
    /// Install all AI tools (default)
    #[default]
    All,
    /// Don't install any AI tools
    None,
    /// Install specific tools
    List(Vec<String>),
}

impl Serialize for AiToolsConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            AiToolsConfig::All => serializer.serialize_str("all"),
            AiToolsConfig::None => serializer.serialize_str("none"),
            AiToolsConfig::List(tools) => tools.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AiToolsConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct AiToolsConfigVisitor;

        impl<'de> Visitor<'de> for AiToolsConfigVisitor {
            type Value = AiToolsConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("'all', 'none', or a list of AI tool names")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<AiToolsConfig, E>
            where
                E: de::Error,
            {
                match value.to_lowercase().as_str() {
                    "all" => Ok(AiToolsConfig::All),
                    "none" => Ok(AiToolsConfig::None),
                    _ => Err(de::Error::custom(format!(
                        "invalid ai_tools value: '{}', expected 'all', 'none', or a list",
                        value
                    ))),
                }
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<AiToolsConfig, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut tools = Vec::new();
                while let Some(tool) = seq.next_element::<String>()? {
                    tools.push(tool);
                }
                Ok(AiToolsConfig::List(tools))
            }
        }

        deserializer.deserialize_any(AiToolsConfigVisitor)
    }
}

impl AiToolsConfig {
    /// Check if a specific tool should be installed
    pub fn should_install(&self, tool: &str) -> bool {
        match self {
            AiToolsConfig::All => true,
            AiToolsConfig::None => false,
            AiToolsConfig::List(tools) => tools.iter().any(|t| t == tool),
        }
    }

    /// Get list of tools to install
    pub fn tools_to_install(&self) -> Vec<&str> {
        match self {
            AiToolsConfig::All => vec!["claude-code", "codex", "opencode"],
            AiToolsConfig::None => vec![],
            AiToolsConfig::List(tools) => tools.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Parse from CLI argument string (e.g., "claude-code,codex" or "all" or "none")
    pub fn from_cli_arg(arg: &str) -> Self {
        match arg.to_lowercase().as_str() {
            "all" => AiToolsConfig::All,
            "none" => AiToolsConfig::None,
            _ => {
                let tools: Vec<String> = arg.split(',').map(|s| s.trim().to_string()).collect();
                AiToolsConfig::List(tools)
            }
        }
    }
}

fn default_version() -> String {
    "1".to_string()
}

/// Resource overrides for the VM
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourcesConfig {
    /// VM size (e.g., s-4vcpu-8gb)
    #[serde(default)]
    pub size: Option<String>,

    /// Region preference (e.g., nyc1)
    #[serde(default)]
    pub region: Option<String>,
}

/// Docker services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicesConfig {
    /// Whether services are enabled (default: true if docker-compose.yaml exists)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to compose file (default: docker-compose.yaml)
    #[serde(default = "default_compose_file")]
    pub compose_file: String,

    /// Docker Compose profiles to activate
    #[serde(default)]
    pub profiles: Vec<String>,
}

impl Default for ServicesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            compose_file: default_compose_file(),
            profiles: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_compose_file() -> String {
    "docker-compose.yaml".to_string()
}

/// Repository to clone
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Repository {
    /// Short format: "owner/repo" (assumes GitHub)
    Short(String),
    /// Full format with options
    Full(RepositoryConfig),
}

/// Full repository configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// Git URL (SSH or HTTPS)
    pub url: String,

    /// Target path (default: ~/projects/<repo-name>)
    #[serde(default)]
    pub path: Option<String>,

    /// Branch to checkout
    #[serde(default)]
    pub branch: Option<String>,
}

/// Lifecycle hooks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Script to run after environment is ready
    #[serde(default)]
    pub post_up: Option<String>,

    /// Script to run before destroying
    #[serde(default)]
    pub pre_down: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            name: None,
            resources: ResourcesConfig::default(),
            bundles: Vec::new(),
            packages: Vec::new(),
            services: ServicesConfig::default(),
            repositories: Vec::new(),
            env: HashMap::new(),
            setup: Vec::new(),
            ports: Vec::new(),
            hooks: HooksConfig::default(),
            ai_tools: AiToolsConfig::default(),
        }
    }
}

impl ProjectConfig {
    /// Look for spuff.yaml in the current directory or parent directories
    pub fn discover() -> Option<PathBuf> {
        let mut current = std::env::current_dir().ok()?;

        loop {
            let config_path = current.join("spuff.yaml");
            if config_path.exists() {
                return Some(config_path);
            }

            // Also check spuff.yml
            let alt_path = current.join("spuff.yml");
            if alt_path.exists() {
                return Some(alt_path);
            }

            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Load project configuration from a path
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SpuffError::Config(format!("Failed to read {}: {}", path.display(), e)))?;

        let mut config: ProjectConfig = serde_yaml::from_str(&content)
            .map_err(|e| SpuffError::Config(format!("Invalid spuff.yaml: {}", e)))?;

        // Load secrets if they exist
        let secrets_path = path.with_file_name("spuff.secrets.yaml");
        if secrets_path.exists() {
            config.merge_secrets(&secrets_path)?;
        }

        // Resolve environment variables in env values
        config.resolve_env_vars();

        Ok(config)
    }

    /// Load project configuration from the current directory (discovers automatically)
    pub fn load_from_cwd() -> Result<Option<Self>> {
        match Self::discover() {
            Some(path) => Ok(Some(Self::load(&path)?)),
            None => Ok(None),
        }
    }

    /// Merge secrets from spuff.secrets.yaml
    fn merge_secrets(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SpuffError::Config(format!("Failed to read {}: {}", path.display(), e)))?;

        #[derive(Deserialize)]
        struct Secrets {
            #[serde(default)]
            env: HashMap<String, String>,
        }

        let secrets: Secrets = serde_yaml::from_str(&content)
            .map_err(|e| SpuffError::Config(format!("Invalid spuff.secrets.yaml: {}", e)))?;

        // Secrets override env vars from main config
        for (key, value) in secrets.env {
            self.env.insert(key, value);
        }

        Ok(())
    }

    /// Resolve environment variables in env values ($VAR or ${VAR:-default})
    fn resolve_env_vars(&mut self) {
        let resolved: HashMap<String, String> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), resolve_env_value(v)))
            .collect();
        self.env = resolved;
    }
}

/// Resolve environment variable references in a value
fn resolve_env_value(value: &str) -> String {
    let mut result = value.to_string();

    // Match ${VAR:-default} pattern
    let re_with_default =
        regex_lite::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*):-([^}]*)\}").unwrap();
    result = re_with_default
        .replace_all(&result, |caps: &regex_lite::Captures| {
            let var_name = &caps[1];
            let default = &caps[2];
            std::env::var(var_name).unwrap_or_else(|_| default.to_string())
        })
        .to_string();

    // Match ${VAR} pattern
    let re_braces = regex_lite::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    result = re_braces
        .replace_all(&result, |caps: &regex_lite::Captures| {
            let var_name = &caps[1];
            std::env::var(var_name).unwrap_or_default()
        })
        .to_string();

    // Match $VAR pattern (at word boundary or end)
    let re_simple = regex_lite::Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    result = re_simple
        .replace_all(&result, |caps: &regex_lite::Captures| {
            let var_name = &caps[1];
            std::env::var(var_name).unwrap_or_default()
        })
        .to_string();

    result
}

/// Status of a project setup item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed(String),
    Skipped,
}

/// Status of the entire project setup
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectSetupState {
    pub started: bool,
    pub completed: bool,
    pub bundles: Vec<BundleStatus>,
    pub packages: PackagesStatus,
    pub services: ServicesStatus,
    pub repositories: Vec<RepositoryStatus>,
    pub scripts: Vec<ScriptStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundleStatus {
    pub name: String,
    pub status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackagesStatus {
    pub status: SetupStatus,
    pub installed: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServicesStatus {
    pub status: SetupStatus,
    pub containers: Vec<ContainerStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatus {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepositoryStatus {
    pub url: String,
    pub path: String,
    pub status: SetupStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScriptStatus {
    pub command: String,
    pub status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let yaml = r#"
version: "1"
name: my-project
bundles:
  - rust
  - python
packages:
  - postgresql-client
ports:
  - 3000
  - 8080
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, Some("my-project".to_string()));
        assert_eq!(config.bundles, vec!["rust", "python"]);
        assert_eq!(config.packages, vec!["postgresql-client"]);
        assert_eq!(config.ports, vec![3000, 8080]);
    }

    #[test]
    fn test_parse_resources_override() {
        let yaml = r#"
resources:
  size: s-4vcpu-8gb
  region: nyc1
bundles:
  - go
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.resources.size, Some("s-4vcpu-8gb".to_string()));
        assert_eq!(config.resources.region, Some("nyc1".to_string()));
    }

    #[test]
    fn test_parse_repository_short_format() {
        let yaml = r#"
repositories:
  - owner/repo
  - another/project
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.repositories.len(), 2);

        // Verify short format parsed correctly
        match &config.repositories[0] {
            Repository::Short(s) => assert_eq!(s, "owner/repo"),
            Repository::Full(_) => panic!("Expected short format"),
        }
    }

    #[test]
    fn test_parse_repository_full_format() {
        let yaml = r#"
repositories:
  - url: git@github.com:empresa/backend.git
    path: ~/projects/backend
    branch: develop
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();

        // Verify full format parsed correctly
        match &config.repositories[0] {
            Repository::Full(repo) => {
                assert_eq!(repo.url, "git@github.com:empresa/backend.git");
                assert_eq!(repo.path, Some("~/projects/backend".to_string()));
                assert_eq!(repo.branch, Some("develop".to_string()));
            }
            Repository::Short(_) => panic!("Expected full format"),
        }
    }

    #[test]
    fn test_parse_services_config() {
        let yaml = r#"
services:
  enabled: true
  compose_file: docker-compose.dev.yaml
  profiles:
    - dev
    - debug
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.services.enabled);
        assert_eq!(config.services.compose_file, "docker-compose.dev.yaml");
        assert_eq!(config.services.profiles, vec!["dev", "debug"]);
    }

    #[test]
    fn test_parse_env_vars() {
        let yaml = r#"
env:
  DATABASE_URL: postgres://localhost/dev
  DEBUG: "true"
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.env.get("DATABASE_URL"),
            Some(&"postgres://localhost/dev".to_string())
        );
        assert_eq!(config.env.get("DEBUG"), Some(&"true".to_string()));
    }

    #[test]
    fn test_parse_setup_scripts() {
        let yaml = r#"
setup:
  - cargo build --release
  - npm install
  - ./scripts/init-db.sh
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.setup.len(), 3);
        assert_eq!(config.setup[0], "cargo build --release");
    }

    #[test]
    fn test_parse_hooks() {
        let yaml = r#"
hooks:
  post_up: |
    echo "Environment ready!"
    make dev-setup
  pre_down: |
    make db-dump > /tmp/backup.sql
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.hooks.post_up.is_some());
        assert!(config.hooks.pre_down.is_some());
    }

    #[test]
    fn test_resolve_env_var_simple() {
        std::env::set_var("TEST_VAR", "hello");
        let result = resolve_env_value("$TEST_VAR");
        assert_eq!(result, "hello");
        std::env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_resolve_env_var_with_braces() {
        std::env::set_var("TEST_VAR2", "world");
        let result = resolve_env_value("${TEST_VAR2}");
        assert_eq!(result, "world");
        std::env::remove_var("TEST_VAR2");
    }

    #[test]
    fn test_resolve_env_var_with_default() {
        // Ensure var doesn't exist
        std::env::remove_var("NONEXISTENT_VAR");
        let result = resolve_env_value("${NONEXISTENT_VAR:-default_value}");
        assert_eq!(result, "default_value");
    }

    #[test]
    fn test_resolve_env_var_existing_with_default() {
        std::env::set_var("EXISTING_VAR", "actual");
        let result = resolve_env_value("${EXISTING_VAR:-default}");
        assert_eq!(result, "actual");
        std::env::remove_var("EXISTING_VAR");
    }

    #[test]
    fn test_default_config() {
        let config = ProjectConfig::default();
        assert_eq!(config.version, "1");
        assert!(config.bundles.is_empty());
        assert!(config.packages.is_empty());
        assert!(config.services.enabled);
        assert_eq!(config.ai_tools, AiToolsConfig::All);
    }

    #[test]
    fn test_ai_tools_config_all() {
        let yaml = r#"
ai_tools: all
"#;
        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.ai_tools.should_install("claude-code"));
        assert!(config.ai_tools.should_install("codex"));
        assert!(config.ai_tools.should_install("opencode"));
    }

    #[test]
    fn test_ai_tools_config_none() {
        let yaml = r#"
ai_tools: none
"#;
        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.ai_tools.should_install("claude-code"));
        assert!(!config.ai_tools.should_install("codex"));
        assert!(!config.ai_tools.should_install("opencode"));
    }

    #[test]
    fn test_ai_tools_config_list() {
        let yaml = r#"
ai_tools:
  - claude-code
  - opencode
"#;
        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.ai_tools.should_install("claude-code"));
        assert!(!config.ai_tools.should_install("codex"));
        assert!(config.ai_tools.should_install("opencode"));
    }

    #[test]
    fn test_ai_tools_from_cli_arg() {
        assert_eq!(AiToolsConfig::from_cli_arg("all"), AiToolsConfig::All);
        assert_eq!(AiToolsConfig::from_cli_arg("ALL"), AiToolsConfig::All);
        assert_eq!(AiToolsConfig::from_cli_arg("none"), AiToolsConfig::None);
        assert_eq!(AiToolsConfig::from_cli_arg("NONE"), AiToolsConfig::None);

        let list = AiToolsConfig::from_cli_arg("claude-code,codex");
        match list {
            AiToolsConfig::List(tools) => {
                assert_eq!(tools, vec!["claude-code", "codex"]);
            }
            _ => panic!("Expected List variant"),
        }
    }

    #[test]
    fn test_ai_tools_tools_to_install() {
        assert_eq!(
            AiToolsConfig::All.tools_to_install(),
            vec!["claude-code", "codex", "opencode"]
        );
        assert!(AiToolsConfig::None.tools_to_install().is_empty());

        let list = AiToolsConfig::List(vec!["claude-code".to_string()]);
        assert_eq!(list.tools_to_install(), vec!["claude-code"]);
    }
}
