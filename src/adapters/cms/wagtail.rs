// CMS-WAGTAIL-001 (2026-05-05) — Adapter Wagtail (Python/Django) del catálogo CMS.
//
// Tercer CMS validando la abstracción `CmsLifecycle` (DRUPAL-001 §13.56),
// y primer representante Python del catálogo. Combina:
//   - Patrón 2-contenedores (web + DB) — como Drupal.
//   - Puerto interno custom (8000, no 80) — como Ghost (2368).
//   - DB Postgres 16 (no MariaDB) — primer Postgres del catálogo.
//
// Stack:
//   - Web: `wagtail/bakerydemo:latest` (oficial)  — Django + Wagtail framework.
//   - BD:  `postgres:16-alpine` en contenedor separado.
//
// Naming (§13.3 — prefijos distintos garantizan aislamiento Tor/Nginx):
//   - Web:        `wagtail-{name}`
//   - DB:         `db-{name}-wagtail`
//   - Network:    `enola_net_wagtail_{name}`
//
// Paths (§13.2):
//   - App:        `/srv/enola-wagtail/{name}/app/`     → `/app`
//   - DB:         `/srv/enola-wagtail/{name}/db/`      → `/var/lib/postgresql/data`
//   - Secrets:    `/srv/enola-wagtail/{name}/secrets/` → `/run/secrets/` (modo 0700/0600)
//
// Setup wizard (§13.1): Wagtail genera `superuser` por env. Tras boot el
// admin queda disponible en `/admin/`. Códigos aceptables durante boot:
// 200/301/302/304/500 (errores transitorios mientras Django carga).
//
// Puerto interno: Wagtail/Django escuchan en 8000 dentro del contenedor.
// El binding host:contenedor mapea http_port (host) → 8000 (container).
//
// Docker binding (§13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).
//
// RAM mínima: 512 MB (Python + Django runtime).

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

/// Puerto interno donde Wagtail/Django escucha dentro del contenedor.
const WAGTAIL_INTERNAL_PORT: u16 = 8000;

/// Directorio base por defecto para datos persistentes de Wagtail.
const DEFAULT_WAGTAIL_BASE_DIR: &str = "/srv/enola-wagtail";

/// Resuelve el directorio base. Tests pueden sobreescribir con
/// `ENOLA_WAGTAIL_BASE_DIR` (mismo patrón que Drupal/Ghost).
fn wagtail_base_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(dir) = std::env::var("ENOLA_WAGTAIL_BASE_DIR") {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(DEFAULT_WAGTAIL_BASE_DIR)
}

/// Adapter Wagtail. Igual que Drupal, solo necesita `ContainerPort`.
pub struct WagtailCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl WagtailCmsAdapter {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            manifest,
        }
    }

    fn web_container_name(name: &str) -> String {
        format!("wagtail-{}", name)
    }

    fn db_container_name(name: &str) -> String {
        format!("db-{}-wagtail", name)
    }

    fn network_name(name: &str) -> String {
        format!("enola_net_wagtail_{}", name)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid Wagtail instance name '{}': only alphanumeric, '_' and '-' allowed",
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

    /// Extrae el puerto host HTTP. Acepta `127.0.0.1:8080->8000/tcp` o
    /// `8080->8000/tcp`. NO matchea con puertos 80 (WP/Drupal) ni 2368 (Ghost).
    fn parse_http_port(ports: &[String]) -> Option<u16> {
        let needle = format!("->{}/tcp", WAGTAIL_INTERNAL_PORT);
        for entry in ports {
            if let Some(prefix) = entry.split(needle.as_str()).next() {
                if prefix == entry.as_str() {
                    continue; // no contenía el needle
                }
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(p) = host_part.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
        None
    }
}

impl CmsAdapter for WagtailCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        wagtail_descriptor()
    }
}

/// Descriptor estático compartido entre el adapter en runtime y el catálogo
/// (`adapters::cms::catalog_descriptors`). Mantener sincronizado.
pub(crate) fn wagtail_descriptor() -> CmsDescriptor {
    CmsDescriptor {
        kind: CmsKind::Wagtail,
        display_name: "Wagtail",
        default_image: "wagtail/bakerydemo:latest",
        db_stack: DbStack::Postgres,
        // §13.1: Django/Wagtail durante boot puede devolver 500 mientras se
        // aplican migraciones; tras boot, `/` redirige a `/admin/` (302) o
        // sirve la home (200).
        setup_wizard_status_codes: &[200, 301, 302, 304, 500],
        container_prefix: "wagtail-",
        data_root: "/srv/enola-wagtail",
        http_port_range: (8000, 9999),
    }
}

#[async_trait::async_trait]
impl CmsLifecycle for WagtailCmsAdapter {
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance> {
        Self::validate_name(&request.name)?;

        let http_port = request.http_port.ok_or_else(|| {
            EnolaError::ValidationError(
                "WagtailCmsAdapter.create() requires an explicit http_port \
                 (use PortValidator at the CLI layer)"
                    .to_string(),
            )
        })?;

        let web_name = Self::web_container_name(&request.name);
        let db_name = Self::db_container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Secrets (mismo patrón SEC-005 que WP/Drupal).
        let base = wagtail_base_dir();
        let inst_dir = base.join(&request.name);
        let secrets_dir = inst_dir.join("secrets");
        let db_pass = request
            .db_password
            .clone()
            .unwrap_or_else(|| Self::generate_password(20));
        let django_secret = Self::generate_password(50);
        let admin_pass = Self::generate_password(16);
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;
        let django_secret_path =
            write_secret_file(&secrets_dir, "django_secret_key", &django_secret)?;
        let admin_pass_path = write_secret_file(&secrets_dir, "admin_password", &admin_pass)?;

        // 3. Volúmenes persistentes.
        let db_volume = inst_dir.join("db");
        let app_volume = inst_dir.join("app");

        // 4. Contenedor BD (Postgres 16) — primer Postgres del catálogo CMS.
        // POSTGRES_PASSWORD_FILE permite leer la contraseña desde /run/secrets/
        // sin ponerla en la línea de comandos (visible en `ps`).
        let mut db_env = HashMap::new();
        db_env.insert("POSTGRES_DB".to_string(), "wagtail".to_string());
        db_env.insert("POSTGRES_USER".to_string(), "wagtail".to_string());
        db_env.insert(
            "POSTGRES_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );

        let mut db_volumes = HashMap::new();
        db_volumes.insert(
            db_volume.to_string_lossy().to_string(),
            "/var/lib/postgresql/data".to_string(),
        );
        let mut db_secrets = HashMap::new();
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
                    "Wagtail descriptor missing DB image (DbStack::Postgres)".to_string(),
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

        // 5. Contenedor Wagtail (Django).
        // wagtail/bakerydemo:latest espera variables de entorno DATABASE_URL y
        // SECRET_KEY. Se inyectan a través de archivos en /run/secrets/ por
        // simetría con WP/Drupal (no por línea de comandos).
        let mut web_volumes = HashMap::new();
        web_volumes.insert(app_volume.to_string_lossy().to_string(), "/app".to_string());

        let mut web_ports = HashMap::new();
        web_ports.insert(http_port, WAGTAIL_INTERNAL_PORT);

        let mut web_secrets = HashMap::new();
        web_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "django_secret_key".to_string(),
            django_secret_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "admin_password".to_string(),
            admin_pass_path.to_string_lossy().to_string(),
        );

        // bakerydemo espera DATABASE_URL y SECRET_KEY como env vars estándar.
        // SEC-001: Secrets are mounted as Docker secrets at /run/secrets/ and
        // injected via an entrypoint wrapper script. This avoids plaintext
        // secrets in the container's env (visible via `docker inspect`).
        let mut web_env = HashMap::new();
        web_env.insert(
            "ENOLA_WAGTAIL_INTERNAL_PORT".to_string(),
            WAGTAIL_INTERNAL_PORT.to_string(),
        );

        // SEC-001: Entrypoint wrapper that reads secrets from /run/secrets/ and
        // exports them as env vars before exec'ing the original Wagtail entrypoint.
        let entrypoint_script = r#"#!/bin/sh
# SEC-001: Read secrets from /run/secrets/ and export as env vars
DB_PASS="$(cat /run/secrets/db_password)"
DJANGO_SECRET="$(cat /run/secrets/django_secret_key)"
export DATABASE_URL="postgres://wagtail:${DB_PASS}@${DB_HOST:-db}:5432/wagtail"
export SECRET_KEY="${DJANGO_SECRET}"
exec "$@"
"#;
        let entrypoint_path = inst_dir.join("entrypoint.sh");
        std::fs::write(&entrypoint_path, entrypoint_script).map_err(|e| {
            EnolaError::InfrastructureError(format!("Cannot write entrypoint script: {}", e))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&entrypoint_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Cannot set entrypoint permissions: {}",
                        e
                    ))
                })?;
        }

        // Mount the entrypoint wrapper into the container
        web_volumes.insert(
            entrypoint_path.to_string_lossy().to_string(),
            "/entrypoint.sh".to_string(),
        );

        let web_config = ContainerConfig {
            name: web_name.clone(),
            image: self.descriptor().default_image.to_string(),
            // SEC-001: Use entrypoint wrapper to inject secrets from files
            command: Some(vec![
                "/bin/sh".to_string(),
                "/entrypoint.sh".to_string(),
                "python".to_string(),
                "manage.py".to_string(),
                "runserver".to_string(),
                "0.0.0.0:8000".to_string(),
            ]),
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
            // SEC-019: Wagtail needs write access for content
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(web_config).await?;
        self.container_manager.start_container(&web_name).await?;
        let _ = self.manifest.append("docker_container", &web_name);

        Ok(CmsInstance {
            kind: CmsKind::Wagtail,
            name: request.name,
            status: CmsStatus::Initializing, // Django migrations + collectstatic
            http_port: Some(http_port),
            db_port: None, // BD no expuesta a host
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let db_name = Self::db_container_name(name);
        let web_name = Self::web_container_name(name);
        // Postgres primero para que Django no falle el connect inicial.
        self.container_manager.start_container(&db_name).await?;
        self.container_manager.start_container(&web_name).await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);
        let containers = self.container_manager.list_containers(true).await?;
        let exists = containers
            .iter()
            .any(|c| c.name == web_name || c.name == db_name);
        if !exists {
            return Err(EnolaError::NotFound(format!(
                "Wagtail instance '{}' not found. Use `wagtail list` to see existing instances.",
                name
            )));
        }
        // Django primero para no dejar conexiones colgadas a Postgres.
        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        Ok(())
    }

    async fn delete(&self, name: &str, force: bool) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);

        if !force {
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
                    "Cannot delete Wagtail '{}': containers still running ({}). \
                     Use --force or stop first.",
                    name,
                    running.join(", ")
                )));
            }
        }

        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        let _ = self.container_manager.remove_container(&web_name).await;
        let _ = self.container_manager.remove_container(&db_name).await;
        let _ = self.manifest.remove("docker_container", &web_name);
        let _ = self.manifest.remove("docker_container", &db_name);
        // Remove the Docker network (same pattern as Drupal).
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = wagtail_base_dir();
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

        // Estado agregado (§13.5): Running solo si AMBOS contenedores están up.
        let status = match (web, db) {
            (None, None) => CmsStatus::NotFound,
            (Some(w), Some(d)) => {
                let w_up = w.status.to_lowercase().starts_with("up");
                let d_up = d.status.to_lowercase().starts_with("up");
                if w_up && d_up {
                    CmsStatus::Running
                } else {
                    CmsStatus::Stopped
                }
            }
            _ => CmsStatus::Stopped, // un contenedor existe pero el otro no
        };

        let http_port = web.and_then(|c| Self::parse_http_port(&c.ports));

        Ok(CmsInstance {
            kind: CmsKind::Wagtail,
            name: name.to_string(),
            status,
            http_port,
            db_port: None,
            onion_address: super::read_onion_address(&format!("wagtail-{}", name)),
        })
    }
}

/// Escribe un secreto en disco con permisos 0600 (mismo patrón SEC-005 que WP/Drupal).
/// Crea el directorio padre con modo 0700 si no existe.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to create Wagtail secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to chmod Wagtail secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
    }
    let path = dir.join(name);
    std::fs::write(&path, value).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Failed to write Wagtail secret {}: {}",
            path.display(),
            e
        ))
    })?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Failed to chmod Wagtail secret {}: {}",
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

    /// Mutex global para serializar mutaciones de ENOLA_WAGTAIL_BASE_DIR (§13.33).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_test_base() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ENOLA_WAGTAIL_BASE_DIR", tmp.path());
        (tmp, guard)
    }

    fn teardown_test_base(_tmp: TempDir, _guard: std::sync::MutexGuard<'static, ()>) {
        std::env::remove_var("ENOLA_WAGTAIL_BASE_DIR");
    }

    #[test]
    fn descriptor_matches_wagtail_metadata() {
        let mock = MockContainerPort::new();
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Wagtail);
        assert_eq!(d.kind.slug(), "wagtail");
        assert_eq!(d.default_image, "wagtail/bakerydemo:latest");
        assert_eq!(d.db_stack, DbStack::Postgres);
        assert_eq!(d.container_prefix, "wagtail-");
        assert_eq!(d.data_root, "/srv/enola-wagtail");
        assert!(d.requires_db(), "Wagtail+Postgres requires external DB");
        assert!(d.setup_wizard_status_codes.contains(&200));
        assert!(d.setup_wizard_status_codes.contains(&302));
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(WagtailCmsAdapter::validate_name("").is_err());
        assert!(WagtailCmsAdapter::validate_name("my site").is_err());
        assert!(WagtailCmsAdapter::validate_name("my/site").is_err());
        assert!(WagtailCmsAdapter::validate_name("my$site").is_err());
        assert!(WagtailCmsAdapter::validate_name("mysite").is_ok());
        assert!(WagtailCmsAdapter::validate_name("my_site-1").is_ok());
    }

    #[test]
    fn naming_uses_wagtail_prefix_and_does_not_collide_with_other_cms() {
        // §13.3 + §13.41 — prefijos distintos garantizan aislamiento Tor/Nginx.
        assert_eq!(
            WagtailCmsAdapter::web_container_name("blog"),
            "wagtail-blog"
        );
        assert_eq!(
            WagtailCmsAdapter::db_container_name("blog"),
            "db-blog-wagtail"
        );
        assert_eq!(
            WagtailCmsAdapter::network_name("blog"),
            "enola_net_wagtail_blog"
        );
        // Anti-regresión: wagtail-{name} != wp-{name} != drupal-{name} != ghost-{name}.
        let blog = "blog";
        assert_ne!(
            WagtailCmsAdapter::web_container_name(blog),
            format!("wp-{}", blog)
        );
        assert_ne!(
            WagtailCmsAdapter::web_container_name(blog),
            format!("drupal-{}", blog)
        );
        assert_ne!(
            WagtailCmsAdapter::web_container_name(blog),
            format!("ghost-{}", blog)
        );
        // El sufijo `-wagtail` evita colisión con `db-{name}-drupal`.
        assert_ne!(
            WagtailCmsAdapter::db_container_name(blog),
            format!("db-{}-drupal", blog)
        );
    }

    #[test]
    fn parse_http_port_handles_127001_format() {
        let ports = vec!["127.0.0.1:8085->8000/tcp".to_string()];
        assert_eq!(WagtailCmsAdapter::parse_http_port(&ports), Some(8085));
    }

    #[test]
    fn parse_http_port_handles_short_format() {
        let ports = vec!["8090->8000/tcp".to_string()];
        assert_eq!(WagtailCmsAdapter::parse_http_port(&ports), Some(8090));
    }

    #[test]
    fn parse_http_port_returns_none_for_unrelated_ports() {
        // Puerto 80 (WP/Drupal) NO debe matchear con Wagtail (8000).
        let ports = vec!["8080->80/tcp".to_string()];
        assert_eq!(WagtailCmsAdapter::parse_http_port(&ports), None);
        // Puerto 2368 (Ghost) tampoco.
        let ports = vec!["8080->2368/tcp".to_string()];
        assert_eq!(WagtailCmsAdapter::parse_http_port(&ports), None);
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
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
    async fn create_provisions_two_containers_postgres_and_returns_initializing() {
        let (tmp, guard) = setup_test_base();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        // Postgres ⇒ DOS create_container (vs Ghost que llama 1 vez).
        mock.expect_create_container()
            .times(2)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(2).returning(|_| Ok(()));

        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "myblog".to_string(),
            http_port: Some(8085),
            db_password: Some("custompass".to_string()),
        };
        let inst = adapter.create(req).await.expect("create should succeed");
        assert_eq!(inst.kind, CmsKind::Wagtail);
        assert_eq!(inst.name, "myblog");
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
        assert!(inst.onion_address.is_none());
        teardown_test_base(tmp, guard);
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_only_when_both_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "wagtail-blog".into(),
                    image: "wagtail/bakerydemo:latest".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec!["127.0.0.1:8085->8000/tcp".into()],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-blog-wagtail".into(),
                    image: "postgres:16-alpine".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_only_web_up() {
        // Importante: si Postgres está caído, Django no funciona — mostramos Stopped.
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "wagtail-blog".into(),
                    image: "wagtail/bakerydemo:latest".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec!["127.0.0.1:8085->8000/tcp".into()],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-blog-wagtail".into(),
                    image: "postgres:16-alpine".into(),
                    status: "Exited (0) 1 minute ago".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("blog").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "wagtail-blog".into(),
                image: "wagtail/bakerydemo:latest".into(),
                status: "Up 1 minute".into(),
                ports: vec!["127.0.0.1:8085->8000/tcp".into()],
            }])
        });
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("blog", false).await;
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string().to_lowercase();
        assert!(msg.contains("running") || msg.contains("--force"));
    }

    #[tokio::test]
    async fn delete_with_force_removes_both_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().times(0); // skip running check con --force
        mock.expect_stop_container().times(2).returning(|_| Ok(()));
        mock.expect_remove_container()
            .times(2)
            .returning(|_| Ok(()));
        mock.expect_remove_network().returning(|_| Ok(()));
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("blog", true).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn start_starts_db_before_web() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_start_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("blog").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // Postgres ⇒ DOS starts (vs Ghost que llama 1: web only).
        assert_eq!(recorded.len(), 2);
        // Postgres PRIMERO para que Django no falle el connect inicial.
        assert_eq!(recorded[0], "db-blog-wagtail");
        assert_eq!(recorded[1], "wagtail-blog");
    }

    #[tokio::test]
    async fn stop_stops_web_before_db() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                crate::ports::container::ContainerInfo {
                    id: "a".into(),
                    name: "wagtail-blog".into(),
                    image: "wagtail".into(),
                    status: "Up".into(),
                    ports: vec![],
                },
                crate::ports::container::ContainerInfo {
                    id: "b".into(),
                    name: "db-blog-wagtail".into(),
                    image: "postgres".into(),
                    status: "Up".into(),
                    ports: vec![],
                },
            ])
        });
        mock.expect_stop_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = WagtailCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.stop("blog").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // Django PRIMERO para no dejar conexiones colgadas a Postgres.
        assert_eq!(recorded[0], "wagtail-blog");
        assert_eq!(recorded[1], "db-blog-wagtail");
    }
}
