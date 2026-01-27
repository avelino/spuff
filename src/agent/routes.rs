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
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::Duration;

use crate::devtools::DevToolsConfig;
use crate::docker_manager::{ComposeManager, DockerManager};
use crate::metrics::get_top_processes;
use crate::project_setup::ProjectSetupManager;
use crate::volume_manager::AgentVolumeManager;
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
        .route("/exec-log", get(exec_log))
        .route("/heartbeat", post(heartbeat))
        .route("/logs", get(logs))
        .route("/logs/stream", get(logs_stream))
        .route("/cloud-init", get(cloud_init_status))
        .route("/activity", get(activity_log))
        // Devtools management
        .route("/devtools", get(devtools_status))
        .route("/devtools/install", post(devtools_install))
        // Project setup (from spuff.yaml)
        .route("/project/config", get(project_config))
        .route("/project/status", get(project_status))
        .route("/project/setup", post(project_setup))
        // Volume management
        .route("/volumes", get(volumes_list))
        .route("/volumes/status", get(volumes_status))
        .route("/volumes/unmount", post(volumes_unmount))
        // Graceful shutdown
        .route("/shutdown", post(shutdown))
        // Docker management
        .route("/services/docker", get(docker_list))
        .route("/services/docker/start", post(docker_start))
        .route("/services/docker/stop", post(docker_stop))
        .route("/services/docker/restart", post(docker_restart))
        .route("/services/docker/logs", get(docker_logs))
        // Docker Compose management
        .route("/services/compose", get(compose_list))
        .route("/services/compose/up", post(compose_up))
        .route("/services/compose/down", post(compose_down))
        .route("/services/compose/restart", post(compose_restart))
        .route("/services/compose/logs", get(compose_logs))
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

/// Truncate output string for logging, replacing newlines with \n literal.
fn truncate_output(s: &str, max_len: usize) -> String {
    // Replace newlines and tabs with escaped versions for single-line storage
    let escaped = s
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r");

    if escaped.len() <= max_len {
        escaped
    } else {
        format!("{}...", &escaped[..max_len])
    }
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
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

            // Truncate output for logging (max 500 chars each)
            let stdout_preview = truncate_output(&stdout, 500);
            let stderr_preview = truncate_output(&stderr, 500);

            state
                .log_activity(
                    "exec",
                    Some(format!(
                        "cmd='{}' exit={} duration={}ms\t{}\t{}",
                        cmd_preview, exit_code, duration_ms, stdout_preview, stderr_preview
                    )),
                )
                .await;
            Ok(Json(ExecResponse {
                exit_code,
                stdout,
                stderr,
                duration_ms,
            }))
        }
        Ok(Err(e)) => {
            state
                .log_activity(
                    "exec_failed",
                    Some(format!("cmd='{}' error={}", cmd_preview, e)),
                )
                .await;
            tracing::error!("Command execution failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(format!("Execution failed: {}", e))),
            ))
        }
        Err(_) => {
            state
                .log_activity(
                    "exec_timeout",
                    Some(format!("cmd='{}' timeout={}s", cmd_preview, timeout)),
                )
                .await;
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

/// Query parameters for the /exec-log endpoint.
#[derive(Debug, Deserialize)]
struct ExecLogQuery {
    /// Number of lines to return (default: 50, max: 500)
    lines: Option<usize>,
}

/// Exec log entry parsed from file
#[derive(Debug, Serialize)]
struct ExecLogEntry {
    timestamp: String,
    event: String,
    details: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<String>,
}

/// GET /exec-log - Get command execution history (requires authentication)
///
/// Returns the persistent log of all exec commands for tracking and auditing.
async fn exec_log(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<ExecLogQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    let lines = query.lines.unwrap_or(50).min(500);
    let log_path = Path::new("/var/log/spuff-exec.log");

    if !log_path.exists() {
        return Ok(Json(serde_json::json!({
            "entries": [],
            "count": 0,
            "message": "No exec history yet"
        })));
    }

    let lines_vec = read_last_lines(log_path, lines).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(format!("Failed to read exec log: {}", e))),
        )
    })?;

    // Parse TSV format: timestamp\tevent\tdetails[\tstdout\tstderr]
    let entries: Vec<ExecLogEntry> = lines_vec
        .into_iter()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '\t').collect();
            if parts.len() >= 2 {
                Some(ExecLogEntry {
                    timestamp: parts[0].to_string(),
                    event: parts[1].to_string(),
                    details: parts.get(2).unwrap_or(&"").to_string(),
                    stdout: parts
                        .get(3)
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty()),
                    stderr: parts
                        .get(4)
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty()),
                })
            } else {
                None
            }
        })
        .collect();

    let count = entries.len();
    Ok(Json(serde_json::json!({
        "entries": entries,
        "count": count
    })))
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
        tracing::warn!(
            "Failed to read log file '{}': {}",
            validated_path.display(),
            e
        );
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(format!("Cannot read file: {}", e))),
        )
    })?;

    Ok(Json(serde_json::json!({ "lines": lines_vec })))
}

/// Query parameters for the /logs/stream endpoint.
#[derive(Debug, Deserialize)]
struct LogsStreamQuery {
    /// Log file path (must be within /var/log/).
    file: Option<String>,
    /// Number of initial lines to send (default: 10).
    initial_lines: Option<usize>,
}

/// GET /logs/stream - Stream log file updates via SSE (requires authentication)
///
/// Streams new log lines as they are appended to the file using Server-Sent Events.
/// The connection stays open and sends new lines as they appear.
///
/// # Events
///
/// - `initial`: Initial batch of last N lines
/// - `line`: New log line appended to file
/// - `error`: Error occurred while reading file
async fn logs_stream(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<LogsStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    let file_path = query
        .file
        .unwrap_or_else(|| "/var/log/cloud-init-output.log".to_string());
    let initial_lines = query.initial_lines.unwrap_or(10).min(100);

    // Validate the path to prevent directory traversal
    let validated_path = validate_log_path(&file_path)?;
    let path_clone = validated_path.clone();

    state
        .log_activity(
            "logs_stream",
            Some(format!("file={}", validated_path.display())),
        )
        .await;

    // Create the SSE stream
    let stream = async_stream::stream! {
        // Send initial lines
        match read_last_lines(&path_clone, initial_lines) {
            Ok(lines) => {
                for line in lines {
                    yield Ok(Event::default().event("initial").data(line));
                }
            }
            Err(e) => {
                yield Ok(Event::default().event("error").data(format!("Failed to read initial lines: {}", e)));
            }
        }

        // Track file position for tailing
        let mut last_size = std::fs::metadata(&path_clone)
            .map(|m| m.len())
            .unwrap_or(0);

        // Poll for new content
        let mut interval = tokio::time::interval(Duration::from_millis(500));

        loop {
            interval.tick().await;

            // Check current file size
            let current_size = match std::fs::metadata(&path_clone) {
                Ok(m) => m.len(),
                Err(_) => {
                    // File might have been deleted/rotated, wait for it to reappear
                    continue;
                }
            };

            if current_size > last_size {
                // File grew, read new content
                match std::fs::File::open(&path_clone) {
                    Ok(file) => {
                        use std::io::{Read, Seek, SeekFrom};
                        let mut file = file;

                        // Seek to where we left off
                        if file.seek(SeekFrom::Start(last_size)).is_ok() {
                            let mut new_content = String::new();
                            if file.read_to_string(&mut new_content).is_ok() {
                                for line in new_content.lines() {
                                    if !line.is_empty() {
                                        yield Ok(Event::default().event("line").data(line));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        yield Ok(Event::default().event("error").data(format!("Failed to open file: {}", e)));
                    }
                }
                last_size = current_size;
            } else if current_size < last_size {
                // File was truncated/rotated, start from beginning
                last_size = 0;
                yield Ok(Event::default().event("info").data("Log file was rotated, restarting from beginning"));
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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
            let status = json["status"].as_str().unwrap_or("unknown").to_string();
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

/// GET /devtools - Get devtools installation status (requires authentication)
///
/// Returns the current status of all devtools installations.
async fn devtools_status(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;
    let devtools_state = state.devtools.get_state().await;
    Json(devtools_state)
}

/// POST /devtools/install - Start devtools installation (requires authentication)
///
/// Starts async installation of configured devtools.
/// Returns immediately; poll GET /devtools for status.
async fn devtools_install(
    AuthenticatedState(state): AuthenticatedState,
    Json(config): Json<DevToolsConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    // Log the installation request
    state
        .log_activity(
            "devtools_install",
            Some(format!(
                "docker={} shell_tools={} nodejs={} claude_code={} env={:?}",
                config.docker,
                config.shell_tools,
                config.nodejs,
                config.claude_code,
                config.environment
            )),
        )
        .await;

    match state.devtools.install(config).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "status": "started",
            "message": "Devtools installation started. Poll GET /devtools for status."
        }))),
        Err(e) => Err((StatusCode::CONFLICT, Json(ApiError::new(e)))),
    }
}

/// GET /project/config - Get project configuration (requires authentication)
///
/// Returns the project configuration loaded from /opt/spuff/project.json
async fn project_config(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    match ProjectSetupManager::load_config() {
        Some(config) => Json(serde_json::json!({
            "found": true,
            "config": config
        })),
        None => Json(serde_json::json!({
            "found": false,
            "message": "No project config found at /opt/spuff/project.json"
        })),
    }
}

/// GET /project/status - Get project setup status (requires authentication)
///
/// Returns the current status of project setup including bundles, packages, repos, etc.
async fn project_status(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;
    let project_state = state.project_setup.get_state().await;
    Json(project_state)
}

/// POST /project/setup - Start project setup (requires authentication)
///
/// Starts async project setup from /opt/spuff/project.json.
/// Returns immediately; poll GET /project/status for progress.
async fn project_setup(
    AuthenticatedState(state): AuthenticatedState,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    // Log the setup request
    state
        .log_activity(
            "project_setup",
            Some("Starting project setup from spuff.yaml".to_string()),
        )
        .await;

    match state.project_setup.start_setup().await {
        Ok(()) => Ok(Json(serde_json::json!({
            "status": "started",
            "message": "Project setup started. Poll GET /project/status for progress."
        }))),
        Err(e) => Err((StatusCode::CONFLICT, Json(ApiError::new(e)))),
    }
}

/// GET /volumes - List SSHFS mounted volumes (requires authentication)
///
/// Returns all SSHFS mounts currently active on the VM.
async fn volumes_list(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    let manager = AgentVolumeManager::new();
    let mounts = manager.list_mounts().await;

    Json(serde_json::json!({
        "mounts": mounts,
        "count": mounts.len()
    }))
}

/// GET /volumes/status - Get detailed status of all volumes (requires authentication)
///
/// Returns status information including accessibility and latency for each mount.
async fn volumes_status(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    let manager = AgentVolumeManager::new();
    let statuses = manager.get_status().await;

    let healthy_count = statuses.iter().filter(|s| s.accessible).count();

    Json(serde_json::json!({
        "volumes": statuses,
        "total": statuses.len(),
        "healthy": healthy_count,
        "unhealthy": statuses.len() - healthy_count
    }))
}

/// Request body for the /volumes/unmount endpoint.
#[derive(Debug, Deserialize)]
struct UnmountRequest {
    /// Target path to unmount
    target: String,
}

/// POST /volumes/unmount - Unmount a specific volume (requires authentication)
///
/// Unmounts the SSHFS volume at the specified target path.
async fn volumes_unmount(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<UnmountRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    state
        .log_activity("volume_unmount", Some(format!("target={}", req.target)))
        .await;

    let manager = AgentVolumeManager::new();

    match manager.unmount(&req.target).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "status": "ok",
            "message": format!("Successfully unmounted {}", req.target)
        }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ApiError::new(e)))),
    }
}

// ============================================================================
// Docker Container Management
// ============================================================================

/// GET /services/docker - List all Docker containers (requires authentication)
///
/// Returns all containers (running and stopped) with their status.
async fn docker_list(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    if !DockerManager::is_available().await {
        return Json(serde_json::json!({
            "available": false,
            "containers": [],
            "message": "Docker is not available on this system"
        }));
    }

    match DockerManager::list_containers().await {
        Ok(containers) => {
            let running = containers.iter().filter(|c| c.state == "running").count();
            Json(serde_json::json!({
                "available": true,
                "containers": containers,
                "total": containers.len(),
                "running": running
            }))
        }
        Err(e) => Json(serde_json::json!({
            "available": true,
            "containers": [],
            "error": e
        })),
    }
}

/// Request body for Docker container operations.
#[derive(Debug, Deserialize)]
struct DockerContainerRequest {
    /// Container name or ID.
    container: String,
    /// Timeout in seconds (for stop/restart).
    #[serde(default)]
    timeout_secs: Option<u32>,
}

/// POST /services/docker/start - Start a Docker container (requires authentication)
async fn docker_start(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<DockerContainerRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity("docker_start", Some(format!("container={}", req.container)))
        .await;

    let result = DockerManager::start_container(&req.container).await;
    Json(result)
}

/// POST /services/docker/stop - Stop a Docker container (requires authentication)
async fn docker_stop(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<DockerContainerRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity("docker_stop", Some(format!("container={}", req.container)))
        .await;

    let result = DockerManager::stop_container(&req.container, req.timeout_secs).await;
    Json(result)
}

/// POST /services/docker/restart - Restart a Docker container (requires authentication)
async fn docker_restart(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<DockerContainerRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity(
            "docker_restart",
            Some(format!("container={}", req.container)),
        )
        .await;

    let result = DockerManager::restart_container(&req.container, req.timeout_secs).await;
    Json(result)
}

/// Query parameters for Docker logs endpoint.
#[derive(Debug, Deserialize)]
struct DockerLogsQuery {
    /// Container name or ID.
    container: String,
    /// Number of lines to return.
    lines: Option<usize>,
    /// Show logs since timestamp (e.g., "10m", "1h", "2023-01-01T00:00:00").
    since: Option<String>,
}

/// GET /services/docker/logs - Get Docker container logs (requires authentication)
async fn docker_logs(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<DockerLogsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    match DockerManager::container_logs(&query.container, query.lines, query.since.as_deref()).await
    {
        Ok(logs) => Ok(Json(serde_json::json!({
            "container": query.container,
            "logs": logs
        }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ApiError::new(e)))),
    }
}

// ============================================================================
// Docker Compose Management
// ============================================================================

/// Query parameters for compose endpoints.
#[derive(Debug, Deserialize)]
struct ComposeQuery {
    /// Working directory for compose commands.
    #[serde(default)]
    working_dir: Option<String>,
}

/// GET /services/compose - List Docker Compose services (requires authentication)
///
/// Returns all services defined in the compose file with their status.
async fn compose_list(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<ComposeQuery>,
) -> impl IntoResponse {
    state.update_activity().await;

    if !ComposeManager::is_available().await {
        return Json(serde_json::json!({
            "available": false,
            "services": [],
            "message": "Docker Compose is not available on this system"
        }));
    }

    let manager = ComposeManager::new(query.working_dir.as_deref());

    if !manager.has_compose_file() {
        return Json(serde_json::json!({
            "available": true,
            "has_compose_file": false,
            "services": [],
            "message": "No compose file found in working directory"
        }));
    }

    match manager.list_services().await {
        Ok(services) => {
            let running = services.iter().filter(|s| s.state == "running").count();
            Json(serde_json::json!({
                "available": true,
                "has_compose_file": true,
                "services": services,
                "total": services.len(),
                "running": running
            }))
        }
        Err(e) => Json(serde_json::json!({
            "available": true,
            "has_compose_file": true,
            "services": [],
            "error": e
        })),
    }
}

/// Request body for compose service operations.
#[derive(Debug, Deserialize)]
struct ComposeServiceRequest {
    /// Service name (optional, affects all services if not provided).
    #[serde(default)]
    service: Option<String>,
    /// Working directory for compose commands.
    #[serde(default)]
    working_dir: Option<String>,
    /// Run in detached mode (for up).
    #[serde(default = "default_true")]
    detach: bool,
    /// Remove volumes (for down).
    #[serde(default)]
    remove_volumes: bool,
}

fn default_true() -> bool {
    true
}

/// POST /services/compose/up - Start Docker Compose services (requires authentication)
async fn compose_up(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<ComposeServiceRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity("compose_up", Some(format!("service={:?}", req.service)))
        .await;

    let manager = ComposeManager::new(req.working_dir.as_deref());
    let result = manager.up(req.service.as_deref(), req.detach).await;
    Json(result)
}

/// POST /services/compose/down - Stop Docker Compose services (requires authentication)
async fn compose_down(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<ComposeServiceRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity(
            "compose_down",
            Some(format!(
                "service={:?} remove_volumes={}",
                req.service, req.remove_volumes
            )),
        )
        .await;

    let manager = ComposeManager::new(req.working_dir.as_deref());
    let result = manager
        .down(req.service.as_deref(), req.remove_volumes)
        .await;
    Json(result)
}

/// POST /services/compose/restart - Restart Docker Compose services (requires authentication)
async fn compose_restart(
    AuthenticatedState(state): AuthenticatedState,
    Json(req): Json<ComposeServiceRequest>,
) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity(
            "compose_restart",
            Some(format!("service={:?}", req.service)),
        )
        .await;

    let manager = ComposeManager::new(req.working_dir.as_deref());
    let result = manager.restart(req.service.as_deref()).await;
    Json(result)
}

/// Query parameters for compose logs endpoint.
#[derive(Debug, Deserialize)]
struct ComposeLogsQuery {
    /// Service name (optional, shows all services if not provided).
    #[serde(default)]
    service: Option<String>,
    /// Working directory for compose commands.
    #[serde(default)]
    working_dir: Option<String>,
    /// Number of lines to return.
    lines: Option<usize>,
    /// Show logs since timestamp.
    since: Option<String>,
}

/// GET /services/compose/logs - Get Docker Compose service logs (requires authentication)
async fn compose_logs(
    AuthenticatedState(state): AuthenticatedState,
    Query(query): Query<ComposeLogsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.update_activity().await;

    let manager = ComposeManager::new(query.working_dir.as_deref());

    match manager
        .logs(
            query.service.as_deref(),
            query.lines,
            query.since.as_deref(),
        )
        .await
    {
        Ok(logs) => Ok(Json(serde_json::json!({
            "service": query.service,
            "logs": logs
        }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ApiError::new(e)))),
    }
}

// ============================================================================
// Graceful Shutdown
// ============================================================================

/// Response for the shutdown endpoint.
#[derive(Debug, Serialize)]
struct ShutdownResponse {
    success: bool,
    message: String,
    steps: Vec<ShutdownStep>,
    duration_ms: u64,
}

#[derive(Debug, Serialize)]
struct ShutdownStep {
    name: String,
    success: bool,
    message: String,
}

/// POST /shutdown - Gracefully prepare VM for destruction (requires authentication)
///
/// Executes cleanup operations before the VM is destroyed:
/// 1. Runs pre_down hooks from project config
/// 2. Stops docker-compose services
/// 3. Flushes logs
///
/// Returns when all cleanup is complete.
async fn shutdown(AuthenticatedState(state): AuthenticatedState) -> impl IntoResponse {
    state.update_activity().await;

    state
        .log_activity("shutdown", Some("Starting graceful shutdown".to_string()))
        .await;

    let start = std::time::Instant::now();
    let mut steps: Vec<ShutdownStep> = Vec::new();
    let mut overall_success = true;

    // Step 1: Run pre_down hook if configured
    if let Some(config) = ProjectSetupManager::load_config() {
        if let Some(ref hook) = config.hooks.pre_down {
            tracing::info!("Running pre_down hook");

            let result = run_hook_command(hook).await;
            let success = result.is_ok();
            if !success {
                overall_success = false;
            }

            steps.push(ShutdownStep {
                name: "pre_down_hook".to_string(),
                success,
                message: result.unwrap_or_else(|e| e),
            });
        }
    }

    // Step 2: Stop docker-compose services
    if ComposeManager::is_available().await {
        let manager = ComposeManager::new(None);

        if manager.has_compose_file() {
            tracing::info!("Stopping docker-compose services");

            let result = manager.down(None, false).await;

            steps.push(ShutdownStep {
                name: "docker_compose_down".to_string(),
                success: result.success,
                message: result.message,
            });

            if !result.success {
                overall_success = false;
            }
        } else {
            steps.push(ShutdownStep {
                name: "docker_compose_down".to_string(),
                success: true,
                message: "No compose file found, skipped".to_string(),
            });
        }
    } else {
        steps.push(ShutdownStep {
            name: "docker_compose_down".to_string(),
            success: true,
            message: "Docker Compose not available, skipped".to_string(),
        });
    }

    // Step 3: Stop any standalone Docker containers started by spuff
    if DockerManager::is_available().await {
        match DockerManager::list_containers().await {
            Ok(containers) => {
                let spuff_containers: Vec<_> = containers
                    .iter()
                    .filter(|c| c.name.starts_with("spuff-") && c.state == "running")
                    .collect();

                if !spuff_containers.is_empty() {
                    let mut stopped = 0;
                    let mut failed = 0;

                    for container in spuff_containers {
                        let result = DockerManager::stop_container(&container.name, Some(10)).await;
                        if result.success {
                            stopped += 1;
                        } else {
                            failed += 1;
                        }
                    }

                    steps.push(ShutdownStep {
                        name: "docker_containers_stop".to_string(),
                        success: failed == 0,
                        message: format!("Stopped {} containers, {} failed", stopped, failed),
                    });

                    if failed > 0 {
                        overall_success = false;
                    }
                } else {
                    steps.push(ShutdownStep {
                        name: "docker_containers_stop".to_string(),
                        success: true,
                        message: "No spuff containers to stop".to_string(),
                    });
                }
            }
            Err(e) => {
                steps.push(ShutdownStep {
                    name: "docker_containers_stop".to_string(),
                    success: false,
                    message: format!("Failed to list containers: {}", e),
                });
                overall_success = false;
            }
        }
    }

    // Step 4: Sync and flush filesystem
    tracing::info!("Syncing filesystem");
    let sync_result = tokio::process::Command::new("sync").output().await;

    steps.push(ShutdownStep {
        name: "filesystem_sync".to_string(),
        success: sync_result.is_ok() && sync_result.as_ref().unwrap().status.success(),
        message: if sync_result.is_ok() && sync_result.as_ref().unwrap().status.success() {
            "Filesystem synced".to_string()
        } else {
            "Sync command failed".to_string()
        },
    });

    let duration_ms = start.elapsed().as_millis() as u64;

    state
        .log_activity(
            "shutdown_complete",
            Some(format!(
                "success={} duration={}ms",
                overall_success, duration_ms
            )),
        )
        .await;

    Json(ShutdownResponse {
        success: overall_success,
        message: if overall_success {
            "Graceful shutdown completed successfully".to_string()
        } else {
            "Graceful shutdown completed with some failures".to_string()
        },
        steps,
        duration_ms,
    })
}

/// Helper to run a hook command and capture result.
async fn run_hook_command(command: &str) -> Result<String, String> {
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .map_err(|e| format!("Failed to execute hook: {}", e))?;

    if output.status.success() {
        Ok("Hook completed successfully (exit code: 0)".to_string())
    } else {
        Err(format!(
            "Hook failed with exit code: {:?}",
            output.status.code()
        ))
    }
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
