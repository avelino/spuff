//! spuff-agent - Remote monitoring daemon for spuff dev environments.
//!
//! This agent runs on provisioned VMs and provides:
//! - System metrics (CPU, memory, disk, load)
//! - Process monitoring
//! - Log file access
//! - Cloud-init status
//! - Activity tracking for idle detection
//! - Activity log for transparency
//! - Devtools installation management
//!
//! # Authentication
//!
//! Set the `SPUFF_AGENT_TOKEN` environment variable to enable authentication.
//! When set, all API requests (except `/health`) must include the token
//! in the `X-Spuff-Token` header.
//!
//! # Configuration
//!
//! Environment variables:
//! - `SPUFF_AGENT_TOKEN`: Authentication token (optional, disables auth if unset)
//! - `SPUFF_AGENT_PORT`: Listen port (default: 7575)
//! - `SPUFF_AGENT_USER`: Username for devtools installation (default: from /opt/spuff/username)
//! - `RUST_LOG`: Log level (default: spuff_agent=info,tower_http=info)

mod devtools;
mod metrics;
mod project_setup;
mod routes;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::devtools::DevToolsManager;
use crate::metrics::SystemMetrics;
use crate::project_setup::ProjectSetupManager;

/// Maximum number of activity log entries to keep in memory
const MAX_ACTIVITY_LOG_ENTRIES: usize = 100;

/// File path for persistent exec log
const EXEC_LOG_FILE: &str = "/var/log/spuff-exec.log";

/// An entry in the activity log
#[derive(Debug, Clone, Serialize)]
pub struct ActivityLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub event: String,
    pub details: Option<String>,
}

impl ActivityLogEntry {
    pub fn new(event: impl Into<String>, details: Option<String>) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            event: event.into(),
            details,
        }
    }
}

/// Application state shared across all request handlers.
pub struct AppState {
    /// Timestamp of the last client activity (used for idle detection).
    pub last_activity: RwLock<chrono::DateTime<chrono::Utc>>,
    /// Agent start time.
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// Cached system metrics (updated periodically in background).
    pub metrics: RwLock<SystemMetrics>,
    /// Authentication token (None disables authentication).
    pub auth_token: Option<String>,
    /// Activity log for transparency (ring buffer)
    pub activity_log: RwLock<VecDeque<ActivityLogEntry>>,
    /// Devtools installation manager
    pub devtools: DevToolsManager,
    /// Project setup manager (from spuff.yaml)
    pub project_setup: ProjectSetupManager,
}

impl AppState {
    /// Creates a new AppState with the given authentication token and username.
    fn new(auth_token: Option<String>, username: String) -> Self {
        Self {
            last_activity: RwLock::new(chrono::Utc::now()),
            start_time: chrono::Utc::now(),
            metrics: RwLock::new(SystemMetrics::collect()),
            auth_token,
            activity_log: RwLock::new(VecDeque::with_capacity(MAX_ACTIVITY_LOG_ENTRIES)),
            devtools: DevToolsManager::new(username.clone()),
            project_setup: ProjectSetupManager::new(username),
        }
    }

    /// Updates the last activity timestamp to now.
    pub async fn update_activity(&self) {
        let mut last = self.last_activity.write().await;
        *last = chrono::Utc::now();
    }

    /// Returns the number of seconds since the last client activity.
    pub async fn idle_seconds(&self) -> i64 {
        let last = self.last_activity.read().await;
        (chrono::Utc::now() - *last).num_seconds()
    }

    /// Returns the number of seconds since the agent started.
    pub async fn uptime_seconds(&self) -> i64 {
        (chrono::Utc::now() - self.start_time).num_seconds()
    }

    /// Log an activity event
    pub async fn log_activity(&self, event: impl Into<String>, details: Option<String>) {
        let event_str = event.into();
        let entry = ActivityLogEntry::new(event_str.clone(), details.clone());

        // Persist exec-related events to file for tracking
        if event_str.starts_with("exec") {
            self.persist_exec_log(&entry);
        }

        let mut log = self.activity_log.write().await;
        if log.len() >= MAX_ACTIVITY_LOG_ENTRIES {
            log.pop_front();
        }
        log.push_back(entry);
    }

    /// Persist exec log entry to file for tracking
    fn persist_exec_log(&self, entry: &ActivityLogEntry) {
        use std::io::Write;

        let log_line = format!(
            "{}\t{}\t{}\n",
            entry.timestamp.to_rfc3339(),
            entry.event,
            entry.details.as_deref().unwrap_or("")
        );

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(EXEC_LOG_FILE)
        {
            let _ = file.write_all(log_line.as_bytes());
        } else {
            tracing::warn!("Failed to write to exec log file: {}", EXEC_LOG_FILE);
        }
    }

    /// Get recent activity log entries
    pub async fn get_activity_log(&self, limit: usize) -> Vec<ActivityLogEntry> {
        let log = self.activity_log.read().await;
        log.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "spuff_agent=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration from environment
    let auth_token = std::env::var("SPUFF_AGENT_TOKEN").ok();
    let port: u16 = std::env::var("SPUFF_AGENT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7575);

    // Get username for devtools installation
    let username = std::env::var("SPUFF_AGENT_USER")
        .ok()
        .or_else(|| std::fs::read_to_string("/opt/spuff/username").ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "dev".to_string());

    if auth_token.is_some() {
        tracing::info!("Authentication enabled via SPUFF_AGENT_TOKEN");
    } else {
        tracing::warn!(
            "SPUFF_AGENT_TOKEN not set - authentication disabled. \
             This is insecure for production use."
        );
    }

    tracing::info!("Devtools username: {}", username);

    let state = Arc::new(AppState::new(auth_token, username));

    // Log agent startup
    {
        let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());
        state.log_activity(
            "agent_started",
            Some(format!("spuff-agent v{} on {} (port {})", env!("CARGO_PKG_VERSION"), hostname, port))
        ).await;
    }

    // Background task to update metrics periodically
    let metrics_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let new_metrics = SystemMetrics::collect();
            let mut metrics = metrics_state.metrics.write().await;
            *metrics = new_metrics;
        }
    });

    // Background task to write heartbeat file for idle detection
    let heartbeat_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let idle = heartbeat_state.idle_seconds().await;
            if let Err(e) = std::fs::write("/tmp/spuff-agent-heartbeat", format!("{}", idle)) {
                tracing::debug!("Failed to write heartbeat file: {}", e);
            }
        }
    });

    let app = Router::new()
        .merge(routes::create_routes())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("spuff-agent listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
