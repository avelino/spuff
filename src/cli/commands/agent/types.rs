//! Agent response types
//!
//! Structs for deserializing agent HTTP API responses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AgentStatus {
    pub uptime_seconds: i64,
    pub idle_seconds: i64,
    pub hostname: String,
    pub cloud_init_done: bool,
    #[serde(default)]
    pub bootstrap_status: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub bootstrap_ready: bool,
    pub agent_version: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentMetrics {
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_percent: f32,
    pub load_avg: LoadAverage,
    pub hostname: String,
    pub os: String,
    pub cpus: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
}

#[derive(Debug, Deserialize)]
pub struct ActivityLogEntry {
    pub timestamp: String,
    pub event: String,
    pub details: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActivityLogResponse {
    pub entries: Vec<ActivityLogEntry>,
    #[allow(dead_code)]
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ExecLogEntry {
    pub timestamp: String,
    pub event: String,
    pub details: String,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecLogResponse {
    pub entries: Vec<ExecLogEntry>,
    #[allow(dead_code)]
    pub count: usize,
}

/// Response from agent's /exec endpoint.
#[derive(Debug, Deserialize)]
pub struct AgentExecResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    #[allow(dead_code)]
    pub duration_ms: u64,
}
