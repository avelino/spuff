//! Agent commands
//!
//! Commands for interacting with the spuff-agent running on VMs
//! or Docker containers.

mod docker;
mod exec;
mod format;
mod http;
mod logs;
mod status;
mod types;

// Re-export public functions
pub use exec::exec;
pub use logs::{activity, exec_log, logs};
pub use status::{metrics, processes, status};
