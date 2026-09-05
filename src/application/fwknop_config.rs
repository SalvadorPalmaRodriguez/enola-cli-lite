use crate::domain::error::{EnolaError, Result};
use crate::ports::file::{AtomicFilePort, FileManagerPort};
use crate::ports::service::ServiceManagerPort;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FwknopAccessConfig {
    pub source: String,     // e.g. "ANY"
    pub open_ports: String, // e.g. "tcp/2222"
    pub gpg_home: String,   // e.g. "/root/.gnupg"
    pub gpg_decrypt_id: String,
    pub enable_ipt_auto_rules: bool,
    pub fw_access_timeout: u32,
}

impl Default for FwknopAccessConfig {
    fn default() -> Self {
        Self {
            source: "ANY".to_string(),
            open_ports: "tcp/2222".to_string(),
            gpg_home: "/root/.gnupg".to_string(),
            gpg_decrypt_id: "".to_string(),
            enable_ipt_auto_rules: true,
            fw_access_timeout: 30,
        }
    }
}

pub struct FwknopConfig {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    atomic_file: Arc<dyn AtomicFilePort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    access_conf_path: PathBuf,
    template_path: PathBuf,
}

impl FwknopConfig {
    pub fn new(
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
        atomic_file: Arc<dyn AtomicFilePort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            file_manager,
            atomic_file,
            service_manager,
            access_conf_path: PathBuf::from("/etc/fwknop/access.conf"),
            template_path: PathBuf::from("/usr/share/enola-server/templates/fwknop"),
        }
    }

    pub async fn apply_config(&self, config: FwknopAccessConfig) -> Result<()> {
        if !self.template_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Fwknop template not found at {:?}",
                self.template_path
            )));
        }

        // Backup existing config
        if self.access_conf_path.exists() {
            let backup_path = self.access_conf_path.with_extension("conf.bak");
            self.file_manager
                .copy_file(&self.access_conf_path, &backup_path)
                .await?;
        }

        let mut content = self.file_manager.read_file(&self.template_path).await?;

        // Replace placeholders
        content = content.replace("$SOURCE", &config.source);
        content = content.replace("$OPEN_PORTS", &config.open_ports);
        content = content.replace("$GPG_HOME", &config.gpg_home);
        content = content.replace("$GPG_DECRYPT_ID", &config.gpg_decrypt_id);
        content = content.replace(
            "$ENABLE_IPT_AUTO_RULES",
            if config.enable_ipt_auto_rules {
                "Y"
            } else {
                "N"
            },
        );
        content = content.replace("$FW_ACCESS_TIMEOUT", &config.fw_access_timeout.to_string());

        // Write atomically with 0o600 (no TOCTOU window)
        self.atomic_file
            .write_atomic(&self.access_conf_path, content.as_bytes(), 0o600)
            .await?;

        // Restart service
        self.service_manager.restart_service("fwknopd").await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::file::MockAtomicFilePort;

    #[test]
    fn test_default_config() {
        let config = FwknopAccessConfig::default();
        assert_eq!(config.source, "ANY");
        assert_eq!(config.open_ports, "tcp/2222");
        assert!(config.enable_ipt_auto_rules);
        assert_eq!(config.fw_access_timeout, 30);
    }

    #[test]
    fn test_config_serialization() {
        let config = FwknopAccessConfig {
            source: "192.168.1.0/24".to_string(),
            open_ports: "tcp/22,tcp/443".to_string(),
            gpg_home: "/root/.gnupg".to_string(),
            gpg_decrypt_id: "ABCDEF12".to_string(),
            enable_ipt_auto_rules: false,
            fw_access_timeout: 60,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(json.contains("192.168.1.0/24"));
        assert!(json.contains("ABCDEF12"));
    }

    #[tokio::test]
    async fn test_apply_config_template_not_found() {
        use crate::ports::file::MockFileManagerPort;
        use crate::ports::service::MockServiceManagerPort;

        let mock_file = MockFileManagerPort::new();
        let mock_atomic = MockAtomicFilePort::new();
        let mock_svc = MockServiceManagerPort::new();
        let svc = FwknopConfig::new(
            Arc::new(mock_file),
            Arc::new(mock_atomic),
            Arc::new(mock_svc),
        );
        // Template path doesn't exist, should return NotFound
        let result = svc.apply_config(FwknopAccessConfig::default()).await;
        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(msg)) => assert!(msg.contains("template")),
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }
}
