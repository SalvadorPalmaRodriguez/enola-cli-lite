use crate::domain::error::{EnolaError, Result};
use crate::ports::service::{ServiceManagerPort, ServiceMetrics, ServiceState};
use std::process::Stdio;
use tokio::process::Command;

pub struct SystemdAdapter;

impl Default for SystemdAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemdAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Helper to execute systemctl commands without shell injection risks
    async fn run_systemctl(&self, args: &[&str]) -> Result<()> {
        let output = Command::new("systemctl")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to spawn systemctl: {}", e))
            })?
            .wait_with_output()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to wait for systemctl: {}", e))
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(EnolaError::InfrastructureError(format!(
                "Systemd error ({}): {}",
                args.join(" "),
                stderr.trim()
            )))
        }
    }

    async fn get_status_impl(&self, name: &str) -> Result<ServiceState> {
        let output = Command::new("systemctl")
            .arg("is-active")
            .arg(name)
            .output()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to check status: {}", e))
            })?;

        let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

        match status_str.as_str() {
            "active" => Ok(ServiceState::Active),
            "inactive" => Ok(ServiceState::Inactive),
            "failed" => Ok(ServiceState::Failed),
            "activating" | "deactivating" => Ok(ServiceState::Active), // Treat transition as active-ish or specific state? Sticking to Enum.
            _ => Ok(ServiceState::Unknown),
        }
    }
}

#[async_trait::async_trait]
impl ServiceManagerPort for SystemdAdapter {
    async fn start_service(&self, name: &str) -> Result<()> {
        self.run_systemctl(&["start", name]).await
    }

    async fn stop_service(&self, name: &str) -> Result<()> {
        self.run_systemctl(&["stop", name]).await
    }

    async fn restart_service(&self, name: &str) -> Result<()> {
        self.run_systemctl(&["restart", name]).await
    }

    async fn enable_service(&self, name: &str) -> Result<()> {
        self.run_systemctl(&["enable", name]).await
    }

    async fn disable_service(&self, name: &str) -> Result<()> {
        self.run_systemctl(&["disable", name]).await
    }

    async fn is_active(&self, name: &str) -> Result<bool> {
        let state = self.get_status_impl(name).await?;
        Ok(state == ServiceState::Active)
    }

    async fn get_service_metrics(&self, name: &str) -> Result<ServiceMetrics> {
        // 1. Get MainPID
        let pid_output = Command::new("systemctl")
            .args(["show", name, "--property=MainPID", "--value"])
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to get PID: {}", e)))?;

        let pid_str = String::from_utf8_lossy(&pid_output.stdout)
            .trim()
            .to_string();
        let pid = pid_str.parse::<u32>().unwrap_or(0);

        if pid == 0 {
            return Ok(ServiceMetrics::default());
        }

        // 2. Get MemoryCurrent from systemd (more accurate than ps rss for cgroups usually, but let's see)
        let mem_output = Command::new("systemctl")
            .args(["show", name, "--property=MemoryCurrent", "--value"])
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to get Memory: {}", e)))?;

        let mem_str = String::from_utf8_lossy(&mem_output.stdout)
            .trim()
            .to_string();
        let memory_bytes = mem_str.parse::<u64>().unwrap_or(0);

        // 3. Get CPU from ps (snapshot)
        // ps -p PID -o %cpu --no-headers
        let ps_output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "%cpu", "--no-headers"])
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to run ps: {}", e)))?;

        let cpu_str = String::from_utf8_lossy(&ps_output.stdout)
            .trim()
            .to_string();
        let cpu_percent = cpu_str.parse::<f32>().unwrap_or(0.0);

        Ok(ServiceMetrics {
            cpu_percent,
            memory_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_constructor() {
        let adapter = SystemdAdapter;
        let _ = adapter;
    }

    #[tokio::test]
    async fn test_is_active_tor() {
        let adapter = SystemdAdapter::new();
        // tor may or may not be active — just verify no panic
        let result = adapter.is_active("tor").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_is_active_nonexistent() {
        let adapter = SystemdAdapter::new();
        let result = adapter.is_active("enola-nonexistent-service-12345").await;
        assert!(result.is_ok());
        // Non-existent service should be inactive
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_get_service_metrics_nonexistent() {
        let adapter = SystemdAdapter::new();
        let metrics = adapter
            .get_service_metrics("enola-nonexistent-service-12345")
            .await;
        assert!(metrics.is_ok());
        // PID should be 0 → default metrics
        let m = metrics.unwrap();
        assert_eq!(m.cpu_percent, 0.0);
    }
}
