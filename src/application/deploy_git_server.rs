use crate::domain::error::{EnolaError, Result};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::manifest::ManifestPort;
use crate::ports::port_checker::PortCheckerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{NginxManagerPort, NginxProxyConfigWithSsl};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Checks whether a port is free at both OS level and Docker level.
/// Docker stopped containers still hold port bindings in Docker's state
/// even though `TcpListener::bind` would succeed (the port isn't bound to
/// the OS socket until the container restarts).
fn is_port_free_for_docker(port: u16) -> bool {
    use std::process::Command;
    if std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
        return false;
    }
    let ports_output = Command::new("docker")
        .args(["ps", "-a", "--format", "{{.Ports}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let port_s = port.to_string();
    for line in ports_output.lines() {
        if line.contains(&format!(":{port_s}->")) || line.contains(&format!(":{port_s}/")) {
            return false;
        }
    }
    true
}

/// Application Service for deploying a Git Server (Forgejo)
/// Port logic from: scripts/git/deploy_git.sh
pub struct DeployGitServer {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    #[allow(dead_code)]
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    port_checker: Option<Arc<dyn PortCheckerPort + Send + Sync>>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeployGitServer {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            tor_manager,
            nginx_manager,
            port_checker: None,
            manifest,
        }
    }

    pub fn with_port_checker(mut self, pc: Arc<dyn PortCheckerPort + Send + Sync>) -> Self {
        self.port_checker = Some(pc);
        self
    }

    pub async fn execute(
        &self,
        service_name: &str,
        http_port: u16,
        ssh_port: u16,
        enable_ssl: bool,
        // Modo CLI: si se proporcionan, el primer usuario admin se crea automáticamente.
        // Modo Web: si son None, Forgejo arranca con registro abierto y el usuario
        //           completa el setup en http://localhost:<http_port>/ desde el navegador.
        admin_user: Option<&str>,
        admin_pass: Option<&str>,
    ) -> Result<String> {
        // 1. Validate Service Name
        if service_name.is_empty()
            || !service_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid service name: {}",
                service_name
            )));
        }

        // 2. Verify ports are available BEFORE doing any Tor/Nginx/Docker work.
        //    Si hay PortCheckerPort injectable (tests) se usa; si no, check directo de OS+Docker.
        for (port, label) in [(http_port, "HTTP"), (ssh_port, "SSH")] {
            if port == 0 {
                continue;
            }
            let free = if let Some(pc) = &self.port_checker {
                pc.check_port(port)
                    .map(|r| r.free_os && r.free_docker)
                    .unwrap_or(false)
            } else {
                is_port_free_for_docker(port)
            };
            if !free {
                return Err(EnolaError::ValidationError(format!(
                    "{} port {} is not available. \
                     A running or stopped Docker container may be using it. \
                     Use `docker ps -a --format '{{{{.Ports}}}}'` to identify it.",
                    label, port
                )));
            }
        }

        // 3. Prepare Container Config — /srv/enola-git/{name}
        let host_data_path = PathBuf::from("/srv/enola-git").join(service_name);

        // Create the data directory with correct ownership BEFORE starting the container.
        // Forgejo runs as UID/GID 1000 and needs write access to /data.
        // With no-new-privileges, the container cannot chown root-owned directories.
        // Best-effort: if this fails (e.g. in unit tests without root), Docker will
        // create the directory automatically when mounting the volume.
        let _ = std::fs::create_dir_all(&host_data_path);
        // chown to 1000:1000 (matching USER_UID/USER_GID env vars)
        let _ = std::process::Command::new("chown")
            .args(["-R", "1000:1000", &host_data_path.to_string_lossy()])
            .status();

        let mut ports = HashMap::new();
        ports.insert(http_port, 3000u16);
        ports.insert(ssh_port, 22u16);

        let mut volumes = HashMap::new();
        volumes.insert(
            host_data_path.to_string_lossy().to_string(),
            "/data".to_string(),
        );

        let mut env = HashMap::new();
        env.insert("USER_UID".to_string(), "1000".to_string());
        env.insert("USER_GID".to_string(), "1000".to_string());
        // Configuración base de Forgejo via ENV (formato: FORGEJO__sección__CLAVE)
        env.insert(
            "FORGEJO__server__DOMAIN".to_string(),
            "localhost".to_string(),
        );
        env.insert(
            "FORGEJO__server__ROOT_URL".to_string(),
            format!("http://localhost:{}/", http_port),
        );
        env.insert("FORGEJO__server__HTTP_PORT".to_string(), "3000".to_string());
        env.insert(
            "FORGEJO__database__DB_TYPE".to_string(),
            "sqlite3".to_string(),
        );
        env.insert(
            "FORGEJO__database__PATH".to_string(),
            "/data/gitea/gitea.db".to_string(),
        );

        // PQC-010: Hardening del servidor SSH interno de Forgejo.
        //
        // LIMITACIÓN PQC: Forgejo 9.x usa su propio servidor SSH escrito en Go
        // (crypto/ssh), NO OpenSSH. Por tanto NO soporta el algoritmo PQC híbrido
        // sntrup761x25519-sha512@openssh.com todavía.
        //
        // Medidas aplicadas ahora (best-effort con crypto/ssh de Go):
        //   - MACs: solo hmac-sha2-256 y hmac-sha2-512 (elimina md5/sha1)
        //   - HostKey: preferir Ed25519 sobre RSA
        //   - No es posible forzar KexAlgorithms PQC en el SSH interno de Go
        //
        // Cuando Forgejo soporte OpenSSH externo estable (PQC-010-UPDATE):
        //   1. Añadir FORGEJO__server__START_SSH_SERVER=false
        //   2. Montar sshd_config con KexAlgorithms sntrup761x25519-sha512@openssh.com
        //   3. Actualizar tarea: enola-cli git edit --ssh-harden-pqc
        //   See PQC documentation
        env.insert("FORGEJO__server__SSH_SERVER_MACS".to_string(),
                   "hmac-sha2-256-etm@openssh.com,hmac-sha2-512-etm@openssh.com,hmac-sha2-256,hmac-sha2-512".to_string());
        env.insert(
            "FORGEJO__server__SSH_SERVER_HOST_KEYS".to_string(),
            "ssh/gitea.rsa,ssh/gitea.ed25519".to_string(),
        );

        match (admin_user, admin_pass) {
            (Some(_user), Some(_pass)) => {
                // ── MODO CLI ──────────────────────────────────────────────
                // INSTALL_LOCK=true hace que Forgejo salte el wizard de instalación.
                // El admin NO se crea via ENV en Forgejo 9.x — se crea después
                // vía `forgejo admin user create` dentro del contenedor
                // (ver git::create en commands.rs, Fase 4).
                //
                // NO pasar DISABLE_REGISTRATION via ENV: Forgejo sobreescribe app.ini
                // con los valores ENV en cada reinicio, lo que impide que
                // `git registration --disable` persista. Se controla solo via app.ini.
                env.insert(
                    "FORGEJO__security__INSTALL_LOCK".to_string(),
                    "true".to_string(),
                );
            }
            _ => {
                // ── MODO WEB ──────────────────────────────────────────────
                // Forgejo arranca con el asistente de instalación web activo.
                // El usuario abre http://localhost:<puerto>/ en su navegador,
                // rellena el formulario de configuración y crea su cuenta.
                // No se toca INSTALL_LOCK → Forgejo muestra el wizard de setup.
            }
        }

        // AA-002: inyectar perfil AppArmor si está activo en el kernel y cargado.
        // Degrada silenciosamente en WSL2 / kernels sin AppArmor.
        let profile_name = format!("enola-git-{}", service_name);
        let mut security_opt = Vec::new();
        if let Some(aa_opt) =
            crate::infrastructure::security_opt::apparmor_profile_opt(&profile_name)
        {
            security_opt.push(aa_opt);
        }

        let config = ContainerConfig {
            name: format!("enola-git-{}", service_name),
            image: "codeberg.org/forgejo/forgejo:9.0".to_string(),
            env,
            ports,
            volumes,
            command: None,
            gpu_support: false,
            network: None,
            restart_policy: Some("unless-stopped".to_string()),
            auto_remove: false,
            working_dir: None,
            security_opt,
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: HashMap::new(),
            // SEC-019: Forgejo needs write access for repos and DB
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };

        // 4. Deploy Container
        self.container_manager.create_container(config).await?;
        self.container_manager
            .start_container(&format!("enola-git-{}", service_name))
            .await?;
        let _ = self
            .manifest
            .append("docker_container", &format!("enola-git-{}", service_name));

        // Si se proporcionaron credenciales admin (modo CLI), guardarlas localmente
        // para que `git user list/create/delete` puedan autenticarse sin pedirlas.
        if let (Some(user), Some(pass)) = (admin_user, admin_pass) {
            // SEC-EXT-LIC-032: credenciales auxiliares con permisos owner-only.
            let _ = write_admin_creds_hash(&host_data_path, user, pass);
        }

        // 5. Configure Nginx Reverse Proxy with SSL (only when --ssl and Nginx available)
        //    Tor is NOT auto-published on create for security: the Forgejo setup wizard
        //    would be exposed on Tor before the user configures the admin account.
        //    The user must run `git publish <name>` after completing the setup.
        if enable_ssl {
            if let Some(nginx_manager) = &self.nginx_manager {
                let https_port = nginx_manager.find_available_port(15001, 20000).await?;

                let (cert_path, key_path) = nginx_manager
                    .generate_self_signed_cert(service_name)
                    .await?;

                let ssl_config = NginxProxyConfigWithSsl {
                    service_name: service_name.to_string(),
                    http_port,
                    https_port,
                    backend_port: http_port,
                    server_name: "localhost".to_string(),
                    ssl_cert_path: cert_path,
                    ssl_key_path: key_path,
                    rate_limit: None,
                };

                nginx_manager
                    .create_proxy_config_with_ssl(ssl_config)
                    .await?;
                nginx_manager
                    .enable_site(&format!("proxy_{}", service_name))
                    .await?;
                nginx_manager.reload().await?;
            }
        }

        let local_addr = format!("localhost:{}", http_port);

        // Mensaje de éxito con instrucciones según el modo elegido
        let instructions = match admin_user {
            Some(user) => format!(
                "\n   🔑 Admin creado: usuario='{}' (la contraseña es la que elegiste)\
                 \n   🌐 Accede en: http://127.0.0.1:{}\
                 \n   ⚠️  Completa el setup, luego publica en Tor: enola-cli git publish {}",
                user, http_port, service_name
            ),
            None => format!(
                "\n   🌐 Abre en el navegador: http://127.0.0.1:{}\
                 \n      → Se mostrará el asistente de instalación web de Forgejo.\
                 \n      → Crea tu cuenta de administrador desde el formulario.\
                 \n   ⚠️  Completa el setup, luego publica en Tor: enola-cli git publish {}",
                http_port, service_name
            ),
        };

        Ok(format!("{}{}", local_addr, instructions))
    }
}

fn write_admin_creds_hash(creds_dir: &std::path::Path, user: &str, pass: &str) -> Result<()> {
    let creds_path = creds_dir.join(".enola-admin-creds");
    // SEC-002: Store bcrypt hash instead of plaintext password.
    let hashed = bcrypt::hash(pass, 12)
        .map_err(|e| EnolaError::InfrastructureError(format!("bcrypt hash failed: {}", e)))?;
    let content = format!(
        "ADMIN_USER={}\nADMIN_PASS_HASH={}\nADMIN_PASS_HASH_ALGO=bcrypt\n",
        user, hashed
    );
    std::fs::create_dir_all(creds_dir).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Cannot create creds dir {}: {}",
            creds_dir.display(),
            e
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // SEC-FIX-GIT-PERMS (2026-05-14): El directorio raíz de datos Forgejo DEBE ser 0o755
        // para que el proceso git (UID 1000) pueda atravesarlo. Antes era 0o700 (root-only),
        // lo que hacía que Forgejo fallara al leer /data/gitea/conf/app.ini con permission denied.
        // La protección de las credenciales viene del archivo .enola-admin-creds (0o600, root:root):
        // el directorio no necesita permisos 700 porque el FILE ya es solo-owner.
        std::fs::set_permissions(creds_dir, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| {
                EnolaError::InfrastructureError(format!(
                    "Cannot set creds dir permissions {}: {}",
                    creds_dir.display(),
                    e
                ))
            },
        )?;
    }

    std::fs::write(&creds_path, content).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Cannot write creds file {}: {}",
            creds_path.display(),
            e
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| {
                EnolaError::InfrastructureError(format!(
                    "Cannot set creds file permissions {}: {}",
                    creds_path.display(),
                    e
                ))
            },
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::container::{ContainerInfo, ContainerStats};
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::port_checker::{PortCheckResult, PortCheckerPort};
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::Mutex;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    struct FreePortChecker;
    impl PortCheckerPort for FreePortChecker {
        fn check_port(&self, port: u16) -> crate::domain::error::Result<PortCheckResult> {
            Ok(PortCheckResult {
                port,
                free_os: true,
                free_docker: true,
            })
        }
        fn find_free_port(&self, start: u16, _end: u16) -> crate::domain::error::Result<u16> {
            Ok(start)
        }
    }

    struct MockContainerManager {
        should_fail: bool,
        fail_on_start: bool,
        created_containers: Mutex<Vec<String>>,
        started_containers: Mutex<Vec<String>>,
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                fail_on_start: false,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                fail_on_start: false,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
            }
        }

        fn failing_on_start() -> Self {
            Self {
                should_fail: false,
                fail_on_start: true,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManager {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            Ok(vec![])
        }
        async fn create_container(&self, config: ContainerConfig) -> Result<String> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Create failed".into()))
            } else {
                self.created_containers
                    .lock()
                    .unwrap()
                    .push(config.name.clone());
                Ok(config.name)
            }
        }
        async fn start_container(&self, id: &str) -> Result<()> {
            if self.fail_on_start {
                Err(EnolaError::InfrastructureError("Start failed".into()))
            } else {
                self.started_containers.lock().unwrap().push(id.to_string());
                Ok(())
            }
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
            Ok("logs".into())
        }
        async fn inspect_container(&self, _id: &str) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn execute_command(&self, _id: &str, _cmd: Vec<String>) -> Result<String> {
            Ok("output".into())
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
            Ok((0, "success".into()))
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

    struct MockTorManager {
        should_fail: bool,
        deployed_services: Mutex<Vec<String>>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                deployed_services: Mutex::new(vec![]),
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                deployed_services: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            Ok(vec![])
        }
        async fn deploy_hidden_service(&self, name: &str, _: Vec<(u16, u16)>) -> Result<String> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Deploy failed".into()))
            } else {
                self.deployed_services
                    .lock()
                    .unwrap()
                    .push(name.to_string());
                Ok(format!("{}.onion", name))
            }
        }
        async fn remove_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn get_onion_address(&self, _: &str) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn reload_tor(&self) -> Result<()> {
            Ok(())
        }
        async fn generate_client_keys(&self, _: &str) -> Result<(String, String)> {
            Ok(("priv".into(), "pub".into()))
        }
        async fn add_client_auth(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke_client_auth(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn start_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn rotate_hidden_service_identity(&self, _: &str) -> Result<String> {
            Ok("new.onion".into())
        }
    }

    #[tokio::test]
    async fn test_deploy_git_server_success() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(
            container.clone(),
            tor.clone(),
            None,
            Arc::new(mock_manifest()),
        )
        .with_port_checker(Arc::new(FreePortChecker));

        let result = use_case
            .execute("mygit", 58500, 58501, false, None, None)
            .await;

        assert!(result.is_ok(), "execute failed: {:?}", result.err());
        let addr = result.unwrap();
        assert!(addr.contains("localhost"));

        // Verify container was created and started
        let created = container.created_containers.lock().unwrap();
        assert_eq!(created.len(), 1);
        assert!(created[0].contains("enola-git-mygit"));

        let started = container.started_containers.lock().unwrap();
        assert_eq!(started.len(), 1);

        // Tor is NOT auto-published on create (manual publish model)
        let deployed = tor.deployed_services.lock().unwrap();
        assert_eq!(deployed.len(), 0);
    }

    #[tokio::test]
    async fn test_deploy_git_server_empty_name() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case.execute("", 58510, 58511, false, None, None).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid service name"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn write_admin_creds_hash_sets_0755_dir_and_0600_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("git-data");
        write_admin_creds_hash(&dir, "admin", "secret").expect("write creds");

        let file = dir.join(".enola-admin-creds");
        let dir_meta = std::fs::metadata(&dir).expect("dir metadata");
        let file_meta = std::fs::metadata(&file).expect("file metadata");
        let dir_mode = dir_meta.permissions().mode() & 0o777;
        let file_mode = file_meta.permissions().mode() & 0o777;
        // SEC-FIX-GIT-PERMS: directorio 0o755 (Forgejo UID 1000 debe poder atravesarlo),
        // archivo 0o600 (solo root puede leer las credenciales admin).
        assert_eq!(dir_mode, 0o755);
        assert_eq!(file_mode, 0o600);

        let uid = unsafe { libc::geteuid() };
        assert_eq!(dir_meta.uid(), uid);
        assert_eq!(file_meta.uid(), uid);

        // SEC-002: Verify content is hashed, not plaintext
        let content = std::fs::read_to_string(&file).expect("read creds");
        assert!(content.contains("ADMIN_USER=admin"));
        assert!(content.contains("ADMIN_PASS_HASH=$2b$"));
        assert!(content.contains("ADMIN_PASS_HASH_ALGO=bcrypt"));
        // Plaintext password must NOT appear in the file
        assert!(!content.contains("ADMIN_PASS=secret"));
        assert!(!content.contains("ADMIN_PASS=secret\n"));
    }

    #[tokio::test]
    async fn test_deploy_git_server_invalid_name_with_spaces() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case
            .execute("my git server", 58506, 58507, false, None, None)
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid service name"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_git_server_invalid_name_with_special_chars() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case
            .execute("git@server!", 58508, 58509, false, None, None)
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid service name"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_git_server_valid_name_with_underscore() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case
            .execute("my_git_server", 58502, 58503, false, None, None)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deploy_git_server_valid_name_with_hyphen() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case
            .execute("my-git-server", 58504, 58505, false, None, None)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deploy_git_server_container_create_failure() {
        let container = Arc::new(MockContainerManager::failing());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        // Puertos únicos para evitar race condition con tests paralelos (§flaky-ports)
        let result = use_case
            .execute("mygit", 58506, 58507, false, None, None)
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::InfrastructureError(msg)) => {
                assert!(msg.contains("Create failed"));
            }
            _ => panic!("Expected InfrastructureError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_git_server_container_start_failure() {
        let container = Arc::new(MockContainerManager::failing_on_start());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        // Puertos únicos para evitar race condition con tests paralelos (§flaky-ports)
        let result = use_case
            .execute("mygit", 58508, 58509, false, None, None)
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::InfrastructureError(msg)) => {
                assert!(msg.contains("Start failed"));
            }
            _ => panic!("Expected InfrastructureError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_git_server_tor_failure_is_graceful() {
        // Tor is not called during create (manual publish model).
        // A failing Tor manager should not affect deployment at all.
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::failing());
        let use_case = DeployGitServer::new(container, tor, None, Arc::new(mock_manifest()));

        let result = use_case
            .execute("mygit", 59500, 59501, false, None, None)
            .await;

        // Should succeed — Tor is not invoked during create
        assert!(result.is_ok());
        let address = result.unwrap();
        assert!(address.contains("localhost"));
    }
}
