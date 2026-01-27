//! System metrics collection for the spuff-agent.
//!
//! This module provides real-time system information including CPU, memory,
//! disk, network, and load average metrics.

use serde::Serialize;
use sysinfo::{Disks, Networks, System};

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
    /// Disk I/O statistics.
    pub disk_io: DiskIoStats,
    /// Network I/O statistics.
    pub network_io: NetworkIoStats,
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

/// Disk I/O statistics.
#[derive(Debug, Clone, Serialize, Default)]
pub struct DiskIoStats {
    /// Total bytes read since boot.
    pub read_bytes: u64,
    /// Total bytes written since boot.
    pub write_bytes: u64,
}

/// Network I/O statistics (aggregated across all interfaces).
#[derive(Debug, Clone, Serialize, Default)]
pub struct NetworkIoStats {
    /// Total bytes received since boot.
    pub rx_bytes: u64,
    /// Total bytes transmitted since boot.
    pub tx_bytes: u64,
    /// Total packets received since boot.
    pub rx_packets: u64,
    /// Total packets transmitted since boot.
    pub tx_packets: u64,
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
            .map_or((0, 0), |d| {
                (d.total_space(), d.total_space() - d.available_space())
            });

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

        // Collect disk I/O stats from /proc/diskstats (Linux)
        let disk_io = Self::collect_disk_io();

        // Collect network I/O stats
        let network_io = Self::collect_network_io();

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
            disk_io,
            network_io,
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

    /// Collect disk I/O statistics from /proc/diskstats.
    fn collect_disk_io() -> DiskIoStats {
        // On Linux, read from /proc/diskstats
        // Format: major minor name reads_completed reads_merged sectors_read time_reading
        //         writes_completed writes_merged sectors_written time_writing ...
        // Sector size is typically 512 bytes
        const SECTOR_SIZE: u64 = 512;

        let content = match std::fs::read_to_string("/proc/diskstats") {
            Ok(c) => c,
            Err(_) => return DiskIoStats::default(),
        };

        let mut total_read_sectors: u64 = 0;
        let mut total_write_sectors: u64 = 0;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 14 {
                continue;
            }

            let device_name = parts[2];

            // Only count physical devices (sda, vda, nvme0n1) not partitions (sda1, vda1, nvme0n1p1)
            let is_physical = (device_name.starts_with("sd") || device_name.starts_with("vd"))
                && device_name.len() == 3
                || (device_name.starts_with("nvme")
                    && device_name.ends_with("n1")
                    && !device_name.contains('p'));

            if is_physical {
                // Field 6 is sectors read, field 10 is sectors written
                if let (Ok(read), Ok(write)) = (parts[5].parse::<u64>(), parts[9].parse::<u64>()) {
                    total_read_sectors += read;
                    total_write_sectors += write;
                }
            }
        }

        DiskIoStats {
            read_bytes: total_read_sectors * SECTOR_SIZE,
            write_bytes: total_write_sectors * SECTOR_SIZE,
        }
    }

    /// Collect network I/O statistics using sysinfo.
    fn collect_network_io() -> NetworkIoStats {
        let networks = Networks::new_with_refreshed_list();

        let mut rx_bytes: u64 = 0;
        let mut tx_bytes: u64 = 0;
        let mut rx_packets: u64 = 0;
        let mut tx_packets: u64 = 0;

        for (_name, network) in &networks {
            rx_bytes += network.total_received();
            tx_bytes += network.total_transmitted();
            rx_packets += network.total_packets_received();
            tx_packets += network.total_packets_transmitted();
        }

        NetworkIoStats {
            rx_bytes,
            tx_bytes,
            rx_packets,
            tx_packets,
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
        assert!(
            metrics.memory_total > 0,
            "Should have non-zero total memory"
        );
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
        assert!(json.contains("disk_io"));
        assert!(json.contains("network_io"));
        assert!(json.contains("load_avg"));
        assert!(json.contains("hostname"));
        assert!(json.contains("cpus"));
    }

    #[test]
    fn test_disk_io_stats_serialization() {
        let disk_io = DiskIoStats {
            read_bytes: 1024 * 1024 * 100,
            write_bytes: 1024 * 1024 * 50,
        };

        let json = serde_json::to_string(&disk_io).unwrap();

        assert!(json.contains("read_bytes"));
        assert!(json.contains("write_bytes"));
    }

    #[test]
    fn test_network_io_stats_serialization() {
        let network_io = NetworkIoStats {
            rx_bytes: 1024 * 1024 * 200,
            tx_bytes: 1024 * 1024 * 100,
            rx_packets: 10000,
            tx_packets: 5000,
        };

        let json = serde_json::to_string(&network_io).unwrap();

        assert!(json.contains("rx_bytes"));
        assert!(json.contains("tx_bytes"));
        assert!(json.contains("rx_packets"));
        assert!(json.contains("tx_packets"));
    }

    #[test]
    fn test_system_metrics_io_stats() {
        let metrics = SystemMetrics::collect();

        // Verify I/O stats are collected (they're cumulative counters from boot)
        // On non-Linux systems, disk_io might be 0 but network_io should have data
        // Just verify the fields exist and are serializable
        let json = serde_json::to_string(&metrics.disk_io).unwrap();
        assert!(json.contains("read_bytes"));
        assert!(json.contains("write_bytes"));

        let json = serde_json::to_string(&metrics.network_io).unwrap();
        assert!(json.contains("rx_bytes"));
        assert!(json.contains("tx_bytes"));
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
            let expected_percent = (metrics.disk_used as f32 / metrics.disk_total as f32) * 100.0;
            let diff = (metrics.disk_percent - expected_percent).abs();
            assert!(diff < 0.1, "Disk percent calculation should be accurate");
        }
    }
}
