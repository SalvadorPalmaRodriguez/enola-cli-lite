// CMS-MAGNOLIA-001 (2026-05-05) — Adapter Magnolia CMS Community (Java/Tomcat) del catálogo.
//
// Quinto CMS y primer representante Java/JVM del catálogo (13.56).
// A diferencia de Drupal/Wagtail/Strapi, Magnolia usa H2 embebido en el JVM:
// NO crea un contenedor BD separado. Patrón: contenedor único (como Ghost).
//
// Stack:
//   - Web: `ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest` (community)  — Tomcat + JCR + H2.
//   - BD:  H2 embebido en el proceso Tomcat (sin contenedor BD separado).
//          DbStack::None → no external DB container required.
//
// ⚠️ RAM mínima: 1.5 GB. El adaptador emite una advertencia en `create` si
// la RAM libre del SO es <1600 MB. Los usuarios deben tener en cuenta que
// Magnolia (JVM) consume significativamente más que Ghost (256 MB) o Wagtail (512 MB).
//
// Naming (13.3 — prefijos distintos garantizan aislamiento Tor/Nginx):
//   - Contenedor:  `magnolia-{name}`
//   - Network:     `enola_net_magnolia_{name}`
//
// Paths (13.2):
//   - Datos JCR:   `/srv/enola-magnolia/{name}/data/`    → `/magnolia/data`
//   - Secrets:     `/srv/enola-magnolia/{name}/secrets/` → montados como archivos (modo 0700/0600)
//
// Secretos generados (SEC-005):
//   - `admin_password` → contraseña del superuser 'superuser' de Magnolia (16 chars alfanumérico)
//
// Setup wizard (13.1): Magnolia arranca el Tomcat y aplica el bootstrap del JCR
// antes de que el panel admin esté listo. Durante ese proceso (30s-2min) devuelve
// 200/302/500 dependiendo del estado de la inicialización.
//
// Puerto interno: Tomcat escucha en **8080** por defecto.
// El binding host:contenedor mapea http_port (host) → 8080 (container).
//
// Docker binding (13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).
//
// RAM mínima: 1.5 GB ⚠️ — advertencia automática si <1600 MB disponibles.

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

/// Puerto interno donde Magnolia/Tomcat escucha dentro del contenedor.
const MAGNOLIA_INTERNAL_PORT: u16 = 8080;

/// RAM mínima recomendada en MB para Magnolia (JVM overhead + Tomcat + Magnolia).
const MAGNOLIA_MIN_RAM_MB: u64 = 1536;

/// Directorio base por defecto para datos persistentes de Magnolia.
const DEFAULT_MAGNOLIA_BASE_DIR: &str = "/srv/enola-magnolia";

/// Resuelve el directorio base. Tests pueden sobreescribir con
/// `ENOLA_MAGNOLIA_BASE_DIR` (mismo patrón que Drupal/Ghost/Wagtail/Strapi).
fn magnolia_base_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(dir) = std::env::var("ENOLA_MAGNOLIA_BASE_DIR") {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(DEFAULT_MAGNOLIA_BASE_DIR)
}

/// Adapter Magnolia Community. Solo necesita `ContainerPort` — sin BD externa.
pub struct MagnoliaCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl MagnoliaCmsAdapter {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            manifest,
        }
    }

    fn container_name(name: &str) -> String {
        format!("magnolia-{}", name)
    }

    fn network_name(name: &str) -> String {
        format!("enola_net_magnolia_{}", name)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid Magnolia instance name '{}': only alphanumeric, '_' and '-' allowed",
                name
            )));
        }
        Ok(())
    }

    /// Genera una contraseña alfanumérica aleatoria de `length` caracteres.
    fn generate_password(length: usize) -> String {
        rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect()
    }

    /// Extrae el puerto host HTTP desde la lista de puertos de Docker.
    /// Acepta `127.0.0.1:8085->8080/tcp` o `8085->8080/tcp`.
    /// NO matchea con 80 (WP/Drupal), 2368 (Ghost), 8000 (Wagtail) ni 1337 (Strapi).
    fn parse_http_port(ports: &[String]) -> Option<u16> {
        let needle = format!("->{}/tcp", MAGNOLIA_INTERNAL_PORT);
        for entry in ports {
            if let Some(prefix) = entry.split(needle.as_str()).next() {
                if prefix == entry.as_str() {
                    continue;
                }
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(p) = host_part.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Obtiene la RAM disponible en MB leyendo `/proc/meminfo` (Linux only).
    /// Retorna `None` si no se puede determinar (tests, macOS, etc.).
    fn available_ram_mb() -> Option<u64> {
        let content = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in content.lines() {
            if line.starts_with("MemAvailable:") {
                let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
                return Some(kb / 1024);
            }
        }
        None
    }

    /// Emite una advertencia si la RAM disponible es insuficiente para Magnolia.
    /// No bloquea la creación — el usuario puede tener más RAM de la que reporta
    /// `/proc/meminfo` en WSL2.
    fn warn_if_low_ram() {
        if let Some(available_mb) = Self::available_ram_mb() {
            if available_mb < MAGNOLIA_MIN_RAM_MB {
                eprintln!(
                    "⚠️  Magnolia (JVM) requiere ≥{} MB RAM disponible. Detectados: {} MB.",
                    MAGNOLIA_MIN_RAM_MB, available_mb
                );
                eprintln!(
                    "    Considera Ghost (Node, 256 MB) o Wagtail (Python, 512 MB) si tu\n\
                     máquina está ajustada de memoria."
                );
            }
        }
    }
}

impl CmsAdapter for MagnoliaCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        magnolia_descriptor()
    }
}

/// Descriptor estático compartido entre el adapter en runtime y el catálogo
/// (`adapters::cms::catalog_descriptors`). Mantener sincronizado.
pub(crate) fn magnolia_descriptor() -> CmsDescriptor {
    CmsDescriptor {
        kind: CmsKind::Magnolia,
        display_name: "Magnolia",
        default_image: "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest",
        // H2 embebido en el JVM de Tomcat. Sin contenedor BD separado.
        db_stack: DbStack::None,
        // 13.1: Magnolia arranca Tomcat + JCR bootstrap. Durante ese proceso
        // (~30s-2min primera vez) puede devolver 500/302 antes de estar listo.
        setup_wizard_status_codes: &[200, 301, 302, 304, 500],
        container_prefix: "magnolia-",
        data_root: "/srv/enola-magnolia",
        http_port_range: (8000, 9999),
    }
}

#[async_trait::async_trait]
impl CmsLifecycle for MagnoliaCmsAdapter {
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance> {
        Self::validate_name(&request.name)?;

        let http_port = request.http_port.ok_or_else(|| {
            EnolaError::ValidationError(
                "MagnoliaCmsAdapter.create() requires an explicit http_port \
                 (use PortValidator at the CLI layer)"
                    .to_string(),
            )
        })?;

        // Advertencia de RAM antes de crear (no bloquea).
        Self::warn_if_low_ram();

        let container_name = Self::container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente; aunque contenedor único, aplicamos por simetría).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Secreto de admin (mismo patrón SEC-005).
        let base = magnolia_base_dir();
        let inst_dir = base.join(&request.name);
        let secrets_dir = inst_dir.join("secrets");
        let admin_pass = request
            .db_password // reutilizamos el campo para la contraseña de admin
            .clone()
            .unwrap_or_else(|| Self::generate_password(16));
        let admin_pass_path = write_secret_file(&secrets_dir, "admin_password", &admin_pass)?;

        // 3. Volumen de datos JCR (Java Content Repository).
        let data_volume = inst_dir.join("data");

        let mut volumes = HashMap::new();
        volumes.insert(
            data_volume.to_string_lossy().to_string(),
            "/magnolia/data".to_string(),
        );

        let mut ports = HashMap::new();
        ports.insert(http_port, MAGNOLIA_INTERNAL_PORT);

        let mut secrets = HashMap::new();
        secrets.insert(
            "admin_password".to_string(),
            admin_pass_path.to_string_lossy().to_string(),
        );

        // Variables de entorno básicas de Magnolia.
        // MAGNOLIA_SUPERUSER_PASSWORD_FILE: para que el script de init lea la
        // contraseña del archivo de secretos (no la línea de comandos).
        let mut env = HashMap::new();
        env.insert(
            "ENOLA_MAGNOLIA_ADMIN_PASSWORD_FILE".to_string(),
            "/run/secrets/admin_password".to_string(),
        );
        // Java JVM heap: 512 MB mínimo, 1 GB máximo. Magnolia no arranca
        // correctamente con menos de 512 MB de heap.
        env.insert("JAVA_OPTS".to_string(), "-Xms512m -Xmx1024m".to_string());

        let config = ContainerConfig {
            name: container_name.clone(),
            image: self.descriptor().default_image.to_string(),
            command: None,
            env,
            ports,
            volumes,
            network: Some(net_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets,
            // SEC-019: Magnolia needs write access for JCR repository
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(config).await?;
        self.container_manager
            .start_container(&container_name)
            .await?;
        let _ = self.manifest.append("docker_container", &container_name);

        Ok(CmsInstance {
            kind: CmsKind::Magnolia,
            name: request.name,
            // JCR bootstrap + Tomcat startup tarda hasta 2 min en primera ejecución.
            status: CmsStatus::Initializing,
            http_port: Some(http_port),
            db_port: None,
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        self.container_manager
            .start_container(&Self::container_name(name))
            .await
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let cname = Self::container_name(name);
        let containers = self.container_manager.list_containers(true).await?;
        if !containers.iter().any(|c| c.name == cname) {
            return Err(EnolaError::NotFound(format!(
                "Magnolia instance '{}' not found. Use `magnolia list` to see existing instances.",
                name
            )));
        }
        let _ = self.container_manager.stop_container(&cname).await;
        Ok(())
    }

    async fn delete(&self, name: &str, force: bool) -> Result<()> {
        Self::validate_name(name)?;
        let cname = Self::container_name(name);

        if !force {
            let containers = self.container_manager.list_containers(true).await?;
            let running = containers
                .iter()
                .any(|c| c.name == cname && c.status.to_lowercase().starts_with("up"));
            if running {
                return Err(EnolaError::ValidationError(format!(
                    "Cannot delete Magnolia '{}': container still running. \
                     Use --force or stop first.",
                    name
                )));
            }
        }

        let _ = self.container_manager.stop_container(&cname).await;
        let _ = self.container_manager.remove_container(&cname).await;
        let _ = self.manifest.remove("docker_container", &cname);
        // Remove the Docker network.
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = magnolia_base_dir();
        let inst_dir = base.join(name);
        let _ = std::fs::remove_dir_all(&inst_dir);
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<CmsInstance> {
        Self::validate_name(name)?;
        let cname = Self::container_name(name);

        let containers = self.container_manager.list_containers(true).await?;
        let c = containers.iter().find(|c| c.name == cname);

        let status = match c {
            None => CmsStatus::NotFound,
            Some(c) if c.status.to_lowercase().starts_with("up") => CmsStatus::Running,
            _ => CmsStatus::Stopped,
        };

        let http_port = c.and_then(|c| Self::parse_http_port(&c.ports));

        Ok(CmsInstance {
            kind: CmsKind::Magnolia,
            name: name.to_string(),
            status,
            http_port,
            db_port: None,
            onion_address: super::read_onion_address(&format!("magnolia-{}", name)),
        })
    }
}

/// Escribe un secreto en disco con permisos 0600 (mismo patrón SEC-005).
/// Crea el directorio padre con modo 0700 si no existe.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to create Magnolia secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to chmod Magnolia secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
    }
    let path = dir.join(name);
    crate::infrastructure::atomic_secret_file::write_secret_atomically(&path, value.as_bytes())
        .map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write Magnolia secret {}: {}",
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
    use tempfile::TempDir;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    /// Mutex global para serializar mutaciones de ENOLA_MAGNOLIA_BASE_DIR (13.33).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_test_base() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ENOLA_MAGNOLIA_BASE_DIR", tmp.path());
        (tmp, guard)
    }

    fn teardown_test_base(_tmp: TempDir, _guard: std::sync::MutexGuard<'static, ()>) {
        std::env::remove_var("ENOLA_MAGNOLIA_BASE_DIR");
    }

    #[test]
    fn descriptor_matches_magnolia_metadata() {
        let mock = MockContainerPort::new();
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Magnolia);
        assert_eq!(d.kind.slug(), "magnolia");
        assert_eq!(
            d.default_image,
            "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest"
        );
        assert_eq!(d.db_stack, DbStack::None);
        assert_eq!(d.container_prefix, "magnolia-");
        assert_eq!(d.data_root, "/srv/enola-magnolia");
        assert!(
            !d.requires_db(),
            "Magnolia+H2 embedded does NOT require external DB"
        );
        assert!(d.setup_wizard_status_codes.contains(&200));
        assert!(d.setup_wizard_status_codes.contains(&302));
        assert!(d.setup_wizard_status_codes.contains(&500));
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(MagnoliaCmsAdapter::validate_name("").is_err());
        assert!(MagnoliaCmsAdapter::validate_name("my site").is_err());
        assert!(MagnoliaCmsAdapter::validate_name("my/cms").is_err());
        assert!(MagnoliaCmsAdapter::validate_name("myCMS").is_ok());
        assert!(MagnoliaCmsAdapter::validate_name("my_cms-1").is_ok());
    }

    #[test]
    fn naming_uses_magnolia_prefix_and_does_not_collide_with_other_cms() {
        // 13.3 + 13.41 — prefijos distintos garantizan aislamiento Tor/Nginx.
        assert_eq!(MagnoliaCmsAdapter::container_name("blog"), "magnolia-blog");
        assert_eq!(
            MagnoliaCmsAdapter::network_name("blog"),
            "enola_net_magnolia_blog"
        );
        let name = "cms";
        // Anti-regresión: magnolia-{name} != wp/drupal/ghost/wagtail/strapi-{name}.
        assert_ne!(
            MagnoliaCmsAdapter::container_name(name),
            format!("wp-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::container_name(name),
            format!("drupal-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::container_name(name),
            format!("ghost-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::container_name(name),
            format!("wagtail-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::container_name(name),
            format!("strapi-{}", name)
        );
    }

    #[test]
    fn parse_http_port_handles_127001_format() {
        let ports = vec!["127.0.0.1:8085->8080/tcp".to_string()];
        assert_eq!(MagnoliaCmsAdapter::parse_http_port(&ports), Some(8085));
    }

    #[test]
    fn parse_http_port_handles_short_format() {
        let ports = vec!["8090->8080/tcp".to_string()];
        assert_eq!(MagnoliaCmsAdapter::parse_http_port(&ports), Some(8090));
    }

    #[test]
    fn parse_http_port_returns_none_for_unrelated_ports() {
        // Puerto 80 (WP/Drupal) NO debe matchear con Magnolia (8080).
        let ports = vec!["8085->80/tcp".to_string()];
        assert_eq!(MagnoliaCmsAdapter::parse_http_port(&ports), None);
        // Puerto 2368 (Ghost) tampoco.
        let ports = vec!["8085->2368/tcp".to_string()];
        assert_eq!(MagnoliaCmsAdapter::parse_http_port(&ports), None);
        // Puerto 1337 (Strapi) tampoco.
        let ports = vec!["8085->1337/tcp".to_string()];
        assert_eq!(MagnoliaCmsAdapter::parse_http_port(&ports), None);
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "mycms".to_string(),
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
        let (tmp, guard) = setup_test_base();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        // Contenedor único (sin BD externa) — 1 create + 1 start.
        mock.expect_create_container()
            .times(1)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(1).returning(|_| Ok(()));

        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "mycms".to_string(),
            http_port: Some(8085),
            db_password: Some("adminpass".to_string()),
        };
        let inst = adapter.create(req).await.expect("create should succeed");
        assert_eq!(inst.kind, CmsKind::Magnolia);
        assert_eq!(inst.name, "mycms");
        // JCR bootstrap + Tomcat startup → Initializing.
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
        assert!(inst.onion_address.is_none());
        teardown_test_base(tmp, guard);
    }

    #[tokio::test]
    async fn create_generates_admin_password_secret_file() {
        let (tmp, guard) = setup_test_base();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container()
            .times(1)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(1).returning(|_| Ok(()));

        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "sectest".to_string(),
            http_port: Some(8086),
            db_password: None,
        };
        adapter.create(req).await.expect("create should succeed");

        // Solo un secreto: admin_password.
        let secrets_dir = tmp.path().join("sectest").join("secrets");
        let path = secrets_dir.join("admin_password");
        assert!(path.exists(), "admin_password secret file must exist");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "admin_password must not be empty");
        assert_eq!(content.len(), 16, "generated password should be 16 chars");

        teardown_test_base(tmp, guard);
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_container() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_when_container_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "magnolia-mycms".into(),
                image: "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest".into(),
                status: "Up 2 minutes".into(),
                ports: vec!["127.0.0.1:8085->8080/tcp".into()],
            }])
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_container_exited() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "magnolia-mycms".into(),
                image: "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest".into(),
                status: "Exited (0) 5 minutes ago".into(),
                ports: vec![],
            }])
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "magnolia-mycms".into(),
                image: "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest".into(),
                status: "Up 2 minutes".into(),
                ports: vec![],
            }])
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("mycms", false).await;
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string().to_lowercase();
        assert!(msg.contains("running") || msg.contains("--force"));
    }

    #[tokio::test]
    async fn delete_with_force_removes_container() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().times(0);
        mock.expect_stop_container().times(1).returning(|_| Ok(()));
        mock.expect_remove_container()
            .times(1)
            .returning(|_| Ok(()));
        mock.expect_remove_network().returning(|_| Ok(()));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("mycms", true).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn start_starts_single_container() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_start_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("mycms").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // Contenedor único, solo 1 start.
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "magnolia-mycms");
    }

    #[tokio::test]
    async fn stop_stops_container() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![crate::ports::container::ContainerInfo {
                id: "abc".into(),
                name: "magnolia-mycms".into(),
                image: "magnolia".into(),
                status: "Up".into(),
                ports: vec![],
            }])
        });
        mock.expect_stop_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.stop("mycms").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "magnolia-mycms");
    }
}
