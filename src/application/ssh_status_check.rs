use crate::domain::error::Result;
use crate::ports::file::FileManagerPort;
use crate::ports::service::ServiceManagerPort;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct SshStatus {
    pub active: bool,
    pub ports: Vec<u16>,
    pub listening_confirmed: bool,
}

pub struct SshStatusCheck {
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
}

impl SshStatusCheck {
    pub fn new(
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            service_manager,
            file_manager,
        }
    }

    pub async fn execute(&self) -> Result<SshStatus> {
        // 1. Check Service Status
        let mut active = self.service_manager.is_active("ssh").await.unwrap_or(false);
        if !active {
            active = self
                .service_manager
                .is_active("sshd")
                .await
                .unwrap_or(false);
        }

        // 2. Parse Ports
        let ports = self.get_configured_ports().await?;

        // 3. Confirm listening (Optional, requires reading netstat/ss output which is hard from here without command/adapter)
        // For MVP, if config and service is active, we assume ok.
        // Or we could run `ss -ltn` via Command.
        // Since we are not strictly bound to ports for simple checks, I can add a quick check here using std::net::TcpListener?
        // No, that checks if I can BIND. I want to check if SOMEONE is bound.
        // `ss` command is best.
        // But App layer shouldn't run commands directly if possible.
        // Let's rely on active status for now, and maybe a simple check if we can connect to localhost:port?
        // Connecting to localhost:port is a good check.

        let mut listening_confirmed = false;
        if active && !ports.is_empty() {
            // Try connecting to first port
            for port in &ports {
                if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
                    .await
                    .is_ok()
                {
                    listening_confirmed = true;
                    break;
                }
            }
        }

        Ok(SshStatus {
            active,
            ports,
            listening_confirmed,
        })
    }

    async fn get_configured_ports(&self) -> Result<Vec<u16>> {
        let mut ports = Vec::new();
        let config_path = PathBuf::from("/etc/ssh/sshd_config");

        if let Ok(content) = self.file_manager.read_file(&config_path).await {
            for line in content.lines() {
                let trim = line.trim();
                if trim.starts_with("Port ") {
                    if let Some(val) = trim.split_whitespace().nth(1) {
                        if let Ok(p) = val.parse::<u16>() {
                            ports.push(p);
                        }
                    }
                }
            }
        }

        // Also check sshd_config.d/*.conf?
        // FileManagerPort usually handles single files.
        // We skip complex include logic for MVP unless critical.
        // Default to 22 if empty.
        if ports.is_empty() {
            ports.push(22);
        }

        // Dedup
        ports.sort();
        ports.dedup();

        Ok(ports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::service::ServiceMetrics;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::Path;

    struct MockServiceManager {
        ssh_active: bool,
        sshd_active: bool,
    }

    impl MockServiceManager {
        fn new(ssh: bool, sshd: bool) -> Self {
            Self {
                ssh_active: ssh,
                sshd_active: sshd,
            }
        }

        fn both_inactive() -> Self {
            Self {
                ssh_active: false,
                sshd_active: false,
            }
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
        async fn is_active(&self, name: &str) -> Result<bool> {
            match name {
                "ssh" => Ok(self.ssh_active),
                "sshd" => Ok(self.sshd_active),
                _ => Ok(false),
            }
        }
        async fn get_service_metrics(&self, _name: &str) -> Result<ServiceMetrics> {
            Ok(ServiceMetrics::default())
        }
    }

    struct MockFileManager {
        config_content: Option<String>,
    }

    impl MockFileManager {
        fn new() -> Self {
            Self {
                config_content: None,
            }
        }

        fn with_config(content: &str) -> Self {
            Self {
                config_content: Some(content.to_string()),
            }
        }
    }

    #[async_trait]
    impl FileManagerPort for MockFileManager {
        async fn read_file(&self, _path: &Path) -> Result<String> {
            self.config_content
                .clone()
                .ok_or_else(|| EnolaError::NotFound("Config not found".into()))
        }
        async fn write_file(&self, _path: &Path, _content: &str) -> Result<()> {
            Ok(())
        }
        async fn ensure_dir(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        async fn read_env(&self, _path: &Path) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn update_env_key(&self, _path: &Path, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }
        async fn delete_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        async fn copy_file(&self, _from: &Path, _to: &Path) -> Result<()> {
            Ok(())
        }
        async fn set_ownership(&self, _path: &Path, _user: &str, _group: &str) -> Result<()> {
            Ok(())
        }
        async fn set_permissions(&self, _path: &Path, _mode: u32) -> Result<()> {
            Ok(())
        }
        async fn create_archive(&self, _source_dir: &Path, _dest_file: &Path) -> Result<()> {
            Ok(())
        }
        async fn extract_archive(&self, _archive: &Path, _dest_dir: &Path) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_ssh_status_active_via_ssh() {
        let service = Arc::new(MockServiceManager::new(true, false));
        let file = Arc::new(MockFileManager::new());
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.active);
    }

    #[tokio::test]
    async fn test_ssh_status_active_via_sshd() {
        let service = Arc::new(MockServiceManager::new(false, true));
        let file = Arc::new(MockFileManager::new());
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.active);
    }

    #[tokio::test]
    async fn test_ssh_status_inactive() {
        let service = Arc::new(MockServiceManager::both_inactive());
        let file = Arc::new(MockFileManager::new());
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(!status.active);
    }

    #[tokio::test]
    async fn test_ssh_status_default_port() {
        let service = Arc::new(MockServiceManager::new(true, false));
        let file = Arc::new(MockFileManager::new()); // No config, use default
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.ports.contains(&22));
    }

    #[tokio::test]
    async fn test_ssh_status_custom_port() {
        let config = r#"
# SSH Config
Port 2222
PermitRootLogin no
"#;
        let service = Arc::new(MockServiceManager::new(true, false));
        let file = Arc::new(MockFileManager::with_config(config));
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.ports, vec![2222]);
    }

    #[tokio::test]
    async fn test_ssh_status_multiple_ports() {
        let config = r#"
Port 22
Port 2222
Port 8022
"#;
        let service = Arc::new(MockServiceManager::new(true, false));
        let file = Arc::new(MockFileManager::with_config(config));
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.ports.len(), 3);
        assert!(status.ports.contains(&22));
        assert!(status.ports.contains(&2222));
        assert!(status.ports.contains(&8022));
    }

    #[tokio::test]
    async fn test_ssh_status_dedup_ports() {
        let config = r#"
Port 22
Port 22
Port 22
"#;
        let service = Arc::new(MockServiceManager::new(true, false));
        let file = Arc::new(MockFileManager::with_config(config));
        let checker = SshStatusCheck::new(service, file);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.ports.len(), 1);
        assert_eq!(status.ports[0], 22);
    }

    #[test]
    fn test_ssh_status_serialization() {
        let status = SshStatus {
            active: true,
            ports: vec![22, 2222],
            listening_confirmed: false,
        };

        let json = serde_json::to_string(&status);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"active\":true"));
        assert!(json_str.contains("\"ports\":[22,2222]"));
    }
}
