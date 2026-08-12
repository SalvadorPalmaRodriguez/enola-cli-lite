use crate::domain::error::Result;
use sysinfo::{Disks, System};

/// Threshold configuration for resource alerts
#[derive(Debug, Clone)]
pub struct AlertThresholds {
    pub ram_warning_percent: f64,
    pub ram_critical_percent: f64,
    pub disk_warning_percent: f64,
    pub disk_critical_percent: f64,
    pub cpu_warning_percent: f64,
    pub cpu_critical_percent: f64,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            ram_warning_percent: 80.0,
            ram_critical_percent: 95.0,
            disk_warning_percent: 85.0,
            disk_critical_percent: 95.0,
            cpu_warning_percent: 80.0,
            cpu_critical_percent: 95.0,
        }
    }
}

/// Alert severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertLevel {
    Ok,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertLevel::Ok => write!(f, "OK"),
            AlertLevel::Warning => write!(f, "WARNING"),
            AlertLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// A single resource alert
#[derive(Debug, Clone)]
pub struct ResourceAlert {
    pub resource: String,
    pub level: AlertLevel,
    pub current_value: f64,
    pub threshold: f64,
    pub message: String,
}

impl std::fmt::Display for ResourceAlert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {:.1}% (threshold: {:.1}%) - {}",
            self.level, self.resource, self.current_value, self.threshold, self.message
        )
    }
}

/// Current system resource status
#[derive(Debug, Clone)]
pub struct ResourceStatus {
    pub ram_used_percent: f64,
    pub ram_total_gb: f64,
    pub ram_used_gb: f64,
    pub disk_used_percent: f64,
    pub disk_total_gb: f64,
    pub disk_used_gb: f64,
    pub cpu_usage_percent: f64,
    pub alerts: Vec<ResourceAlert>,
    pub overall_level: AlertLevel,
}

/// Resource alert monitor
pub struct ResourceAlertMonitor {
    thresholds: AlertThresholds,
}

impl Default for ResourceAlertMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceAlertMonitor {
    pub fn new() -> Self {
        Self {
            thresholds: AlertThresholds::default(),
        }
    }

    pub fn with_thresholds(mut self, thresholds: AlertThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Check current resource status and generate alerts
    pub fn check_resources(&self) -> Result<ResourceStatus> {
        let mut sys = System::new_all();
        sys.refresh_all();

        // RAM
        let total_memory = sys.total_memory() as f64;
        let used_memory = sys.used_memory() as f64;
        let ram_used_percent = if total_memory > 0.0 {
            (used_memory / total_memory) * 100.0
        } else {
            0.0
        };

        // Disk (root partition)
        let disks = Disks::new_with_refreshed_list();
        let (disk_total, disk_used) = disks
            .iter()
            .find(|d| d.mount_point().to_string_lossy() == "/")
            .map(|d| {
                (
                    d.total_space() as f64,
                    (d.total_space() - d.available_space()) as f64,
                )
            })
            .unwrap_or((0.0, 0.0));

        let disk_used_percent = if disk_total > 0.0 {
            (disk_used / disk_total) * 100.0
        } else {
            0.0
        };

        // CPU (average across all cores)
        let cpus = sys.cpus();
        let cpu_usage = if !cpus.is_empty() {
            cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64
        } else {
            0.0
        };

        // Generate alerts
        let mut alerts = Vec::new();
        let mut overall_level = AlertLevel::Ok;

        // RAM alerts
        if ram_used_percent >= self.thresholds.ram_critical_percent {
            alerts.push(ResourceAlert {
                resource: "RAM".to_string(),
                level: AlertLevel::Critical,
                current_value: ram_used_percent,
                threshold: self.thresholds.ram_critical_percent,
                message: "Memory usage critically high! Consider stopping services.".to_string(),
            });
            overall_level = AlertLevel::Critical;
        } else if ram_used_percent >= self.thresholds.ram_warning_percent {
            alerts.push(ResourceAlert {
                resource: "RAM".to_string(),
                level: AlertLevel::Warning,
                current_value: ram_used_percent,
                threshold: self.thresholds.ram_warning_percent,
                message: "Memory usage elevated.".to_string(),
            });
            if overall_level < AlertLevel::Warning {
                overall_level = AlertLevel::Warning;
            }
        }

        // Disk alerts
        if disk_used_percent >= self.thresholds.disk_critical_percent {
            alerts.push(ResourceAlert {
                resource: "Disk".to_string(),
                level: AlertLevel::Critical,
                current_value: disk_used_percent,
                threshold: self.thresholds.disk_critical_percent,
                message: "Disk space critically low! Free up space immediately.".to_string(),
            });
            overall_level = AlertLevel::Critical;
        } else if disk_used_percent >= self.thresholds.disk_warning_percent {
            alerts.push(ResourceAlert {
                resource: "Disk".to_string(),
                level: AlertLevel::Warning,
                current_value: disk_used_percent,
                threshold: self.thresholds.disk_warning_percent,
                message: "Disk space running low.".to_string(),
            });
            if overall_level < AlertLevel::Warning {
                overall_level = AlertLevel::Warning;
            }
        }

        // CPU alerts
        if cpu_usage >= self.thresholds.cpu_critical_percent {
            alerts.push(ResourceAlert {
                resource: "CPU".to_string(),
                level: AlertLevel::Critical,
                current_value: cpu_usage,
                threshold: self.thresholds.cpu_critical_percent,
                message: "CPU usage critically high!".to_string(),
            });
            overall_level = AlertLevel::Critical;
        } else if cpu_usage >= self.thresholds.cpu_warning_percent {
            alerts.push(ResourceAlert {
                resource: "CPU".to_string(),
                level: AlertLevel::Warning,
                current_value: cpu_usage,
                threshold: self.thresholds.cpu_warning_percent,
                message: "CPU usage elevated.".to_string(),
            });
            if overall_level < AlertLevel::Warning {
                overall_level = AlertLevel::Warning;
            }
        }

        Ok(ResourceStatus {
            ram_used_percent,
            ram_total_gb: total_memory / 1024.0 / 1024.0 / 1024.0,
            ram_used_gb: used_memory / 1024.0 / 1024.0 / 1024.0,
            disk_used_percent,
            disk_total_gb: disk_total / 1024.0 / 1024.0 / 1024.0,
            disk_used_gb: disk_used / 1024.0 / 1024.0 / 1024.0,
            cpu_usage_percent: cpu_usage,
            alerts,
            overall_level,
        })
    }

    /// Check if any critical alerts exist
    pub fn has_critical_alerts(&self) -> Result<bool> {
        let status = self.check_resources()?;
        Ok(status.overall_level == AlertLevel::Critical)
    }

    /// Check if any warnings or critical alerts exist
    pub fn has_any_alerts(&self) -> Result<bool> {
        let status = self.check_resources()?;
        Ok(status.overall_level != AlertLevel::Ok)
    }

    /// Get a summary string for display in UI
    pub fn get_status_summary(&self) -> Result<String> {
        let status = self.check_resources()?;

        let alert_indicator = match status.overall_level {
            AlertLevel::Ok => "🟢",
            AlertLevel::Warning => "🟡",
            AlertLevel::Critical => "🔴",
        };

        Ok(format!(
            "{} RAM: {:.1}% | Disk: {:.1}% | CPU: {:.1}%",
            alert_indicator,
            status.ram_used_percent,
            status.disk_used_percent,
            status.cpu_usage_percent
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_thresholds() {
        let thresholds = AlertThresholds::default();
        assert_eq!(thresholds.ram_warning_percent, 80.0);
        assert_eq!(thresholds.disk_critical_percent, 95.0);
    }

    #[test]
    fn test_alert_level_ordering() {
        assert!(AlertLevel::Ok < AlertLevel::Warning);
        assert!(AlertLevel::Warning < AlertLevel::Critical);
    }

    #[test]
    fn test_monitor_creation() {
        let monitor = ResourceAlertMonitor::new();
        assert_eq!(monitor.thresholds.ram_warning_percent, 80.0);
    }

    #[test]
    fn test_monitor_with_custom_thresholds() {
        let custom = AlertThresholds {
            ram_warning_percent: 70.0,
            ..Default::default()
        };
        let monitor = ResourceAlertMonitor::new().with_thresholds(custom);
        assert_eq!(monitor.thresholds.ram_warning_percent, 70.0);
    }

    #[test]
    fn test_check_resources_runs() {
        let monitor = ResourceAlertMonitor::new();
        let result = monitor.check_resources();
        assert!(result.is_ok());

        let status = result.unwrap();
        assert!(status.ram_used_percent >= 0.0);
        assert!(status.disk_used_percent >= 0.0);
    }

    #[test]
    fn test_status_summary() {
        let monitor = ResourceAlertMonitor::new();
        let summary = monitor.get_status_summary();
        assert!(summary.is_ok());
        assert!(summary.unwrap().contains("RAM:"));
    }
}
