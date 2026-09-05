// DRUPAL-002 (2026-04-29) — Adapter Drupal del catálogo CMS.
//
// Implementación de referencia del trait `CmsLifecycle` que valida la
// abstracción introducida en DRUPAL-001 contra un CMS distinto a WordPress.
//
// Stack:
//   - Web:  `drupal:10-apache`  (oficial)
//   - BD:   `mariadb:10.11`     (vía DbStack::MariaDB.default_image())
//
// Naming (§13.3):
//   - Contenedor web: `drupal-{name}`
//   - Contenedor BD:  `db-{name}-drupal`
//   - Network Docker: `enola_net_drupal_{name}`
//
// Paths (§13.2):
//   - Datos:    `/srv/enola-drupal/{name}/web/`  → `/var/www/html`
//   - BD:       `/srv/enola-drupal/{name}/db/`   → `/var/lib/mysql`
//   - Secrets:  `/srv/enola-drupal/{name}/secrets/{db_root_password,db_password}` (0644)
//
// Setup wizard (§13.1):
//   - Drupal expone wizard web de instalación en la primera ejecución.
//   - Devuelve HTTP 200/302/403 hasta que el usuario completa el formulario inicial.
//   - 403 puede ocurrir por permisos o configuración Apache inicial (TEST-COV-DRUPAL-019).
//   - Los tests E2E DEBEN aceptar esos códigos como PASS.
//
// Docker binding (§13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).

use crate::domain::cms::{
    CmsCreateRequest, CmsDescriptor, CmsInstance, CmsKind, CmsStatus, DbStack,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::cms::{CmsAdapter, CmsLifecycle};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::manifest::ManifestPort;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Directorio base por defecto para datos persistentes de Drupal.
const DEFAULT_DRUPAL_BASE_DIR: &str = "/srv/enola-drupal";

/// Resuelve el directorio base de datos. En tests, se inyecta via `new_with_base`.
fn drupal_base_dir(override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => PathBuf::from(DEFAULT_DRUPAL_BASE_DIR),
    }
}

/// Adapter Drupal. Mantiene una referencia al `ContainerPort` para crear y
/// gestionar los contenedores. No tiene estado mutable propio.
pub struct DrupalCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    base_dir: Option<PathBuf>,
}

impl DrupalCmsAdapter {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            manifest,
            base_dir: None,
        }
    }

    /// Constructor para tests: inyecta el base_dir sin usar env vars (thread-safe).
    #[cfg(test)]
    pub fn new_with_base(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
        base_dir: PathBuf,
    ) -> Self {
        Self {
            container_manager,
            manifest,
            base_dir: Some(base_dir),
        }
    }

    fn web_container_name(name: &str) -> String {
        format!("drupal-{}", name)
    }

    fn db_container_name(name: &str) -> String {
        format!("db-{}-drupal", name)
    }

    fn network_name(name: &str) -> String {
        format!("enola_net_drupal_{}", name)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid Drupal instance name '{}': only alphanumeric, '_' and '-' allowed",
                name
            )));
        }
        Ok(())
    }

    fn generate_password(length: usize) -> String {
        rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect()
    }

    /// Extrae el puerto host HTTP desde la lista de puertos de Docker.
    /// Acepta formatos `127.0.0.1:8080->80/tcp` o `8080->80/tcp`.
    fn parse_http_port(ports: &[String]) -> Option<u16> {
        for entry in ports {
            // Buscar "->80/tcp" y extraer el host port a la izquierda
            if let Some(prefix) = entry.split("->80/tcp").next() {
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(p) = host_part.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
        None
    }
}

impl CmsAdapter for DrupalCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        CmsDescriptor {
            kind: CmsKind::Drupal,
            display_name: "Drupal",
            default_image: "drupal:10-apache",
            db_stack: DbStack::MariaDB,
            // §13.1: Drupal wizard web acepta 200/301/302/304/403 hasta completarse.
            // 403 puede ocurrir por permisos o configuración Apache inicial (TEST-COV-DRUPAL-019).
            // 500 incluido por simetría con WP (errores transitorios durante boot DB).
            setup_wizard_status_codes: &[200, 301, 302, 304, 403, 500],
            container_prefix: "drupal-",
            data_root: "/srv/enola-drupal",
            http_port_range: (8000, 9999),
        }
    }
}

#[async_trait::async_trait]
impl CmsLifecycle for DrupalCmsAdapter {
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance> {
        Self::validate_name(&request.name)?;

        let http_port = request.http_port.ok_or_else(|| {
            EnolaError::ValidationError(
                "DrupalCmsAdapter.create() requires an explicit http_port \
                 (use PortValidator/WordPressPortManager-equivalent at the CLI layer)"
                    .to_string(),
            )
        })?;

        let web_name = Self::web_container_name(&request.name);
        let db_name = Self::db_container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente: docker maneja duplicados).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Secrets (mismo patrón SEC-005 que WordPress).
        let base = drupal_base_dir(self.base_dir.as_deref());
        let inst_dir = base.join(&request.name);
        let secrets_dir = inst_dir.join("secrets");
        let db_root_pass = Self::generate_password(20);
        let db_pass = request
            .db_password
            .clone()
            .unwrap_or_else(|| Self::generate_password(16));
        let db_root_pass_path = write_secret_file(&secrets_dir, "db_root_password", &db_root_pass)?;
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;

        // 3. Volúmenes persistentes.
        let db_volume = inst_dir.join("db");
        let web_volume = inst_dir.join("web");

        // 4. Contenedor de BD (MariaDB).
        let mut db_env = HashMap::new();
        db_env.insert(
            "MYSQL_ROOT_PASSWORD_FILE".to_string(),
            "/run/secrets/db_root_password".to_string(),
        );
        db_env.insert("MYSQL_DATABASE".to_string(), "drupal".to_string());
        db_env.insert("MYSQL_USER".to_string(), "drupal".to_string());
        db_env.insert(
            "MYSQL_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );

        let mut db_volumes = HashMap::new();
        db_volumes.insert(
            db_volume.to_string_lossy().to_string(),
            "/var/lib/mysql".to_string(),
        );
        let mut db_secrets = HashMap::new();
        db_secrets.insert(
            "db_root_password".to_string(),
            db_root_pass_path.to_string_lossy().to_string(),
        );
        db_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );

        let db_image = self
            .descriptor()
            .db_stack
            .default_image()
            .ok_or_else(|| {
                EnolaError::InfrastructureError(
                    "Drupal descriptor missing DB image (DbStack::MariaDB)".to_string(),
                )
            })?
            .to_string();

        let db_config = ContainerConfig {
            name: db_name.clone(),
            image: db_image,
            command: None,
            env: db_env,
            ports: HashMap::new(),
            volumes: db_volumes,
            network: Some(net_name.clone()),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: db_secrets,
            // SEC-019: DB needs write access
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(db_config).await?;
        self.container_manager.start_container(&db_name).await?;
        let _ = self.manifest.append("docker_container", &db_name);

        // 5. Contenedor Drupal.
        // Drupal::10-apache no usa env DB (la configura el wizard web), pero
        // exponemos los hostnames y secret mount para el usuario en el wizard.
        let mut web_volumes = HashMap::new();
        web_volumes.insert(
            web_volume.to_string_lossy().to_string(),
            "/var/www/html".to_string(),
        );

        let mut web_ports = HashMap::new();
        web_ports.insert(http_port, 80u16);

        let mut web_secrets = HashMap::new();
        web_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );

        // Pistas para el wizard de Drupal: usuario, BD, host y password file.
        let mut web_env = HashMap::new();
        web_env.insert("ENOLA_DRUPAL_DB_HOST".to_string(), db_name.clone());
        web_env.insert("ENOLA_DRUPAL_DB_NAME".to_string(), "drupal".to_string());
        web_env.insert("ENOLA_DRUPAL_DB_USER".to_string(), "drupal".to_string());
        web_env.insert(
            "ENOLA_DRUPAL_DB_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );

        let web_config = ContainerConfig {
            name: web_name.clone(),
            image: self.descriptor().default_image.to_string(),
            command: None,
            env: web_env,
            ports: web_ports,
            volumes: web_volumes,
            network: Some(net_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: web_secrets,
            // SEC-019: Drupal needs write access for content
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(web_config).await?;

        // Drupal:10-apache has files in /var/www/html/ but the bind mount
        // hides them with an empty directory. Copy files from the image to
        // the bind mount volume using docker cp from a temporary container.
        if web_volume.exists()
            && std::fs::read_dir(&web_volume)
                .map(|mut d| d.next().is_none())
                .unwrap_or(true)
        {
            let tmp_container = format!("drupal-init-{}", request.name);
            let copy_result = std::process::Command::new("docker")
                .args([
                    "create",
                    "--name",
                    &tmp_container,
                    self.descriptor().default_image,
                ])
                .output();
            if let Ok(out) = copy_result {
                if out.status.success() {
                    let _ = std::process::Command::new("docker")
                        .args([
                            "cp",
                            "-a",
                            &format!("{}:/var/www/html/.", &tmp_container),
                            &web_volume.to_string_lossy(),
                        ])
                        .output();
                }
                let _ = std::process::Command::new("docker")
                    .args(["rm", &tmp_container])
                    .output();
            }
        }

        self.container_manager.start_container(&web_name).await?;
        let _ = self.manifest.append("docker_container", &web_name);

        Ok(CmsInstance {
            kind: CmsKind::Drupal,
            name: request.name,
            status: CmsStatus::Initializing, // wizard aún por completar
            http_port: Some(http_port),
            db_port: None, // BD no expuesta a host
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let db_name = Self::db_container_name(name);
        let web_name = Self::web_container_name(name);
        // Arrancar BD primero para evitar que Drupal arranque sin BD lista.
        self.container_manager.start_container(&db_name).await?;
        self.container_manager.start_container(&web_name).await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);
        // Verificar que el sitio existe antes de intentar parar.
        let containers = self.container_manager.list_containers(true).await?;
        let exists = containers
            .iter()
            .any(|c| c.name == web_name || c.name == db_name);
        if !exists {
            return Err(EnolaError::NotFound(format!(
                "Drupal site '{}' not found. Use `drupal list` to see existing sites.",
                name
            )));
        }
        // Parar Drupal primero para que no quede colgado intentando hablar con BD.
        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        Ok(())
    }

    async fn delete(&self, name: &str, force: bool) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);

        if !force {
            // Verificar que ambos contenedores estén stopped.
            let containers = self.container_manager.list_containers(true).await?;
            let running: Vec<&str> = containers
                .iter()
                .filter(|c| {
                    (c.name == web_name || c.name == db_name)
                        && c.status.to_lowercase().starts_with("up")
                })
                .map(|c| c.name.as_str())
                .collect();
            if !running.is_empty() {
                return Err(EnolaError::ValidationError(format!(
                    "Cannot delete Drupal '{}': containers still running ({}). \
                     Use --force or stop first.",
                    name,
                    running.join(", ")
                )));
            }
        }

        // Stop + remove (idempotente). force=true ⇒ ignoramos errores de stop.
        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        let _ = self.container_manager.remove_container(&web_name).await;
        let _ = self.container_manager.remove_container(&db_name).await;
        let _ = self.manifest.remove("docker_container", &web_name);
        let _ = self.manifest.remove("docker_container", &db_name);
        // Remove the Docker network. Without this, orphaned networks accumulate
        // and exhaust Docker's address pool ("all predefined address pools have
        // been fully subnetted"), blocking new site creation.
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = drupal_base_dir(self.base_dir.as_deref());
        let inst_dir = base.join(name);
        let _ = std::fs::remove_dir_all(&inst_dir);
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<CmsInstance> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);

        let containers = self.container_manager.list_containers(true).await?;
        let web = containers.iter().find(|c| c.name == web_name);
        let db = containers.iter().find(|c| c.name == db_name);

        let status = match (web, db) {
            (None, None) => CmsStatus::NotFound,
            (Some(w), Some(d)) => {
                let web_up = w.status.to_lowercase().starts_with("up");
                let db_up = d.status.to_lowercase().starts_with("up");
                if web_up && db_up {
                    CmsStatus::Running
                } else if !web_up && !db_up {
                    CmsStatus::Stopped
                } else {
                    CmsStatus::Initializing
                }
            }
            _ => CmsStatus::Initializing, // solo uno de los dos existe
        };

        let http_port = web.and_then(|c| Self::parse_http_port(&c.ports));

        Ok(CmsInstance {
            kind: CmsKind::Drupal,
            name: name.to_string(),
            status,
            http_port,
            db_port: None,
            onion_address: super::read_onion_address(&format!("drupal-{}", name)),
        })
    }
}

/// Escribe un secreto en disco con permisos 0600 (mismo patrón SEC-005 que WP).
/// Crea el directorio padre con modo 0700 si no existe.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to create Drupal secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to chmod Drupal secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
    }
    let path = dir.join(name);
    crate::infrastructure::atomic_secret_file::write_secret_atomically(&path, value.as_bytes())
        .map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write Drupal secret {}: {}",
                path.display(),
                e
            ))
        })?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::ports::container::{ContainerInfo, MockContainerPort};
    use crate::ports::manifest::MockManifestPort;
    use std::sync::Mutex;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    #[test]
    fn descriptor_matches_drupal_metadata() {
        let mock = MockContainerPort::new();
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Drupal);
        assert_eq!(d.kind.slug(), "drupal");
        assert_eq!(d.default_image, "drupal:10-apache");
        assert_eq!(d.db_stack, DbStack::MariaDB);
        assert_eq!(d.container_prefix, "drupal-");
        assert_eq!(d.data_root, "/srv/enola-drupal");
        assert!(d.requires_db());
        assert!(d.setup_wizard_status_codes.contains(&302));
        assert!(d.setup_wizard_status_codes.contains(&200));
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(DrupalCmsAdapter::validate_name("").is_err());
        assert!(DrupalCmsAdapter::validate_name("my site").is_err()); // espacio
        assert!(DrupalCmsAdapter::validate_name("my/site").is_err()); // slash
        assert!(DrupalCmsAdapter::validate_name("my$site").is_err()); // shell metachar
        assert!(DrupalCmsAdapter::validate_name("myblog").is_ok());
        assert!(DrupalCmsAdapter::validate_name("my_blog-1").is_ok());
    }

    #[test]
    fn naming_convention_uses_drupal_prefix_and_db_suffix() {
        assert_eq!(DrupalCmsAdapter::web_container_name("blog"), "drupal-blog");
        assert_eq!(
            DrupalCmsAdapter::db_container_name("blog"),
            "db-blog-drupal"
        );
        assert_eq!(
            DrupalCmsAdapter::network_name("blog"),
            "enola_net_drupal_blog"
        );
    }

    #[test]
    fn parse_http_port_handles_127001_format() {
        let ports = vec!["127.0.0.1:8085->80/tcp".to_string()];
        assert_eq!(DrupalCmsAdapter::parse_http_port(&ports), Some(8085));
    }

    #[test]
    fn parse_http_port_handles_short_format() {
        let ports = vec!["8090->80/tcp".to_string()];
        assert_eq!(DrupalCmsAdapter::parse_http_port(&ports), Some(8090));
    }

    #[test]
    fn parse_http_port_returns_none_for_unrelated_ports() {
        let ports = vec!["3306->3306/tcp".to_string()];
        assert_eq!(DrupalCmsAdapter::parse_http_port(&ports), None);
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "blog".to_string(),
            http_port: None,
            db_password: None,
        };
        let r = adapter.create(req).await;
        assert!(r.is_err());
        let err = r.unwrap_err().to_string().to_lowercase();
        assert!(err.contains("http_port"), "got: {}", err);
    }

    #[tokio::test]
    async fn create_provisions_db_then_web_and_returns_initializing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(|c| Ok(c.name));
        mock.expect_start_container().returning(|_| Ok(()));

        let adapter = DrupalCmsAdapter::new_with_base(
            Arc::new(mock),
            Arc::new(mock_manifest()),
            tmp.path().to_path_buf(),
        );
        let req = CmsCreateRequest {
            name: "myblog".to_string(),
            http_port: Some(8085),
            db_password: Some("supersecret".to_string()),
        };
        let inst = adapter.create(req).await.expect("create should succeed");
        assert_eq!(inst.kind, CmsKind::Drupal);
        assert_eq!(inst.name, "myblog");
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
        assert!(inst.onion_address.is_none());
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("ghost").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_when_both_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "drupal-blog".into(),
                    image: "drupal:10-apache".into(),
                    status: "Up 5 minutes".into(),
                    ports: vec!["127.0.0.1:8085->80/tcp".into()],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-blog-drupal".into(),
                    image: "mariadb:10.11".into(),
                    status: "Up 5 minutes".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_both_exited() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "drupal-blog".into(),
                    image: "drupal:10-apache".into(),
                    status: "Exited (0) 2 minutes ago".into(),
                    ports: vec![],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-blog-drupal".into(),
                    image: "mariadb:10.11".into(),
                    status: "Exited (0) 2 minutes ago".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "drupal-blog".into(),
                image: "drupal:10-apache".into(),
                status: "Up 1 minute".into(),
                ports: vec!["127.0.0.1:8085->80/tcp".into()],
            }])
        });
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("blog", false).await;
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string().to_lowercase();
        assert!(msg.contains("running") || msg.contains("--force"));
    }

    #[tokio::test]
    async fn start_starts_db_then_web() {
        let mut mock = MockContainerPort::new();
        // Verificación de orden: el primer start es la BD.
        let order = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let order_clone = order.clone();
        mock.expect_start_container().returning(move |id| {
            order_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = DrupalCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("blog").await.unwrap();
        let calls = order.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], "db-blog-drupal");
        assert_eq!(calls[1], "drupal-blog");
    }
}
