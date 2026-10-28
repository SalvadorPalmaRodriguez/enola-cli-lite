// CMS-MAGNOLIA-001 (2026-05-05) — Adapter Magnolia CMS Community (Java/Tomcat) del catálogo.
//
// Primer representante Java/JVM del catálogo (13.56). A25-N3 (2026-10): ya no usa
// la imagen comunitaria abandonada (2020) con H2 embebido; Enola construye su propia
// imagen y el repositorio JCR vive en PostgreSQL (contenedor aparte), como Wagtail.
//
// Stack:
//   - Web: `enola/magnolia:<ver>-jdk<NN>` construida en local desde el Dockerfile
//          embebido: Magnolia CE 6.4 (GPLv3, WAR oficial verificado por SHA-256) sobre
//          Tomcat 10.1 + Java 21 LTS, driver JDBC de PostgreSQL añadido, proceso
//          `enola-magnolia` sin root y aplicación de solo lectura.
//   - BD:  `postgres:16-alpine` en contenedor separado (`DbStack::Postgres`).
//
// ⚠️ RAM: el contenedor web tiene un límite de 3 GB (decisión del operador); la JVM
// va con `-Xms512m -Xmx1024m`. El adaptador emite una advertencia en `create` si la
// RAM libre del SO es <1536 MB.
//
// Naming (13.3 — prefijos distintos garantizan aislamiento Tor/Nginx):
//   - Web:         `magnolia-{name}`
//   - DB:          `db-{name}-magnolia`
//   - Network:     `enola_net_magnolia_{name}`
//
// Paths (13.2):
//   - Datos:       `/srv/enola-magnolia/{name}/data/`    → `/data` (repositorio, índices, claves)
//   - DB:          `/srv/enola-magnolia/{name}/db/`      → `/var/lib/postgresql/data`
//   - Secrets:     `/srv/enola-magnolia/{name}/secrets/` → `/run/secrets/` (modo 0700/0600)
//
// Secretos generados (SEC-005), leídos por la imagen desde ficheros (nunca env/argv):
//   - `db_password`    → clave de PostgreSQL (o la de `CmsCreateRequest.db_password`).
//   - `admin_password` → contraseña del usuario `superuser` de Magnolia (16 chars).
//     Desde Magnolia 6.4.4 no existe contraseña por defecto: se fija la primera vez
//     con `magnolia.superuser.bootstrap.password_file`.
//
// Setup wizard (13.1): Magnolia instala sus módulos en el primer arranque
// (`magnolia.update.auto=true`, ~30 s) y responde 401 en `/` hasta que se hace login
// (instancia author). Códigos aceptables durante boot: 200/301/302/304/401/500.
//
// Puerto interno: Tomcat escucha en **8080**.
// El binding host:contenedor mapea http_port (host) → 8080 (container).
//
// Docker binding (13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).

use crate::domain::cms::{
    CmsCreateRequest, CmsDescriptor, CmsInstance, CmsKind, CmsStatus, DbStack,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::cms::{CmsAdapter, CmsLifecycle};
use crate::ports::container::{ContainerConfig, ContainerPort, ImageBuildConfig};
use crate::ports::manifest::ManifestPort;
use crate::ports::service_user::ServiceUserPort;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Puerto interno donde Magnolia/Tomcat escucha dentro del contenedor.
const MAGNOLIA_INTERNAL_PORT: u16 = 8080;

/// RAM mínima recomendada en MB para Magnolia (JVM overhead + Tomcat + Magnolia).
const MAGNOLIA_MIN_RAM_MB: u64 = 1536;

/// Límite de memoria del contenedor web: 3 GB (decisión del operador, 2026-10-28).
const MAGNOLIA_MEMORY_LIMIT_BYTES: i64 = 3 * 1024 * 1024 * 1024;

/// Directorio base por defecto para datos persistentes de Magnolia.
const DEFAULT_MAGNOLIA_BASE_DIR: &str = "/srv/enola-magnolia";

/// Resuelve el directorio base. En tests, se inyecta via `new_with_base`.
fn magnolia_base_dir(override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => PathBuf::from(DEFAULT_MAGNOLIA_BASE_DIR),
    }
}

/// Adapter Magnolia Community: web (Tomcat) + PostgreSQL. Resuelve además el
/// usuario de sistema bajo el que corre el proceso (SD-9/SD-10).
pub struct MagnoliaCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    base_dir: Option<PathBuf>,
    service_user: Arc<dyn ServiceUserPort + Send + Sync>,
    /// Directorio del contexto de build; `None` = el canónico `/opt/enola/docker/magnolia`.
    build_dir: Option<PathBuf>,
}

impl MagnoliaCmsAdapter {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            manifest,
            base_dir: None,
            service_user: Arc::new(crate::adapters::infra::service_user::SystemServiceUser::new()),
            build_dir: None,
        }
    }

    /// Constructor para tests: inyecta el base_dir sin usar env vars (thread-safe),
    /// el usuario de servicio (sin `useradd` real) y el directorio de build.
    #[cfg(test)]
    pub fn new_with_base(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
        base_dir: PathBuf,
        service_user: Arc<dyn ServiceUserPort + Send + Sync>,
        build_dir: PathBuf,
    ) -> Self {
        Self {
            container_manager,
            manifest,
            base_dir: Some(base_dir),
            service_user,
            build_dir: Some(build_dir),
        }
    }

    /// A25-N3: construye `enola/magnolia:<ver>` desde el contexto embebido si la
    /// imagen no existe. Se llama ANTES de tocar red, secretos o contenedores,
    /// así un build fallido no deja residuos.
    async fn ensure_image(&self) -> Result<()> {
        use crate::infrastructure::embedded_scripts as emb;
        let tag = emb::MAGNOLIA_IMAGE_TAG;
        if self.container_manager.image_exists(tag).await? {
            return Ok(());
        }
        let ctx = match &self.build_dir {
            Some(dir) => emb::ensure_magnolia_context_in(dir),
            None => emb::ensure_magnolia_context(),
        }
        .map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Cannot prepare the Magnolia build context: {e}"
            ))
        })?;
        eprintln!(
            "🏗️  Building the Magnolia image {tag} (first time only, ~3-6 min, ~900 MB, needs Internet)..."
        );
        self.container_manager
            .build_image(ImageBuildConfig {
                dockerfile_path: ctx.join(emb::MAGNOLIA_DOCKERFILE_NAME),
                context_path: ctx,
                tag: tag.to_string(),
                build_args: HashMap::new(),
            })
            .await?;
        Ok(())
    }

    fn web_container_name(name: &str) -> String {
        format!("magnolia-{}", name)
    }

    fn db_container_name(name: &str) -> String {
        format!("db-{}-magnolia", name)
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
        default_image: crate::domain::images::MAGNOLIA.image,
        // A25-N3: repositorio JCR en PostgreSQL (contenedor aparte), sin H2.
        db_stack: DbStack::Postgres,
        // 13.1: Magnolia instala sus módulos en el primer arranque (~30 s) y, al ser
        // una instancia author, responde 401 hasta el login.
        setup_wizard_status_codes: &[200, 301, 302, 304, 401, 500],
        container_prefix: "magnolia-",
        data_root: "/srv/enola-magnolia",
        http_port_range: (8080, 9000),
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

        // 0. Imagen propia (se construye una vez si falta).
        self.ensure_image().await?;

        let web_name = Self::web_container_name(&request.name);
        let db_name = Self::db_container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Secretos (mismo patrón SEC-005).
        let base = magnolia_base_dir(self.base_dir.as_deref());
        // SEC-A7: directorio base de datos en 0700 (secretos escritos por los contenedores).
        crate::infrastructure::safe_dir::secure_service_base_dir(&base).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Insecure data directory {}: {}",
                base.display(),
                e
            ))
        })?;
        let inst_dir = base.join(&request.name);
        let secrets_dir = inst_dir.join("secrets");
        let db_pass = request
            .db_password
            .clone()
            .unwrap_or_else(|| Self::generate_password(20));
        let admin_pass = Self::generate_password(16);
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;
        let admin_pass_path = write_secret_file(&secrets_dir, "admin_password", &admin_pass)?;

        // 3. Volúmenes + usuario de servicio. El proceso corre como `enola-magnolia`
        // (sin root, `cap_drop ALL`): `/data` y los secretos que lee desde
        // /run/secrets tienen que ser suyos. Postgres sigue leyendo `db_password`
        // gracias a DAC_OVERRIDE (DB_CAPS).
        let db_volume = inst_dir.join("db");
        let data_volume = inst_dir.join("data");
        std::fs::create_dir_all(&data_volume).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Cannot create {}: {}",
                data_volume.display(),
                e
            ))
        })?;
        let (magnolia_uid, magnolia_gid) = self
            .service_user
            .ensure(crate::domain::naming::MAGNOLIA_SERVICE_USER)?;
        self.service_user
            .chown_tree(&data_volume, magnolia_uid, magnolia_gid)?;
        self.service_user
            .chown_tree(&secrets_dir, magnolia_uid, magnolia_gid)?;

        // 4. Contenedor BD (Postgres 16). POSTGRES_PASSWORD_FILE lee la clave de
        // /run/secrets sin ponerla en la línea de comandos.
        let mut db_env = HashMap::new();
        db_env.insert("POSTGRES_DB".to_string(), "magnolia".to_string());
        db_env.insert("POSTGRES_USER".to_string(), "magnolia".to_string());
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
        let db_config = ContainerConfig {
            name: db_name.clone(),
            image: crate::domain::images::POSTGRES.image.to_string(),
            // A25-07/08: tag fijo + digest + cap_drop ALL / cap_add mínimo (domain::images).
            image_digest: crate::domain::images::POSTGRES.digest_string(),
            cap_drop: crate::domain::images::ImageSpec::cap_drop_all(),
            cap_add: crate::domain::images::POSTGRES.cap_add_vec(),
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

        // 5. Contenedor Magnolia (imagen propia `enola/magnolia`, A25-N3). La imagen
        // lleva su ENTRYPOINT (genera la config de Jackrabbit desde los secretos y
        // lanza Tomcat). Los secretos llegan como ficheros en /run/secrets (SEC-001).
        let mut web_volumes = HashMap::new();
        web_volumes.insert(
            data_volume.to_string_lossy().to_string(),
            "/data".to_string(),
        );
        let mut web_ports = HashMap::new();
        web_ports.insert(http_port, MAGNOLIA_INTERNAL_PORT);
        let mut web_secrets = HashMap::new();
        web_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "admin_password".to_string(),
            admin_pass_path.to_string_lossy().to_string(),
        );

        // Solo configuración no sensible.
        let mut web_env = HashMap::new();
        web_env.insert("MAGNOLIA_DB_HOST".to_string(), db_name.clone());
        web_env.insert("MAGNOLIA_DB_NAME".to_string(), "magnolia".to_string());
        web_env.insert("MAGNOLIA_DB_USER".to_string(), "magnolia".to_string());
        // Java JVM heap: 512 MB mínimo, 1 GB máximo (el resto del límite de 3 GB es
        // para metaspace, Tomcat, el índice Lucene fuera del heap y el SO).
        web_env.insert("JAVA_OPTS".to_string(), "-Xms512m -Xmx1024m".to_string());

        let web_config = ContainerConfig {
            name: web_name.clone(),
            image: crate::domain::images::MAGNOLIA.image.to_string(),
            // Imagen construida en local: sin digest (el tag fija la versión).
            image_digest: crate::domain::images::MAGNOLIA.digest_string(),
            cap_drop: crate::domain::images::ImageSpec::cap_drop_all(),
            cap_add: crate::domain::images::MAGNOLIA.cap_add_vec(),
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
            memory_limit: Some(MAGNOLIA_MEMORY_LIMIT_BYTES),
            nano_cpus: None,
            pids_limit: None,
            secrets: web_secrets,
            // SEC-019: la aplicación es de root y de solo lectura; Magnolia escribe
            // en `/data` (volumen) y Tomcat en logs/temp/work. Se mantiene rw hasta
            // validar `read_only_rootfs` con E2E.
            read_only_rootfs: false,
            no_new_privileges: true,
            user: Some(format!("{magnolia_uid}:{magnolia_gid}")),
        };
        self.container_manager.create_container(web_config).await?;
        self.container_manager.start_container(&web_name).await?;
        let _ = self.manifest.append("docker_container", &web_name);

        Ok(CmsInstance {
            kind: CmsKind::Magnolia,
            name: request.name,
            status: CmsStatus::Initializing, // Tomcat + instalación de módulos en JCR
            http_port: Some(http_port),
            db_port: None, // BD no expuesta a host
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        // Postgres primero para que Jackrabbit no falle el connect inicial.
        self.container_manager
            .start_container(&Self::db_container_name(name))
            .await?;
        self.container_manager
            .start_container(&Self::web_container_name(name))
            .await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);
        let containers = self.container_manager.list_containers(true).await?;
        if !containers
            .iter()
            .any(|c| c.name == web_name || c.name == db_name)
        {
            return Err(EnolaError::NotFound(format!(
                "Magnolia instance '{}' not found. Use `magnolia list` to see existing instances.",
                name
            )));
        }
        // Tomcat primero para no dejar conexiones colgadas a Postgres.
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
                    "Cannot delete Magnolia '{}': containers still running ({}). \
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
        // Remove the Docker network.
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = magnolia_base_dir(self.base_dir.as_deref());
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
        let up =
            |c: &crate::ports::container::ContainerInfo| c.status.to_lowercase().starts_with("up");
        let status = match (web, db) {
            (None, None) => CmsStatus::NotFound,
            (Some(w), Some(d)) if up(w) && up(d) => CmsStatus::Running,
            _ => CmsStatus::Stopped,
        };

        let http_port = web.and_then(|c| Self::parse_http_port(&c.ports));

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

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    #[test]
    fn descriptor_matches_magnolia_metadata() {
        let mock = MockContainerPort::new();
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Magnolia);
        assert_eq!(d.kind.slug(), "magnolia");
        assert_eq!(d.default_image, crate::domain::images::MAGNOLIA.image);
        assert_eq!(d.db_stack, DbStack::Postgres);
        assert_eq!(d.container_prefix, "magnolia-");
        assert_eq!(d.data_root, "/srv/enola-magnolia");
        assert!(d.requires_db(), "Magnolia+PostgreSQL requires external DB");
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
        assert_eq!(
            MagnoliaCmsAdapter::web_container_name("blog"),
            "magnolia-blog"
        );
        assert_eq!(
            MagnoliaCmsAdapter::db_container_name("blog"),
            "db-blog-magnolia"
        );
        assert_eq!(
            MagnoliaCmsAdapter::network_name("blog"),
            "enola_net_magnolia_blog"
        );
        let name = "cms";
        // Anti-regresión: magnolia-{name} != wp/drupal/ghost/wagtail/strapi-{name}.
        assert_ne!(
            MagnoliaCmsAdapter::web_container_name(name),
            format!("wp-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::web_container_name(name),
            format!("drupal-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::web_container_name(name),
            format!("ghost-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::web_container_name(name),
            format!("wagtail-{}", name)
        );
        assert_ne!(
            MagnoliaCmsAdapter::web_container_name(name),
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

    const GIB3: i64 = 3 * 1024 * 1024 * 1024;

    /// Usuario de servicio simulado: `enola-magnolia` = 990:991.
    fn mock_user() -> crate::ports::service_user::MockServiceUserPort {
        let mut u = crate::ports::service_user::MockServiceUserPort::new();
        u.expect_ensure()
            .withf(|n| n == "enola-magnolia")
            .returning(|_| Ok((990, 991)));
        u.expect_chown_tree().returning(|_, _, _| Ok(()));
        u
    }

    fn adapter_with(
        mock: MockContainerPort,
        user: crate::ports::service_user::MockServiceUserPort,
        tmp: &Path,
    ) -> MagnoliaCmsAdapter {
        MagnoliaCmsAdapter::new_with_base(
            Arc::new(mock),
            Arc::new(mock_manifest()),
            tmp.join("data"),
            Arc::new(user),
            tmp.join("ctx"),
        )
    }

    fn create_req() -> CmsCreateRequest {
        CmsCreateRequest {
            name: "myblog".to_string(),
            http_port: Some(8085),
            db_password: Some("custompass".to_string()),
        }
    }

    fn info(name: &str, status: &str, ports: Vec<String>) -> ContainerInfo {
        ContainerInfo {
            id: name.into(),
            name: name.into(),
            image: "enola/magnolia:6.4.11-jdk21".into(),
            status: status.into(),
            ports,
        }
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "blog".to_string(),
            http_port: None,
            db_password: None,
        };
        let err = adapter
            .create(req)
            .await
            .unwrap_err()
            .to_string()
            .to_lowercase();
        assert!(err.contains("http_port"), "got: {err}");
    }

    #[tokio::test]
    async fn create_provisions_two_containers_postgres_and_returns_initializing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(true));
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container()
            .times(2)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(2).returning(|_| Ok(()));

        let adapter = adapter_with(mock, mock_user(), tmp.path());
        let inst = adapter.create(create_req()).await.expect("create ok");
        assert_eq!(inst.kind, CmsKind::Magnolia);
        assert_eq!(inst.name, "myblog");
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
    }

    /// A25-N3: si la imagen no existe, `create` la construye desde el contexto
    /// embebido ANTES de crear red, secretos o contenedores.
    #[tokio::test]
    async fn create_builds_the_image_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists()
            .withf(|i| i == crate::domain::images::MAGNOLIA.image)
            .returning(|_| Ok(false));
        mock.expect_build_image()
            .times(1)
            .withf(|c| {
                c.tag == crate::domain::images::MAGNOLIA.image
                    && c.dockerfile_path.ends_with("Dockerfile.magnolia")
                    && c.dockerfile_path.is_file()
                    && c.context_path.join("magnolia_enola.properties").is_file()
            })
            .returning(|c| Ok(c.tag));
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(|c| Ok(c.name));
        mock.expect_start_container().returning(|_| Ok(()));

        let adapter = adapter_with(mock, mock_user(), tmp.path());
        adapter.create(create_req()).await.expect("create ok");
    }

    #[tokio::test]
    async fn create_skips_the_build_when_the_image_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(true));
        mock.expect_build_image().times(0);
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(|c| Ok(c.name));
        mock.expect_start_container().returning(|_| Ok(()));
        let adapter = adapter_with(mock, mock_user(), tmp.path());
        adapter.create(create_req()).await.expect("create ok");
    }

    #[tokio::test]
    async fn create_leaves_no_residue_when_the_build_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(false));
        mock.expect_build_image()
            .times(1)
            .returning(|_| Err(EnolaError::InfrastructureError("maven falló".into())));
        mock.expect_create_network().times(0);
        mock.expect_create_container().times(0);
        let adapter = adapter_with(mock, mock_user(), tmp.path());
        let err = adapter.create(create_req()).await.unwrap_err().to_string();
        assert!(err.contains("maven falló"), "got: {err}");
        assert!(!tmp.path().join("data/myblog").exists());
    }

    /// SD-9/SD-10 + decisión del operador (3 GB): web endurecido, con la imagen
    /// propia, sin secretos en el entorno, y BD PostgreSQL aparte sin puertos.
    #[tokio::test]
    async fn create_runs_web_hardened_and_db_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::<ContainerConfig>::new()));
        let sink = seen.clone();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(true));
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(move |c| {
            sink.lock().unwrap().push(c.clone());
            Ok(c.name)
        });
        mock.expect_start_container().returning(|_| Ok(()));
        let adapter = adapter_with(mock, mock_user(), tmp.path());
        adapter.create(create_req()).await.expect("create ok");

        let seen = seen.lock().unwrap();
        let web = seen
            .iter()
            .find(|c| c.name == "magnolia-myblog")
            .expect("web");
        assert_eq!(web.image, crate::domain::images::MAGNOLIA.image);
        assert_eq!(web.image_digest, None);
        assert_eq!(web.user.as_deref(), Some("990:991"));
        assert_eq!(web.cap_drop, vec!["ALL".to_string()]);
        assert!(web.cap_add.is_empty());
        assert!(web.no_new_privileges);
        assert_eq!(web.memory_limit, Some(GIB3), "límite de 3 GB (operador)");
        assert_eq!(web.command, None, "manda el ENTRYPOINT de la imagen");
        assert_eq!(web.ports.get(&8085), Some(&8080));
        assert!(web.volumes.values().any(|v| v == "/data"));
        let mut keys: Vec<_> = web.secrets.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["admin_password", "db_password"]);
        assert_eq!(
            web.env.get("MAGNOLIA_DB_HOST").map(String::as_str),
            Some("db-myblog-magnolia")
        );
        assert_eq!(
            web.env.get("JAVA_OPTS").map(String::as_str),
            Some("-Xms512m -Xmx1024m")
        );
        for (k, v) in &web.env {
            let k = k.to_lowercase();
            assert!(
                !(k.contains("pass") || k.contains("secret") || k.contains("key")),
                "variable sospechosa: {k}={v}"
            );
        }

        let db = seen
            .iter()
            .find(|c| c.name == "db-myblog-magnolia")
            .expect("db");
        assert_eq!(db.image, crate::domain::images::POSTGRES.image);
        assert!(db.ports.is_empty(), "la BD no se expone al host");
        assert_eq!(db.network, web.network);
        assert_eq!(
            db.env.get("POSTGRES_PASSWORD_FILE").map(String::as_str),
            Some("/run/secrets/db_password")
        );
        assert!(db.volumes.values().any(|v| v == "/var/lib/postgresql/data"));
    }

    /// El proceso sin root solo escribe `/data` y lee los secretos si son suyos.
    #[tokio::test]
    async fn create_hands_secrets_and_data_to_the_service_user() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(true));
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(|c| Ok(c.name));
        mock.expect_start_container().returning(|_| Ok(()));
        let mut user = crate::ports::service_user::MockServiceUserPort::new();
        user.expect_ensure().returning(|_| Ok((990, 991)));
        let inst = tmp.path().join("data/myblog");
        let (data, secrets) = (inst.join("data"), inst.join("secrets"));
        user.expect_chown_tree()
            .withf(move |p, u, g| (p == data || p == secrets) && *u == 990 && *g == 991)
            .times(2)
            .returning(|_, _, _| Ok(()));
        let adapter = adapter_with(mock, user, tmp.path());
        adapter.create(create_req()).await.expect("create ok");
        assert!(inst.join("data").is_dir());
    }

    /// `db_password` es la clave de PostgreSQL (como en Wagtail/Drupal); la del
    /// administrador se genera siempre y es otro secreto.
    #[tokio::test]
    async fn create_writes_distinct_db_and_admin_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockContainerPort::new();
        mock.expect_image_exists().returning(|_| Ok(true));
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container().returning(|c| Ok(c.name));
        mock.expect_start_container().returning(|_| Ok(()));
        let adapter = adapter_with(mock, mock_user(), tmp.path());
        adapter.create(create_req()).await.expect("create ok");
        let sec = tmp.path().join("data/myblog/secrets");
        let db = std::fs::read_to_string(sec.join("db_password")).unwrap();
        let admin = std::fs::read_to_string(sec.join("admin_password")).unwrap();
        assert_eq!(db, "custompass");
        assert_eq!(admin.len(), 16);
        assert_ne!(db, admin);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(sec.join("admin_password"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_only_when_both_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                info(
                    "magnolia-mycms",
                    "Up 2 minutes",
                    vec!["127.0.0.1:8085->8080/tcp".into()],
                ),
                info("db-mycms-magnolia", "Up 2 minutes", vec![]),
            ])
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_only_web_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                info(
                    "magnolia-mycms",
                    "Up 2 minutes",
                    vec!["127.0.0.1:8085->8080/tcp".into()],
                ),
                info("db-mycms-magnolia", "Exited (0) 1 minute ago", vec![]),
            ])
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("mycms").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers()
            .returning(|_| Ok(vec![info("magnolia-mycms", "Up 2 minutes", vec![])]));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let msg = adapter
            .delete("mycms", false)
            .await
            .unwrap_err()
            .to_string()
            .to_lowercase();
        assert!(
            msg.contains("running") || msg.contains("--force"),
            "got: {msg}"
        );
    }

    /// Un delete que dejase el contenedor PostgreSQL huérfano conservaría la BD.
    #[tokio::test]
    async fn delete_with_force_removes_both_containers_and_the_network() {
        let removed = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = removed.clone();
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().times(0);
        mock.expect_stop_container().times(2).returning(|_| Ok(()));
        mock.expect_remove_container()
            .times(2)
            .returning(move |id| {
                sink.lock().unwrap().push(id.to_string());
                Ok(())
            });
        mock.expect_remove_network()
            .withf(|n| n == "enola_net_magnolia_mycms")
            .times(1)
            .returning(|_| Ok(()));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.delete("mycms", true).await.unwrap();
        let mut got = removed.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["db-mycms-magnolia", "magnolia-mycms"]);
    }

    #[tokio::test]
    async fn start_starts_db_before_web() {
        let mut mock = MockContainerPort::new();
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = calls.clone();
        mock.expect_start_container().returning(move |id| {
            sink.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("mycms").await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            ["db-mycms-magnolia", "magnolia-mycms"]
        );
    }

    #[tokio::test]
    async fn stop_stops_web_before_db() {
        let mut mock = MockContainerPort::new();
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = calls.clone();
        mock.expect_list_containers()
            .returning(|_| Ok(vec![info("magnolia-mycms", "Up", vec![])]));
        mock.expect_stop_container().returning(move |id| {
            sink.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.stop("mycms").await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            ["magnolia-mycms", "db-mycms-magnolia"]
        );
    }

    #[tokio::test]
    async fn stop_unknown_instance_is_not_found() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        assert!(adapter.stop("nope").await.is_err());
    }
}
