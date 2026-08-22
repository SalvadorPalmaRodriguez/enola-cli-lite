use crate::application::system_health_check::{HealthStatus, SystemHealthCheck};
use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use crate::ports::hardware::{HardwareProbePort, SystemHardwareSpecs};
use crate::ports::service::{ServiceManagerPort, ServiceMetrics};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct FullResourceReport {
    pub system: HealthStatus,
    pub hardware: Option<SystemHardwareSpecs>,
    pub services: HashMap<String, ServiceMetrics>,
    pub containers: Vec<ContainerResourceInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerResourceInfo {
    pub name: String,
    pub status: String,
    pub cpu_percent: f32,  // If available
    pub memory_usage: u64, // If available
}

pub struct SystemResourceMonitor {
    system_check: Arc<SystemHealthCheck>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    hardware_probe: Option<Arc<dyn HardwareProbePort>>,
}

impl SystemResourceMonitor {
    pub fn new(
        system_check: Arc<SystemHealthCheck>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        hardware_probe: Option<Arc<dyn HardwareProbePort>>,
    ) -> Self {
        Self {
            system_check,
            service_manager,
            container_manager,
            hardware_probe,
        }
    }

    pub async fn execute(&self) -> Result<FullResourceReport> {
        // 1. Global System Health (Basic)
        let system = self.system_check.execute().await?;

        // 2. Advanced Hardware Probe (if available)
        let hardware = if let Some(probe) = &self.hardware_probe {
            probe.probe().await.ok()
        } else {
            None
        };
        let services_to_check = vec!["tor", "nginx", "ssh", "cron"]; // Add others as needed
        let mut services = HashMap::new();

        for name in services_to_check {
            if let Ok(metrics) = self.service_manager.get_service_metrics(name).await {
                // Only add if non-zero? Or always?
                services.insert(name.to_string(), metrics);
            } else {
                // Try fallback names, e.g. sshd
                if name == "ssh" {
                    if let Ok(metrics) = self.service_manager.get_service_metrics("sshd").await {
                        services.insert("ssh".to_string(), metrics);
                    }
                }
            }
        }

        // 3. Container Metrics — use get_container_stats for real CPU/memory
        let mut container_infos = Vec::new();
        if let Ok(containers) = self.container_manager.list_containers(false).await {
            for c in containers {
                let (cpu_percent, memory_usage) = match self
                    .container_manager
                    .get_container_stats(&c.id)
                    .await
                {
                    Ok(stats) => (stats.cpu_percent, stats.memory_usage),
                    Err(_) => {
                        tracing::warn!("Failed to get stats for container {}, using zeros", c.name);
                        (0.0, 0)
                    }
                };
                container_infos.push(ContainerResourceInfo {
                    name: c.name,
                    status: c.status,
                    cpu_percent,
                    memory_usage,
                });
            }
        }

        Ok(FullResourceReport {
            system,
            hardware,
            services,
            containers: container_infos,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::system_health_check::DiskStatus;
    use crate::ports::container::{ContainerConfig, ContainerInfo, ContainerStats};
    use async_trait::async_trait;

    struct MockServiceManager {
        metrics: HashMap<String, ServiceMetrics>,
    }

    impl MockServiceManager {
        fn new() -> Self {
            Self {
                metrics: HashMap::new(),
            }
        }

        fn with_metrics(metrics: HashMap<String, ServiceMetrics>) -> Self {
            Self { metrics }
        }
    }

    #[async_trait]
    impl ServiceManagerPort for MockServiceManager {
        async fn start_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn is_active(&self, _name: &str) -> Result<bool> {
            Ok(true)
        }
        async fn get_service_metrics(&self, name: &str) -> Result<ServiceMetrics> {
            self.metrics.get(name).cloned().ok_or_else(|| {
                crate::domain::error::EnolaError::NotFound(format!("Service {} not found", name))
            })
        }
    }

    struct MockContainerManager {
        containers: Vec<ContainerInfo>,
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self { containers: vec![] }
        }

        fn with_containers(containers: Vec<ContainerInfo>) -> Self {
            Self { containers }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManager {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            Ok(self.containers.clone())
        }
        async fn create_container(&self, _config: ContainerConfig) -> Result<String> {
            Ok("test".into())
        }
        async fn start_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn get_logs(&self, _id: &str, _tail: usize) -> Result<String> {
            Ok("".into())
        }
        async fn inspect_container(&self, _id: &str) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn execute_command(&self, _id: &str, _cmd: Vec<String>) -> Result<String> {
            Ok("".into())
        }
        async fn create_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn connect_container_to_network(
            &self,
            _network: &str,
            _container: &str,
        ) -> Result<()> {
            Ok(())
        }
        async fn image_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        async fn build_image(
            &self,
            _: crate::ports::container::ImageBuildConfig,
        ) -> Result<String> {
            Ok("mock:latest".into())
        }
        async fn run_ephemeral_container(&self, _: ContainerConfig) -> Result<(i64, String)> {
            Ok((0, "".into()))
        }
        async fn prune_system(&self) -> Result<()> {
            Ok(())
        }
        async fn pull_image(&self, _image: &str) -> Result<()> {
            Ok(())
        }
        async fn get_container_stats(&self, _id: &str) -> Result<ContainerStats> {
            Ok(ContainerStats::default())
        }
    }

    fn make_container(name: &str, status: &str) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{}", name),
            name: name.to_string(),
            image: "test:latest".to_string(),
            status: status.to_string(),
            ports: vec![],
        }
    }

    #[tokio::test]
    async fn test_system_resource_monitor_basic() {
        let system_check = Arc::new(SystemHealthCheck::new());
        let service = Arc::new(MockServiceManager::new());
        let container = Arc::new(MockContainerManager::new());
        let monitor = SystemResourceMonitor::new(system_check, service, container, None);

        let result = monitor.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(["ok", "warning", "critical"].contains(&report.system.overall.as_str()));
    }

    #[tokio::test]
    async fn test_system_resource_monitor_with_services() {
        let system_check = Arc::new(SystemHealthCheck::new());
        let mut metrics = HashMap::new();
        metrics.insert(
            "tor".to_string(),
            ServiceMetrics {
                cpu_percent: 5.0,
                memory_bytes: 100_000,
            },
        );
        metrics.insert(
            "nginx".to_string(),
            ServiceMetrics {
                cpu_percent: 2.0,
                memory_bytes: 50_000,
            },
        );
        let service = Arc::new(MockServiceManager::with_metrics(metrics));
        let container = Arc::new(MockContainerManager::new());
        let monitor = SystemResourceMonitor::new(system_check, service, container, None);

        let result = monitor.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.services.contains_key("tor"));
        assert!(report.services.contains_key("nginx"));
        assert_eq!(report.services.get("tor").unwrap().cpu_percent, 5.0);
    }

    struct MockContainerManagerWithStats {
        containers: Vec<ContainerInfo>,
        stats: ContainerStats,
    }

    impl MockContainerManagerWithStats {
        fn with_stats(containers: Vec<ContainerInfo>, stats: ContainerStats) -> Self {
            Self { containers, stats }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManagerWithStats {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            Ok(self.containers.clone())
        }
        async fn create_container(&self, _config: ContainerConfig) -> Result<String> {
            Ok("test".into())
        }
        async fn start_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn get_logs(&self, _id: &str, _tail: usize) -> Result<String> {
            Ok("".into())
        }
        async fn inspect_container(&self, _id: &str) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn execute_command(&self, _id: &str, _cmd: Vec<String>) -> Result<String> {
            Ok("".into())
        }
        async fn create_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn connect_container_to_network(
            &self,
            _network: &str,
            _container: &str,
        ) -> Result<()> {
            Ok(())
        }
        async fn image_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        async fn build_image(
            &self,
            _: crate::ports::container::ImageBuildConfig,
        ) -> Result<String> {
            Ok("mock:latest".into())
        }
        async fn run_ephemeral_container(&self, _: ContainerConfig) -> Result<(i64, String)> {
            Ok((0, "".into()))
        }
        async fn prune_system(&self) -> Result<()> {
            Ok(())
        }
        async fn pull_image(&self, _image: &str) -> Result<()> {
            Ok(())
        }
        async fn get_container_stats(&self, _id: &str) -> Result<ContainerStats> {
            Ok(self.stats.clone())
        }
    }

    #[tokio::test]
    async fn test_resource_monitor_has_real_metrics() {
        let system_check = Arc::new(SystemHealthCheck::new());
        let service = Arc::new(MockServiceManager::new());
        let containers = vec![make_container("wordpress", "Up 2 hours")];
        let stats = ContainerStats {
            cpu_percent: 42.5,
            memory_usage: 536_870_912,
            memory_limit: 1_073_741_824,
        };
        let container = Arc::new(MockContainerManagerWithStats::with_stats(containers, stats));
        let monitor = SystemResourceMonitor::new(system_check, service, container, None);

        let result = monitor.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.containers.len(), 1);
        assert_eq!(report.containers[0].cpu_percent, 42.5);
        assert_eq!(report.containers[0].memory_usage, 536_870_912);
    }

    #[tokio::test]
    async fn test_system_resource_monitor_with_containers() {
        let system_check = Arc::new(SystemHealthCheck::new());
        let service = Arc::new(MockServiceManager::new());
        let containers = vec![
            make_container("wordpress", "Up 2 hours"),
            make_container("mysql", "Up 2 hours"),
        ];
        let container = Arc::new(MockContainerManager::with_containers(containers));
        let monitor = SystemResourceMonitor::new(system_check, service, container, None);

        let result = monitor.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.containers.len(), 2);
        assert_eq!(report.containers[0].name, "wordpress");
        assert_eq!(report.containers[1].name, "mysql");
    }

    #[test]
    fn test_container_resource_info_serialization() {
        let info = ContainerResourceInfo {
            name: "test".to_string(),
            status: "Up".to_string(),
            cpu_percent: 10.5,
            memory_usage: 1024,
        };

        let json = serde_json::to_string(&info);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"name\":\"test\""));
        assert!(json_str.contains("\"cpu_percent\":10.5"));
    }

    #[test]
    fn test_full_resource_report_serialization() {
        let report = FullResourceReport {
            system: HealthStatus {
                overall: "ok".to_string(),
                cpu_usage: 25.0,
                memory_used: 4_000_000_000,
                memory_total: 8_000_000_000,
                disk_usage: vec![DiskStatus {
                    mount_point: "/".to_string(),
                    total_space: 500_000_000_000,
                    available_space: 250_000_000_000,
                }],
            },
            hardware: None,
            services: HashMap::new(),
            containers: vec![],
        };

        let json = serde_json::to_string(&report);
        assert!(json.is_ok());
    }
}
