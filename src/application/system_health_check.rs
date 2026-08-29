use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub overall: String, // "ok", "warning", "critical"
    pub cpu_usage: f32,  // %
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_usage: Vec<DiskStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiskStatus {
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
}

pub struct SystemHealthCheck;

impl Default for SystemHealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemHealthCheck {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self) -> Result<HealthStatus> {
        // Gathering info is usually sync CPU bound, so spawn_blocking
        let status = tokio::task::spawn_blocking(|| {
            let mut sys = System::new_all();
            sys.refresh_all();
            let disks = Disks::new_with_refreshed_list();

            let cpu_usage = sys.global_cpu_info().cpu_usage();
            let memory_used = sys.used_memory();
            let memory_total = sys.total_memory();

            let mut disk_stats = Vec::new();
            for disk in disks.list() {
                disk_stats.push(DiskStatus {
                    mount_point: disk.mount_point().to_string_lossy().to_string(),
                    total_space: disk.total_space(),
                    available_space: disk.available_space(),
                });
            }

            // Simple validation logic
            let mut overall = "ok";
            if cpu_usage > 90.0 {
                overall = "warning";
            }

            // Check memory
            if memory_total > 0 {
                let mem_pct = memory_used as f64 / memory_total as f64;
                if mem_pct > 0.95 {
                    overall = "critical";
                } else if mem_pct > 0.85 && overall != "critical" {
                    overall = "warning";
                }
            }

            // Check disks
            for d in &disk_stats {
                if d.total_space > 0 {
                    let used_pct = 1.0 - (d.available_space as f64 / d.total_space as f64);
                    if used_pct > 0.95 {
                        overall = "critical";
                    } else if used_pct > 0.85 && overall != "critical" {
                        overall = "warning";
                    }
                }
            }

            HealthStatus {
                overall: overall.to_string(),
                cpu_usage,
                memory_used,
                memory_total,
                disk_usage: disk_stats,
            }
        })
        .await
        .map_err(|e| EnolaError::InfrastructureError(format!("Health check failed: {e}")))?;

        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_returns_status() {
        let health_check = SystemHealthCheck::new();

        let result = health_check.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(["ok", "warning", "critical"].contains(&status.overall.as_str()));
    }

    #[tokio::test]
    async fn test_health_check_has_cpu_usage() {
        let health_check = SystemHealthCheck::new();

        let result = health_check.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        // CPU usage should be between 0 and 100
        assert!(status.cpu_usage >= 0.0 && status.cpu_usage <= 100.0);
    }

    #[tokio::test]
    async fn test_health_check_has_memory_info() {
        let health_check = SystemHealthCheck::new();

        let result = health_check.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        // Memory total should be greater than 0 on any real system
        assert!(status.memory_total > 0);
        // Used should be less than or equal to total
        assert!(status.memory_used <= status.memory_total);
    }

    #[tokio::test]
    async fn test_health_check_has_disk_info() {
        let health_check = SystemHealthCheck::new();

        let result = health_check.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        // On most systems, at least one disk should be present
        // But we don't assert this as some test environments might have none
        for disk in &status.disk_usage {
            assert!(!disk.mount_point.is_empty());
            assert!(disk.available_space <= disk.total_space);
        }
    }

    #[tokio::test]
    async fn test_health_check_default_constructor() {
        let health_check = SystemHealthCheck;

        let result = health_check.execute().await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_health_status_serialization() {
        let status = HealthStatus {
            overall: "ok".to_string(),
            cpu_usage: 25.5,
            memory_used: 4_000_000_000,
            memory_total: 8_000_000_000,
            disk_usage: vec![DiskStatus {
                mount_point: "/".to_string(),
                total_space: 500_000_000_000,
                available_space: 250_000_000_000,
            }],
        };

        let json = serde_json::to_string(&status);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"overall\":\"ok\""));
        assert!(json_str.contains("\"cpu_usage\":25.5"));
    }

    #[test]
    fn test_disk_status_serialization() {
        let disk = DiskStatus {
            mount_point: "/home".to_string(),
            total_space: 1_000_000_000_000,
            available_space: 500_000_000_000,
        };

        let json = serde_json::to_string(&disk);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"/home\""));
    }
}
