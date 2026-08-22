use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct WordPressStatus {
    pub blog_name: String,
    pub wp_container_status: String,
    pub db_container_status: String,
    pub http_status: Option<u16>,
    pub is_healthy: bool,
}

pub struct WordPressStatusCheck {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
}

impl WordPressStatusCheck {
    pub fn new(container_manager: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        Self { container_manager }
    }

    pub async fn execute(&self, blog_name: &str) -> Result<WordPressStatus> {
        let db_container = format!("db-{}", blog_name);
        let wp_container = format!("wp-{}", blog_name);

        let containers = self.container_manager.list_containers(true).await?;

        let db_status = containers
            .iter()
            .find(|c| c.name == db_container || c.name.contains(&db_container))
            .map(|c| c.status.clone())
            .unwrap_or_else(|| "Not found".to_string());

        let wp_info = containers
            .iter()
            .find(|c| c.name == wp_container || c.name.contains(&wp_container))
            .cloned();

        let wp_status = wp_info
            .as_ref()
            .map(|c| c.status.clone())
            .unwrap_or_else(|| "Not found".to_string());

        let wp_running = wp_status.contains("Up");
        let db_running = db_status.contains("Up");

        let http_status = if wp_running {
            if let Some(info) = &wp_info {
                if let Some(port) = extract_host_port(&info.ports) {
                    match reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(5))
                        .build()
                    {
                        Ok(client) => {
                            let url = format!("http://127.0.0.1:{}/", port);
                            match client.get(&url).send().await {
                                Ok(resp) => Some(resp.status().as_u16()),
                                Err(_) => None,
                            }
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let is_healthy = wp_running && db_running && http_status.is_none_or(|s| s < 500);

        Ok(WordPressStatus {
            blog_name: blog_name.to_string(),
            wp_container_status: wp_status,
            db_container_status: db_status,
            http_status,
            is_healthy,
        })
    }
}

fn extract_host_port(ports: &[String]) -> Option<u16> {
    for port_str in ports {
        if let Some(host_part) = port_str.split("->").next() {
            if let Some(port_num) = host_part.rsplit(':').next() {
                if let Ok(port) = port_num.parse::<u16>() {
                    return Some(port);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::container::{ContainerConfig, ContainerInfo, ContainerStats};
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct MockContainerManager {
        containers: Vec<ContainerInfo>,
        should_fail: bool,
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self {
                containers: vec![],
                should_fail: false,
            }
        }

        fn with_containers(containers: Vec<ContainerInfo>) -> Self {
            Self {
                containers,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                containers: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManager {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Docker failed".into()))
            } else {
                Ok(self.containers.clone())
            }
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

    fn make_container_with_ports(name: &str, status: &str, ports: Vec<String>) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{}", name),
            name: name.to_string(),
            image: "test:latest".to_string(),
            status: status.to_string(),
            ports,
        }
    }

    #[test]
    fn test_extract_host_port_valid() {
        let ports = vec!["127.0.0.1:8080->80/tcp".to_string()];
        assert_eq!(extract_host_port(&ports), Some(8080));
    }

    #[test]
    fn test_extract_host_port_empty() {
        let ports: Vec<String> = vec![];
        assert_eq!(extract_host_port(&ports), None);
    }

    #[test]
    fn test_extract_host_port_multiple() {
        let ports = vec![
            "0.0.0.0:9090->9090/tcp".to_string(),
            "127.0.0.1:8080->80/tcp".to_string(),
        ];
        assert_eq!(extract_host_port(&ports), Some(9090));
    }

    #[tokio::test]
    async fn test_wordpress_status_both_running() {
        let containers = vec![
            make_container("wp-myblog", "Up 2 hours"),
            make_container("db-myblog", "Up 2 hours"),
        ];
        let manager = Arc::new(MockContainerManager::with_containers(containers));
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("myblog").await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.is_healthy);
        assert!(status.wp_container_status.contains("Up"));
        assert!(status.db_container_status.contains("Up"));
        assert_eq!(status.http_status, None);
    }

    #[tokio::test]
    async fn test_wordpress_status_wp_down() {
        let containers = vec![
            make_container("wp-myblog", "Exited (1) 5 minutes ago"),
            make_container("db-myblog", "Up 2 hours"),
        ];
        let manager = Arc::new(MockContainerManager::with_containers(containers));
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("myblog").await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(!status.is_healthy);
        assert!(status.http_status.is_none());
    }

    #[tokio::test]
    async fn test_wordpress_status_db_down() {
        let containers = vec![
            make_container("wp-myblog", "Up 2 hours"),
            make_container("db-myblog", "Exited (0) 10 minutes ago"),
        ];
        let manager = Arc::new(MockContainerManager::with_containers(containers));
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("myblog").await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(!status.is_healthy);
    }

    #[tokio::test]
    async fn test_wordpress_status_with_port_no_server() {
        let containers = vec![
            make_container_with_ports(
                "wp-myblog",
                "Up 2 hours",
                vec!["127.0.0.1:18080->80/tcp".to_string()],
            ),
            make_container("db-myblog", "Up 2 hours"),
        ];
        let manager = Arc::new(MockContainerManager::with_containers(containers));
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("myblog").await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.wp_container_status.contains("Up"));
        assert!(status.db_container_status.contains("Up"));
        assert_eq!(status.http_status, None);
        assert!(status.is_healthy);
    }

    #[tokio::test]
    async fn test_wordpress_status_not_found() {
        let manager = Arc::new(MockContainerManager::new());
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("nonexistent").await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(!status.is_healthy);
        assert_eq!(status.wp_container_status, "Not found");
        assert_eq!(status.db_container_status, "Not found");
    }

    #[tokio::test]
    async fn test_wordpress_status_docker_failure() {
        let manager = Arc::new(MockContainerManager::failing());
        let checker = WordPressStatusCheck::new(manager);

        let result = checker.execute("myblog").await;

        assert!(result.is_err());
    }

    #[test]
    fn test_wordpress_status_serialization() {
        let status = WordPressStatus {
            blog_name: "test".to_string(),
            wp_container_status: "Up".to_string(),
            db_container_status: "Up".to_string(),
            http_status: Some(200),
            is_healthy: true,
        };

        let json = serde_json::to_string(&status);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"is_healthy\":true"));
        assert!(json_str.contains("\"blog_name\":\"test\""));
    }
}
