//! HTTP routes for the spuff-agent API.
//!
//! All endpoints require authentication via the `X-Spuff-Token` header,
//! except for `/health` which is public for load balancer probes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::metrics::get_top_processes;
use crate::AppState;

/// Directory where log files can be read from.
/// All log file requests are validated against this base path.
const ALLOWED_LOG_DIR: &str = "/var/log";

/// Creates the router with all agent routes.
///
/// Authentication middleware is applied to all routes except `/health`.
pub fn create_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Public routes (no auth required)
        .route("/health", get(health))
        // Protected routes (auth required via AuthenticatedState extractor)
        .route("/metrics", get(metrics))
        .route("/status", get(status))
        .route("/processes", get(processes))
        .route("/exec", post(exec))
        .route("/heartbeat", post(heartbeat))
        .route("/logs", get(logs))
        .route("/cloud-init", get(cloud_init_status))
        .route("/activity", get(activity_log))
}

/// Custom extractor that validates authentication before allowing access to state.
///
/// Uses the `X-Spuff-Token` header for authentication when `SPUFF_AGENT_TOKEN` is set.
/// If the environment variable is not set, authentication is disabled (development mode).
pub struct AuthenticatedState(pub Arc<AppState>);

impl FromRequestParts<Arc<AppState>> for AuthenticatedState {
    type Rejection = (StatusCode, Json<ApiError>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let provided_token = parts
            .headers
            .get("X-Spuff-Token")
            .and_then(|v| v.to_str().ok());

        // Get expected token from state
        if let Some(expected_token) = &state.auth_token {
            match provided_token {
                Some(token) if token == expected_token => {
                    // Token matches, allow access
                    Ok(AuthenticatedState(Arc::clone(state)))
                }
                Some(_) => {
                    // Wrong token
                    Err((
                        StatusCode::UNAUTHORIZED,
                        Json(ApiError::new("Invalid authentication token")),
                    ))
                }
                None => {
                    // Missing token
                    Err((
                        StatusCode::UNAUTHORIZED,
                        Json(ApiError::new("Missing X-Spuff-Token header")),
                    ))
                }
            }
        } else {
            // No token configured, allow request (development mode)
            tracing::debug!("No SPUFF_AGENT_TOKEN configured, authentication disabled");
            Ok(AuthenticatedState(Arc::clone(state)))
        }
    }
}


/// Standard API error response.
#[derive(Debug, Serialize)]
pub struct ApiError {
    error: String,
}

impl ApiError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}

/// GET /health - Simple health check (public, no auth required)
///
/// Returns basic service information. Used by load balancers and monitoring.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "spuff-agent",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// GET /metrics - System metrics (requires authentication)
///
/// Returns current CPU, memory, disk, and load metrics.
async fn metrics(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;
    let metrics = state.metrics.read().await;
    Json(metrics.clone())
}

/// Response for the /status endpoint.
#[derive(Debug, Serialize)]
struct StatusResponse {
    uptime_seconds: i64,
    idle_seconds: i64,
    hostname: String,
    cloud_init_done: bool,
    bootstrap_status: String,
    bootstrap_ready: bool,
    agent_version: String,
}

/// GET /status - Agent and system status (requires authentication)
///
/// Returns uptime, idle time, cloud-init and bootstrap status.
async fn status(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    let cloud_init_done = std::fs::read_to_string("/run/cloud-init/result.json")
        .map(|s| s.contains(r#""status": "done""#) || s.contains(r#""status":"done""#))
        .unwrap_or(false);

    // Read bootstrap status from spuff's status file
    let bootstrap_status = std::fs::read_to_string("/opt/spuff/bootstrap.status")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let bootstrap_ready = bootstrap_status == "ready";

    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

    Json(StatusResponse {
        uptime_seconds: state.uptime_seconds().await,
        idle_seconds: state.idle_seconds().await,
        hostname,
        cloud_init_done,
        bootstrap_status,
        bootstrap_ready,
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /processes - Top processes by CPU usage (requires authentication)
///
/// Returns the top 10 processes sorted by CPU usage.
async fn processes(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;
    Json(get_top_processes(10))
}

/// Request body for the /exec endpoint.
#[derive(Debug, Deserialize)]
struct ExecRequest {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Response for the /exec endpoint.
#[derive(Debug, Serialize)]
struct ExecResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    duration_ms: u64,
}

/// POST /exec - Execute a command (EXPERIMENTAL, requires authentication)
///
/// # Security Warning
///
/// This endpoint executes arbitrary commands. It should only be used in
/// trusted environments and is disabled by default in production.
///
/// TODO: Implement command whitelist or remove this endpoint.
async fn exec(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    // Log the command execution (truncate long commands)
    let cmd_preview = if req.command.len() > 80 {
        format!("{}...", &req.command[..80])
    } else {
        req.command.clone()
    };

    let timeout = req.timeout_secs.unwrap_or(30);
    let start = std::time::Instant::now();

    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout),
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&req.command)
            .output(),
    )
    .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            state.log_activity(
                "exec",
                Some(format!("cmd='{}' exit={} duration={}ms", cmd_preview, exit_code, duration_ms))
            ).await;
            Ok(Json(ExecResponse {
                exit_code,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                duration_ms,
            }))
        }
        Ok(Err(e)) => {
            state.log_activity(
                "exec_failed",
                Some(format!("cmd='{}' error={}", cmd_preview, e))
            ).await;
            tracing::error!("Command execution failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(format!("Execution failed: {}", e))),
            ))
        }
        Err(_) => {
            state.log_activity(
                "exec_timeout",
                Some(format!("cmd='{}' timeout={}s", cmd_preview, timeout))
            ).await;
            Err((
                StatusCode::REQUEST_TIMEOUT,
                Json(ApiError::new(format!(
                    "Command timed out after {}s",
                    timeout
                ))),
            ))
        }
    }
}

/// POST /heartbeat - Update activity timestamp (requires authentication)
///
/// Called by clients to indicate they are still active, resetting the idle timer.
async fn heartbeat(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;
    state.log_activity("heartbeat", None).await;
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Query parameters for the /logs endpoint.
#[derive(Debug, Deserialize)]
struct LogsQuery {
    /// Number of lines to return (default: 100, max: 10000)
    lines: Option<usize>,
    /// Log file path (must be within /var/log/)
    file: Option<String>,
}

/// GET /logs - Get system logs (requires authentication)
///
/// Returns the last N lines of a log file. Only files within `/var/log/` are accessible.
///
/// # Security
///
/// This endpoint validates paths to prevent directory traversal attacks.
/// The path is canonicalized before checking that it resides within `/var/log/`.
async fn logs(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<LogsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    let lines = query.lines.unwrap_or(100).min(10000);
    let file_path = query
        .file
        .unwrap_or_else(|| "/var/log/cloud-init-output.log".to_string());

    // Validate the path to prevent directory traversal
    let validated_path = validate_log_path(&file_path)?;

    // Read the last N lines efficiently without loading entire file
    let lines_vec = read_last_lines(&validated_path, lines).map_err(|e| {
        tracing::warn!("Failed to read log file '{}': {}", validated_path.display(), e);
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(format!("Cannot read file: {}", e))),
        )
    })?;

    Ok(Json(serde_json::json!({ "lines": lines_vec })))
}

/// Validates a log file path, ensuring it exists and is within the allowed directory.
///
/// This function prevents path traversal attacks by:
/// 1. Canonicalizing the path to resolve symlinks and `..` components
/// 2. Verifying the resolved path starts with `/var/log/`
fn validate_log_path(path: &str) -> Result<std::path::PathBuf, (StatusCode, Json<ApiError>)> {
    let path = Path::new(path);

    // First check: path must start with allowed directory (fast reject)
    if !path.starts_with(ALLOWED_LOG_DIR) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(format!(
                "Access denied: path must be within {}",
                ALLOWED_LOG_DIR
            ))),
        ));
    }

    // Canonicalize to resolve symlinks and .. components
    let canonical = path.canonicalize().map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(format!("File not found: {}", e))),
        )
    })?;

    // Second check: canonical path must still be within allowed directory
    let canonical_base = Path::new(ALLOWED_LOG_DIR)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(ALLOWED_LOG_DIR).to_path_buf());

    if !canonical.starts_with(&canonical_base) {
        tracing::warn!(
            "Path traversal attempt detected: '{}' resolved to '{}'",
            path.display(),
            canonical.display()
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new("Access denied: path traversal detected")),
        ));
    }

    Ok(canonical)
}

/// Reads the last N lines from a file efficiently.
///
/// Uses a bounded buffer to avoid loading the entire file into memory.
fn read_last_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    // Use a ring buffer to keep only the last N lines
    let mut ring_buffer: VecDeque<String> = VecDeque::with_capacity(n + 1);

    for line_result in reader.lines() {
        let line = line_result?;
        if ring_buffer.len() >= n {
            ring_buffer.pop_front();
        }
        ring_buffer.push_back(line);
    }

    Ok(ring_buffer.into_iter().collect())
}

/// Response for the /cloud-init endpoint.
#[derive(Debug, Serialize)]
struct CloudInitStatus {
    status: String,
    done: bool,
    errors: Vec<String>,
    boot_finished: Option<String>,
}

/// GET /cloud-init - Cloud-init detailed status (requires authentication)
///
/// Returns detailed cloud-init status including any errors encountered.
async fn cloud_init_status(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    let status_output = tokio::process::Command::new("cloud-init")
        .args(["status", "--format=json"])
        .output()
        .await
        .ok();

    let (status, done, errors) = if let Some(output) = status_output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let status = json["status"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            let done = status == "done";
            let errors: Vec<String> = json["errors"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            (status, done, errors)
        } else {
            ("unknown".to_string(), false, vec![])
        }
    } else {
        ("unavailable".to_string(), false, vec![])
    };

    let boot_finished = std::fs::read_to_string("/var/lib/cloud/instance/boot-finished")
        .ok()
        .map(|s| s.trim().to_string());

    Json(CloudInitStatus {
        status,
        done,
        errors,
        boot_finished,
    })
}

/// Query parameters for the /activity endpoint.
#[derive(Debug, Deserialize)]
struct ActivityQuery {
    /// Number of entries to return (default: 20, max: 100)
    limit: Option<usize>,
}

/// GET /activity - Get recent activity log (requires authentication)
///
/// Returns the most recent activity log entries for transparency.
async fn activity_log(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<ActivityQuery>,
) -> impl IntoResponse {
    state.update_activity().await;

    let limit = query.limit.unwrap_or(20).min(100);
    let entries = state.get_activity_log(limit).await;

    Json(serde_json::json!({
        "entries": entries,
        "count": entries.len()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_log_path_valid() {
        // This test requires /var/log to exist (standard on Linux)
        if Path::new("/var/log").exists() {
            // Find an actual file in /var/log for testing
            if let Ok(entries) = std::fs::read_dir("/var/log") {
                for entry in entries.flatten() {
                    if entry.path().is_file() {
                        let result = validate_log_path(entry.path().to_str().unwrap());
                        assert!(result.is_ok());
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn test_validate_log_path_outside_var_log() {
        let result = validate_log_path("/etc/passwd");
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_validate_log_path_traversal_attempt() {
        // Path traversal should fail - either because the target doesn't exist
        // (404) or because it resolves outside /var/log (403)
        let result = validate_log_path("/var/log/../etc/passwd");
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        // Accept either NOT_FOUND (target doesn't exist) or FORBIDDEN (traversal detected)
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "Expected FORBIDDEN or NOT_FOUND, got {}",
            status
        );
    }

    #[test]
    fn test_validate_log_path_nonexistent() {
        let result = validate_log_path("/var/log/nonexistent_file_12345.log");
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_read_last_lines() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.log");

        // Write test content
        std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

        let result = read_last_lines(&file_path, 3).unwrap();
        assert_eq!(result, vec!["line3", "line4", "line5"]);
    }

    #[test]
    fn test_read_last_lines_fewer_than_requested() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.log");

        std::fs::write(&file_path, "line1\nline2\n").unwrap();

        let result = read_last_lines(&file_path, 10).unwrap();
        assert_eq!(result, vec!["line1", "line2"]);
    }

    #[test]
    fn test_read_last_lines_empty_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("empty.log");

        std::fs::write(&file_path, "").unwrap();

        let result = read_last_lines(&file_path, 10).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_api_error_serialization() {
        let error = ApiError::new("test error");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("test error"));
    }
}
