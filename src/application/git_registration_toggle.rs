use crate::domain::error::{EnolaError, Result};
use crate::ports::container::ContainerPort;
use crate::ports::file::FileManagerPort;
use std::path::PathBuf;
use std::sync::Arc;

pub struct GitRegistrationToggle {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    pub(crate) git_data_base: PathBuf,
}

impl GitRegistrationToggle {
    pub fn new(
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
    ) -> Self {
        Self {
            file_manager,
            container_manager,
            git_data_base: PathBuf::from("/srv/enola-git"),
        }
    }

    pub async fn execute(&self, service_name: &str, enable: bool) -> Result<()> {
        let app_ini = self
            .git_data_base
            .join(service_name)
            .join("gitea/conf/app.ini");

        if !app_ini.exists() {
            return Err(EnolaError::NotFound(format!(
                "Forgejo configuration not found at {:?}",
                app_ini
            )));
        }

        let content = self.file_manager.read_file(&app_ini).await?;
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        let mut in_service_section = false;
        let mut found = false;
        let target_line = format!("DISABLE_REGISTRATION = {}", !enable);

        for line in lines.iter_mut() {
            let trimmed = line.trim();
            if trimmed == "[service]" {
                in_service_section = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_service_section = false;
                continue;
            }

            if in_service_section && trimmed.starts_with("DISABLE_REGISTRATION") {
                *line = target_line.clone();
                found = true;
                break;
            }
        }

        if !found {
            // If not found, we need to add it to [service] section
            let mut service_index = None;
            for (i, line) in lines.iter().enumerate() {
                if line.trim() == "[service]" {
                    service_index = Some(i);
                    break;
                }
            }

            if let Some(idx) = service_index {
                lines.insert(idx + 1, target_line);
            } else {
                // If [service] section doesn't exist at all
                lines.push("[service]".to_string());
                lines.push(target_line);
            }
        }

        let new_content = lines.join("\n");
        self.file_manager.write_file(&app_ini, &new_content).await?;

        // Restart container to apply changes
        let container_name = format!("enola-git-{}", service_name);
        self.container_manager
            .restart_container(&container_name)
            .await?;

        Ok(())
    }

    pub async fn is_registration_enabled(&self, service_name: &str) -> Result<bool> {
        let app_ini = self
            .git_data_base
            .join(service_name)
            .join("gitea/conf/app.ini");

        if !app_ini.exists() {
            return Err(EnolaError::NotFound(format!(
                "Forgejo configuration not found at {:?}",
                app_ini
            )));
        }

        let content = self.file_manager.read_file(&app_ini).await?;

        // Buscar DISABLE_REGISTRATION en la sección [service], tolerando
        // variaciones de espacios (Forgejo puede reformatear el app.ini al reiniciar).
        let mut in_service = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[service]" {
                in_service = true;
                continue;
            }
            if trimmed.starts_with('[') {
                if in_service {
                    break;
                }
                continue;
            }
            if in_service {
                let normalized: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
                if normalized.starts_with("DISABLE_REGISTRATION=true") {
                    return Ok(false);
                }
                if normalized.starts_with("DISABLE_REGISTRATION=false") {
                    return Ok(true);
                }
            }
        }
        // Si no se encontró la clave, el valor por defecto de Forgejo es false
        // (registro habilitado)
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;
    use crate::ports::file::MockFileManagerPort;

    #[tokio::test]
    async fn test_execute_config_not_found() {
        let mock_file = MockFileManagerPort::new();
        let mock_container = MockContainerPort::new();
        let svc = GitRegistrationToggle::new(Arc::new(mock_file), Arc::new(mock_container));
        let result = svc.execute("nonexistent", true).await;
        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(msg)) => assert!(msg.contains("Forgejo configuration")),
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_is_registration_enabled_config_not_found() {
        let mock_file = MockFileManagerPort::new();
        let mock_container = MockContainerPort::new();
        let svc = GitRegistrationToggle::new(Arc::new(mock_file), Arc::new(mock_container));
        let result = svc.is_registration_enabled("nonexistent").await;
        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(_)) => {}
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_struct_creation_default_path() {
        let mock_file = MockFileManagerPort::new();
        let mock_container = MockContainerPort::new();
        let svc = GitRegistrationToggle::new(Arc::new(mock_file), Arc::new(mock_container));
        assert_eq!(svc.git_data_base, PathBuf::from("/srv/enola-git"));
    }

    // ── is_registration_enabled: parse lógica pura ───────────────────────────
    // Estos tests usan un directorio temporal real con app.ini para ejercer
    // las ramas del parser sin mocks de filesystem complejo.

    fn make_toggle_with_base(base: &std::path::Path) -> GitRegistrationToggle {
        use crate::adapters::infra::filesystem::EnolaFileAdapter;
        let fm = Arc::new(EnolaFileAdapter::new());
        let mc = Arc::new(MockContainerPort::new());
        let mut svc = GitRegistrationToggle::new(fm, mc);
        svc.git_data_base = base.to_path_buf();
        svc
    }

    async fn write_app_ini(base: &std::path::Path, svc_name: &str, content: &str) {
        let conf_dir = base.join(svc_name).join("gitea").join("conf");
        std::fs::create_dir_all(&conf_dir).unwrap(); // unwrap: test-only
        std::fs::write(conf_dir.join("app.ini"), content).unwrap(); // unwrap: test-only
    }

    #[tokio::test]
    async fn is_registration_enabled_disable_true_returns_false() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let ini = "[service]\nDISABLE_REGISTRATION = true\n";
        write_app_ini(tmp.path(), "myrepo", ini).await;
        let svc = make_toggle_with_base(tmp.path());
        assert!(!svc.is_registration_enabled("myrepo").await.unwrap());
        // unwrap: test-only
    }

    #[tokio::test]
    async fn is_registration_enabled_disable_false_returns_true() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let ini = "[service]\nDISABLE_REGISTRATION = false\n";
        write_app_ini(tmp.path(), "myrepo", ini).await;
        let svc = make_toggle_with_base(tmp.path());
        assert!(svc.is_registration_enabled("myrepo").await.unwrap()); // unwrap: test-only
    }

    #[tokio::test]
    async fn is_registration_enabled_key_absent_defaults_true() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let ini = "[service]\nSOME_KEY = value\n";
        write_app_ini(tmp.path(), "myrepo", ini).await;
        let svc = make_toggle_with_base(tmp.path());
        assert!(svc.is_registration_enabled("myrepo").await.unwrap()); // unwrap: test-only
    }

    #[tokio::test]
    async fn is_registration_enabled_no_service_section_defaults_true() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let ini = "[server]\nHTTP_PORT = 3000\n";
        write_app_ini(tmp.path(), "myrepo", ini).await;
        let svc = make_toggle_with_base(tmp.path());
        assert!(svc.is_registration_enabled("myrepo").await.unwrap()); // unwrap: test-only
    }

    #[tokio::test]
    async fn is_registration_enabled_whitespace_tolerant() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
                                                // Forgejo puede reformatear con espacios variables alrededor del =
        let ini = "[service]\nDISABLE_REGISTRATION  =  true\n";
        write_app_ini(tmp.path(), "myrepo", ini).await;
        let svc = make_toggle_with_base(tmp.path());
        assert!(!svc.is_registration_enabled("myrepo").await.unwrap());
        // unwrap: test-only
    }
}
