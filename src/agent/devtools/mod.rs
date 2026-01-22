//! Devtools installation manager for spuff-agent.
//!
//! Manages async installation of development tools with per-tool status tracking.
//! Each tool can be installed independently and reports its own status.
//!
//! ## Module structure
//! - `types` - Core types (ToolStatus, DevTool, DevToolsConfig, DevToolsState)
//! - `manager` - DevToolsManager for coordinating installations
//! - `installer` - DevToolsInstaller with installation orchestration
//! - `ai_tools` - AI coding tools (claude-code, codex, opencode)
//! - `system` - System tools (Docker, shell tools, Node.js)
//! - `environment` - Dev environments (devbox, nix, dotfiles, tailscale)

mod ai_tools;
mod environment;
mod installer;
mod manager;
mod system;
mod types;

// Re-export public types used by main.rs and routes.rs
pub use manager::DevToolsManager;
pub use types::DevToolsConfig;
