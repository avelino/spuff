//! System metrics collection for the spuff-agent.
//!
//! This module provides real-time system information including CPU, memory,
//! disk, and load average metrics.

use serde::Serialize;
use sysinfo::{Disks, System};

/// System-wide metrics snapshot.
///
/// All memory and disk values are in bytes.
#[derive(Debug, Clone, Serialize)]
pub struct SystemMetrics {
    /// Current CPU usage as a percentage (0-100).
    pub cpu_usage: f32,
    /// Total physical memory in bytes.
    pub memory_total: u64,
    /// Used physical memory in bytes.
    pub memory_used: u64,
    /// Memory usage as a percentage (0-100).
    pub memory_percent: f32,
    /// Total swap space in bytes.
    pub swap_total: u64,
    /// Used swap space in bytes.
    pub swap_used: u64,
    /// Total disk space on root filesystem in bytes.
    pub disk_total: u64,
    /// Used disk space on root filesystem in bytes.
    pub disk_used: u64,
    /// Disk usage as a percentage (0-100).
    pub disk_percent: f32,
    /// System load averages.
    pub load_avg: LoadAverage,
    /// System hostname.
    pub hostname: String,
    /// Operating system name and version.
    pub os: String,
    /// Kernel version.
    pub kernel: String,
    /// Number of logical CPUs.
    pub cpus: usize,
}

/// System load averages for 1, 5, and 15 minute intervals.
#[derive(Debug, Clone, Serialize)]
pub struct LoadAverage {
    /// 1-minute load average.
    pub one: f64,
    /// 5-minute load average.
    pub five: f64,
    /// 15-minute load average.
    pub fifteen: f64,
}

impl SystemMetrics {
    /// Collects current system metrics.
    ///
    /// This function performs blocking I/O to read system information.
    /// Consider calling from a blocking task pool in async contexts.
    pub fn collect() -> Self {
        // new_all() already populates all system information
        let sys = System::new_all();

        let disks = Disks::new_with_refreshed_list();
        let (disk_total, disk_used) = disks
            .iter()
            .find(|d| d.mount_point() == std::path::Path::new("/"))
            .map_or((0, 0), |d| (d.total_space(), d.total_space() - d.available_space()));

        let memory_total = sys.total_memory();
        let memory_used = sys.used_memory();
        let memory_percent = if memory_total > 0 {
            (memory_used as f32 / memory_total as f32) * 100.0
        } else {
            0.0
        };

        let disk_percent = if disk_total > 0 {
            (disk_used as f32 / disk_total as f32) * 100.0
        } else {
            0.0
        };

        let load = System::load_average();

        Self {
            cpu_usage: sys.global_cpu_usage(),
            memory_total,
            memory_used,
            memory_percent,
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            disk_total,
            disk_used,
            disk_percent,
            load_avg: LoadAverage {
                one: load.one,
                five: load.five,
                fifteen: load.fifteen,
            },
            hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            os: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
            kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
            cpus: sys.cpus().len(),
        }
    }
}

/// Information about a running process.
#[derive(Debug, Serialize)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: u32,
    /// Process name.
    pub name: String,
    /// CPU usage as a percentage.
    pub cpu_usage: f32,
    /// Memory usage in bytes.
    pub memory: u64,
}

/// Returns the top N processes sorted by CPU usage (descending).
///
/// # Arguments
///
/// * `limit` - Maximum number of processes to return.
pub fn get_top_processes(limit: usize) -> Vec<ProcessInfo> {
    let sys = System::new_all();

    let mut processes: Vec<_> = sys
        .processes()
        .iter()
        .map(|(pid, proc)| ProcessInfo {
            pid: pid.as_u32(),
            name: proc.name().to_string_lossy().into_owned(),
            cpu_usage: proc.cpu_usage(),
            memory: proc.memory(),
        })
        .collect();

    // Sort by CPU usage descending
    processes.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(limit);
    processes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_metrics_collect() {
        let metrics = SystemMetrics::collect();

        assert!(metrics.cpus > 0, "Should have at least one CPU");
        assert!(metrics.memory_total > 0, "Should have non-zero total memory");
        assert!((0.0..=100.0).contains(&metrics.memory_percent));
        assert!((0.0..=100.0).contains(&metrics.disk_percent));
    }

    #[test]
    fn test_system_metrics_load_average() {
        let metrics = SystemMetrics::collect();

        assert!(metrics.load_avg.one >= 0.0);
        assert!(metrics.load_avg.five >= 0.0);
        assert!(metrics.load_avg.fifteen >= 0.0);
    }

    #[test]
    fn test_system_metrics_serialization() {
        let metrics = SystemMetrics::collect();
        let json = serde_json::to_string(&metrics).unwrap();

        assert!(json.contains("cpu_usage"));
        assert!(json.contains("memory_total"));
        assert!(json.contains("memory_used"));
        assert!(json.contains("memory_percent"));
        assert!(json.contains("disk_total"));
        assert!(json.contains("disk_used"));
        assert!(json.contains("disk_percent"));
        assert!(json.contains("load_avg"));
        assert!(json.contains("hostname"));
        assert!(json.contains("cpus"));
    }

    #[test]
    fn test_get_top_processes_limit() {
        let processes = get_top_processes(5);
        assert!(processes.len() <= 5);
    }

    #[test]
    fn test_get_top_processes_sorted_by_cpu() {
        let processes = get_top_processes(10);

        for i in 1..processes.len() {
            assert!(
                processes[i - 1].cpu_usage >= processes[i].cpu_usage,
                "Processes should be sorted by CPU usage in descending order"
            );
        }
    }

    #[test]
    fn test_process_info_serialization() {
        let proc = ProcessInfo {
            pid: 1234,
            name: "test_process".to_string(),
            cpu_usage: 25.5,
            memory: 1024 * 1024 * 100,
        };

        let json = serde_json::to_string(&proc).unwrap();

        assert!(json.contains("1234"));
        assert!(json.contains("test_process"));
        assert!(json.contains("25.5"));
    }

    #[test]
    fn test_load_average_serialization() {
        let load = LoadAverage {
            one: 1.5,
            five: 2.0,
            fifteen: 1.75,
        };

        let json = serde_json::to_string(&load).unwrap();

        assert!(json.contains("one"));
        assert!(json.contains("five"));
        assert!(json.contains("fifteen"));
    }

    #[test]
    fn test_memory_calculations() {
        let metrics = SystemMetrics::collect();

        assert!(metrics.memory_used <= metrics.memory_total);

        if metrics.memory_total > 0 {
            let expected_percent =
                (metrics.memory_used as f32 / metrics.memory_total as f32) * 100.0;
            let diff = (metrics.memory_percent - expected_percent).abs();
            assert!(diff < 0.1, "Memory percent calculation should be accurate");
        }
    }

    #[test]
    fn test_disk_calculations() {
        let metrics = SystemMetrics::collect();

        assert!(metrics.disk_used <= metrics.disk_total);

        if metrics.disk_total > 0 {
            let expected_percent =
                (metrics.disk_used as f32 / metrics.disk_total as f32) * 100.0;
            let diff = (metrics.disk_percent - expected_percent).abs();
            assert!(diff < 0.1, "Disk percent calculation should be accurate");
        }
    }
}
