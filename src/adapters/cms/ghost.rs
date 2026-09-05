// CMS-GHOST-001 (2026-04-30) — Adapter Ghost del catálogo CMS.
//
// Segundo CMS validando la abstracción `CmsLifecycle` (DRUPAL-001 §13.56).
// Ghost demuestra que `DbStack::Sqlite` funciona end-to-end: NO crea contenedor
// BD adicional, NO genera secretos en /run/secrets, todo el estado vive dentro
// del propio contenedor Ghost en `/var/lib/ghost/content/`.
//
// Stack:
//   - Web: `ghost:5-alpine`  (oficial)
//   - BD:  SQLite embebido en `/var/lib/ghost/content/data/ghost.db`
//          (no hay contenedor MariaDB/Postgres separado)
//
// Naming (§13.3):
//   - Contenedor:  `ghost-{name}`
//   - Network:     `enola_net_ghost_{name}` (creada por simetría con otros CMS,
//                  aunque Ghost-SQLite no la necesite estrictamente)
//
// Paths (§13.2):
//   - Datos:       `/srv/enola-ghost/{name}/content/`  → `/var/lib/ghost/content`
//                  (incluye SQLite db, themes, images, logs)
//
// Setup wizard (§13.1):
//   - Ghost expone su admin wizard en `/ghost/` la primera vez que se accede.
//   - HTTP en `/` devuelve 200/302 según versión y estado.
//
// Puerto interno: Ghost escucha en **2368** (NO 80). El binding host:contenedor
// mapea http_port (host) → 2368 (container).
//
// Docker binding (§13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).
//
// RAM mínima: ~256 MB (estable, dominio Node.js ligero).

use crate::domain::cms::{
    CmsCreateRequest, CmsDescriptor, CmsInstance, CmsKind, CmsStatus, DbStack,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::cms::{CmsAdapter, CmsLifecycle};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::manifest::ManifestPort;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Puerto interno donde Ghost escucha dentro del contenedor.
const GHOST_INTERNAL_PORT: u16 = 2368;

/// Directorio base por defecto para datos persistentes de Ghost.
const DEFAULT_GHOST_BASE_DIR: &str = "/srv/enola-ghost";

/// Resuelve el directorio base de datos. En tests, se inyecta via `new_with_base`.
fn ghost_base_dir(override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => PathBuf::from(DEFAULT_GHOST_BASE_DIR),
    }
}

/// Adapter Ghost. Solo necesita `ContainerPort` para gestión Docker — ninguna
/// dependencia adicional (no DB, no nginx, no secrets).
pub struct GhostCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    base_dir: Option<PathBuf>,
}

impl GhostCmsAdapter {
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
        format!("ghost-{}", name)
    }

    fn network_name(name: &str) -> String {
        format!("enola_net_ghost_{}", name)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid Ghost instance name '{}': only alphanumeric, '_' and '-' allowed",
                name
            )));
        }
        Ok(())
    }

    /// Extrae el puerto host HTTP desde la lista de puertos de Docker.
    /// Acepta formatos `127.0.0.1:8080->2368/tcp` o `8080->2368/tcp`.
    fn parse_http_port(ports: &[String]) -> Option<u16> {
        let needle = format!("->{}/tcp", GHOST_INTERNAL_PORT);
        for entry in ports {
            if let Some(prefix) = entry.split(needle.as_str()).next() {
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(p) = host_part.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
        None
    }
}

impl CmsAdapter for GhostCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        ghost_descriptor()
    }
}

/// Descriptor estático compartido entre el adapter en runtime y el catálogo
/// (`adapters::cms::catalog_descriptors`). Mantener sincronizado.
pub(crate) fn ghost_descriptor() -> CmsDescriptor {
    CmsDescriptor {
        kind: CmsKind::Ghost,
        display_name: "Ghost",
        default_image: "ghost:5-alpine",
        db_stack: DbStack::Sqlite,
        // §13.1: Ghost durante boot devuelve 502/503 hasta que el server Node
        // arranca; tras boot, `/` redirige a `/ghost/` (302) o devuelve 200.
        setup_wizard_status_codes: &[200, 301, 302, 304, 500, 502, 503],
        container_prefix: "ghost-",
        data_root: "/srv/enola-ghost",
        http_port_range: (8000, 9999),
    }
}

#[async_trait::async_trait]
impl CmsLifecycle for GhostCmsAdapter {
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance> {
        Self::validate_name(&request.name)?;

        let http_port = request.http_port.ok_or_else(|| {
            EnolaError::ValidationError(
                "GhostCmsAdapter.create() requires an explicit http_port \
                 (use PortValidator at the CLI layer)"
                    .to_string(),
            )
        })?;

        let web_name = Self::web_container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Volumen persistente (`content/` lleva SQLite db, themes, imágenes).
        let base = ghost_base_dir(self.base_dir.as_deref());
        let inst_dir = base.join(&request.name);
        let content_volume = inst_dir.join("content");

        // Crear el directorio en disco con modo 0700 si tests/local lo necesitan.
        // En producción Docker lo crea con root; aquí solo aseguramos que existe
        // si el operador lo pre-crea para overrides.
        if !content_volume.exists() {
            // No fatal si no podemos crearlo (Docker lo creará con sus permisos).
            let _ = std::fs::create_dir_all(&content_volume);
        }

        // 3. Env: forzar SQLite explícito y URL pública. Si el usuario no ha
        // configurado URL pública, Ghost usa http://localhost:{port}.
        let mut web_env = HashMap::new();
        web_env.insert("url".to_string(), format!("http://127.0.0.1:{}", http_port));
        web_env.insert("database__client".to_string(), "sqlite3".to_string());
        web_env.insert(
            "database__connection__filename".to_string(),
            "/var/lib/ghost/content/data/ghost.db".to_string(),
        );
        web_env.insert("database__useNullAsDefault".to_string(), "true".to_string());
        web_env.insert("NODE_ENV".to_string(), "production".to_string());

        let mut web_volumes = HashMap::new();
        web_volumes.insert(
            content_volume.to_string_lossy().to_string(),
            "/var/lib/ghost/content".to_string(),
        );

        let mut web_ports = HashMap::new();
        web_ports.insert(http_port, GHOST_INTERNAL_PORT);

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
            secrets: HashMap::new(),
            // SEC-019: Ghost needs write access for content
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(web_config).await?;
        self.container_manager.start_container(&web_name).await?;
        let _ = self.manifest.append("docker_container", &web_name);

        Ok(CmsInstance {
            kind: CmsKind::Ghost,
            name: request.name,
            status: CmsStatus::Initializing,
            http_port: Some(http_port),
            db_port: None,
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        self.container_manager.start_container(&web_name).await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        // Verificar que el sitio existe antes de intentar parar.
        let containers = self.container_manager.list_containers(true).await?;
        let exists = containers.iter().any(|c| c.name == web_name);
        if !exists {
            return Err(EnolaError::NotFound(format!(
                "Ghost site '{}' not found. Use `ghost list` to see existing sites.",
                name
            )));
        }
        let _ = self.container_manager.stop_container(&web_name).await;
        Ok(())
    }

    async fn delete(&self, name: &str, force: bool) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);

        if !force {
            let containers = self.container_manager.list_containers(true).await?;
            let running: Vec<&str> = containers
                .iter()
                .filter(|c| c.name == web_name && c.status.to_lowercase().starts_with("up"))
                .map(|c| c.name.as_str())
                .collect();
            if !running.is_empty() {
                return Err(EnolaError::ValidationError(format!(
                    "Cannot delete Ghost '{}': container still running ({}). \
                     Use --force or stop first.",
                    name,
                    running.join(", ")
                )));
            }
        }

        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.remove_container(&web_name).await;
        let _ = self.manifest.remove("docker_container", &web_name);
        // Remove the Docker network.
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = ghost_base_dir(self.base_dir.as_deref());
        let inst_dir = base.join(name);
        let _ = std::fs::remove_dir_all(&inst_dir);
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<CmsInstance> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);

        let containers = self.container_manager.list_containers(true).await?;
        let web = containers.iter().find(|c| c.name == web_name);

        let status = match web {
            None => CmsStatus::NotFound,
            Some(w) => {
                if w.status.to_lowercase().starts_with("up") {
                    CmsStatus::Running
                } else {
                    CmsStatus::Stopped
                }
            }
        };

        let http_port = web.and_then(|c| Self::parse_http_port(&c.ports));

        Ok(CmsInstance {
            kind: CmsKind::Ghost,
            name: name.to_string(),
            status,
            http_port,
            db_port: None,
            onion_address: super::read_onion_address(&format!("ghost-{}", name)),
        })
    }
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
    fn descriptor_matches_ghost_metadata() {
        let mock = MockContainerPort::new();
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Ghost);
        assert_eq!(d.kind.slug(), "ghost");
        assert_eq!(d.default_image, "ghost:5-alpine");
        assert_eq!(d.db_stack, DbStack::Sqlite);
        assert_eq!(d.container_prefix, "ghost-");
        assert_eq!(d.data_root, "/srv/enola-ghost");
        // SQLite NO requiere contenedor BD externo (DRUPAL-001 §2.2).
        assert!(!d.requires_db());
        assert!(d.setup_wizard_status_codes.contains(&200));
        assert!(d.setup_wizard_status_codes.contains(&302));
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(GhostCmsAdapter::validate_name("").is_err());
        assert!(GhostCmsAdapter::validate_name("my blog").is_err()); // espacio
        assert!(GhostCmsAdapter::validate_name("my/blog").is_err()); // slash
        assert!(GhostCmsAdapter::validate_name("my$blog").is_err()); // shell metachar
        assert!(GhostCmsAdapter::validate_name("myblog").is_ok());
        assert!(GhostCmsAdapter::validate_name("my_blog-1").is_ok());
    }

    #[test]
    fn naming_uses_ghost_prefix_and_does_not_collide_with_wp_or_drupal() {
        // §13.3 + §13.41 — prefijos distintos garantizan aislamiento Tor/Nginx.
        assert_eq!(GhostCmsAdapter::web_container_name("blog"), "ghost-blog");
        assert_eq!(
            GhostCmsAdapter::network_name("blog"),
            "enola_net_ghost_blog"
        );
        // Anti-regresión: ghost-{name} != wp-{name} != drupal-{name}.
        assert_ne!(
            GhostCmsAdapter::web_container_name("blog"),
            format!("wp-{}", "blog")
        );
        assert_ne!(
            GhostCmsAdapter::web_container_name("blog"),
            format!("drupal-{}", "blog")
        );
    }

    #[test]
    fn parse_http_port_handles_127001_format_ghost_port() {
        let ports = vec!["127.0.0.1:8085->2368/tcp".to_string()];
        assert_eq!(GhostCmsAdapter::parse_http_port(&ports), Some(8085));
    }

    #[test]
    fn parse_http_port_handles_short_format_ghost_port() {
        let ports = vec!["8090->2368/tcp".to_string()];
        assert_eq!(GhostCmsAdapter::parse_http_port(&ports), Some(8090));
    }

    #[test]
    fn parse_http_port_returns_none_for_unrelated_ports() {
        // Puerto 80 (WP/Drupal) NO debe matchear con Ghost (2368).
        let ports = vec!["8080->80/tcp".to_string()];
        assert_eq!(GhostCmsAdapter::parse_http_port(&ports), None);
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
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
    async fn create_provisions_single_container_no_db_and_returns_initializing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        // SQLite ⇒ UN solo create_container (vs Drupal que llama 2 veces).
        mock.expect_create_container()
            .times(1)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(1).returning(|_| Ok(()));

        let adapter = GhostCmsAdapter::new_with_base(
            Arc::new(mock),
            Arc::new(mock_manifest()),
            tmp.path().to_path_buf(),
        );
        let req = CmsCreateRequest {
            name: "myblog".to_string(),
            http_port: Some(8085),
            db_password: None, // ignorado: SQLite no usa password
        };
        let inst = adapter.create(req).await.expect("create should succeed");
        assert_eq!(inst.kind, CmsKind::Ghost);
        assert_eq!(inst.name, "myblog");
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
        assert!(inst.onion_address.is_none());
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_container() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_when_container_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "ghost-blog".into(),
                image: "ghost:5-alpine".into(),
                status: "Up 2 minutes".into(),
                ports: vec!["127.0.0.1:8085->2368/tcp".into()],
            }])
        });
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_container_exited() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "ghost-blog".into(),
                image: "ghost:5-alpine".into(),
                status: "Exited (0) 1 minute ago".into(),
                ports: vec![],
            }])
        });
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "ghost-blog".into(),
                image: "ghost:5-alpine".into(),
                status: "Up 1 minute".into(),
                ports: vec!["127.0.0.1:8085->2368/tcp".into()],
            }])
        });
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("blog", false).await;
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string().to_lowercase();
        assert!(msg.contains("running") || msg.contains("--force"));
    }

    #[tokio::test]
    async fn delete_with_force_removes_even_running() {
        let mut mock = MockContainerPort::new();
        // force=true ⇒ NO se llama list_containers (skip running check).
        mock.expect_list_containers().times(0);
        mock.expect_stop_container().returning(|_| Ok(()));
        mock.expect_remove_container().returning(|_| Ok(()));
        mock.expect_remove_network().returning(|_| Ok(()));
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("blog", true).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn start_starts_only_web_container_no_db() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_start_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = GhostCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("blog").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // SQLite ⇒ UN solo start (vs Drupal que llama 2: db + web).
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "ghost-blog");
    }
}
