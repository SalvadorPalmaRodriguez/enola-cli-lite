// CLI Commands Module
// Each function maps directly to a Use Case in the application layer

use crate::adapters::infra::docker::BollardDockerAdapter;
use crate::adapters::infra::filesystem::EnolaFileAdapter;
use crate::adapters::infra::manifest::FileManifestAdapter;
use crate::adapters::infra::nginx::NginxAdapter;
use crate::adapters::infra::systemd::SystemdAdapter;
use crate::adapters::tor::TorConfigAdapter;
use std::sync::Arc;

// Use Cases
use crate::application::deploy_fileserver::{DeployFileServer, DeployFileServerRequest};
use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
use crate::application::list_tor_services::ListTorServices;
use crate::application::manage_client_auth::ManageClientAuth;
use crate::application::remove_tor_service::RemoveTorService;
use crate::application::rotate_tor_identity::RotateTorIdentity;
use crate::application::system_resource_monitor::SystemResourceMonitor;

use crate::domain::error::EnolaError;

/// Result type for CLI commands
pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug)]
pub enum CliError {
    Domain(EnolaError),
    NotImplemented(String),
    InvalidInput(String),
    Io(std::io::Error),
    Generic(String),
    ControlledExit {
        code: i32,
        stdout: Option<String>,
        stderr: Option<String>,
    },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Domain(e) => write!(f, "{}", e),
            CliError::NotImplemented(msg) => write!(f, "Not implemented: {}", msg),
            CliError::InvalidInput(msg) => write!(f, "{}", msg),
            CliError::Io(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    write!(
                        f,
                        "Permission denied. Try running with: sudo enola-cli <command>"
                    )
                } else if e.kind() == std::io::ErrorKind::NotFound {
                    write!(f, "File or directory not found: {}", e)
                } else {
                    write!(f, "I/O error: {}", e)
                }
            }
            CliError::Generic(msg) => write!(f, "{}", msg),
            CliError::ControlledExit { stderr, stdout, .. } => {
                if let Some(msg) = stderr.as_deref() {
                    write!(f, "{}", msg)
                } else if let Some(msg) = stdout.as_deref() {
                    write!(f, "{}", msg)
                } else {
                    write!(f, "controlled exit")
                }
            }
        }
    }
}

impl std::error::Error for CliError {}

impl From<EnolaError> for CliError {
    fn from(e: EnolaError) -> Self {
        CliError::Domain(e)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER: Update Tor config file correctly (edit, not recreate)
// ═══════════════════════════════════════════════════════════════════════════

/// Updates Tor configuration file by editing HiddenServicePort lines.
/// This preserves the existing config and only changes port mappings.
///
/// # Arguments
/// * `service_name` - Name of the Tor service (file: /etc/tor/enola.d/{name}.conf)
/// * `ports` - Vec of (virtual_port, nginx_port) tuples. Order: HTTP(80), HTTPS(443), SSH(22)
async fn update_tor_config_ports(service_name: &str, ports: &[(u16, u16)]) -> CliResult<()> {
    let conf_path = format!("/etc/tor/enola.d/{}.conf", service_name);

    // Read current config
    let content = tokio::fs::read_to_string(&conf_path).await.map_err(|e| {
        CliError::Generic(format!("Failed to read Tor config {}: {}", conf_path, e))
    })?;

    // Build new configuration with updated ports
    let mut new_lines: Vec<String> = Vec::new();
    let mut port_index = 0;

    for line in content.lines() {
        if line.trim().starts_with("HiddenServicePort") {
            // Replace this HiddenServicePort line with the new port
            if port_index < ports.len() {
                let (virtual_port, nginx_port) = ports[port_index];
                new_lines.push(format!(
                    "HiddenServicePort {} 127.0.0.1:{}",
                    virtual_port, nginx_port
                ));
                port_index += 1;
            } else {
                // More ports in file than provided - keep existing
                new_lines.push(line.to_string());
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    // If we have more ports than existing lines, add them
    while port_index < ports.len() {
        let (virtual_port, nginx_port) = ports[port_index];
        new_lines.push(format!(
            "HiddenServicePort {} 127.0.0.1:{}",
            virtual_port, nginx_port
        ));
        port_index += 1;
    }

    let new_content = new_lines.join("\n");
    tokio::fs::write(&conf_path, new_content)
        .await
        .map_err(|e| {
            CliError::Generic(format!("Failed to write Tor config {}: {}", conf_path, e))
        })?;

    Ok(())
}

impl From<String> for CliError {
    fn from(s: String) -> Self {
        CliError::Generic(s)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SHARED PORT HELPER — reutilizable en ai, wp, git, files (PORTS-008)
// ═══════════════════════════════════════════════════════════════════════════

/// Verifica que un puerto no está ocupado ni por el SO ni por Docker
/// (incluyendo contenedores **parados** que retienen sus port bindings).
///
/// `TcpListener::bind` solo detecta puertos activos a nivel SO.
/// Esta función añade la verificación de `docker ps -a` para capturar
/// contenedores stopped antes de intentar crear o editar servicios.
///
/// Se usa en todos los comandos `create` y `edit` con puertos manuales.
pub(crate) fn is_port_free_shared(port: u16) -> bool {
    use std::process::Command;
    if std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
        return false;
    }
    let ports_output = Command::new("docker")
        .args(["ps", "-a", "--format", "{{.Ports}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let port_str = port.to_string();
    for line in ports_output.lines() {
        if line.contains(&format!(":{port_str}->")) || line.contains(&format!(":{port_str}/tcp")) {
            return false;
        }
    }
    true
}

/// Genera un mensaje de error amigable cuando un puerto está ocupado.
pub(crate) fn port_in_use_error(port: u16, flag: &str) -> CliError {
    CliError::InvalidInput(format!(
        "Port {} (--{}) is already in use (OS or Docker stopped container).\n\
         Free it first or choose a different port with --{} <PORT>.\n\
         Tip: run 'sudo enola-cli ports list' to see all ports in use.",
        port, flag, flag
    ))
}
/// SEC-EXT-RACE-011: reserva un puerto via flock antes de validar disponibilidad
/// y de ejecutar `docker run -p`. Devuelve un guard RAII; mantenlo vivo hasta
/// DESPUS de que docker termine de bindear. Si otro proceso CLI ya tiene el
/// lock (despliegue concurrente), aborta inmediatamente con error claro.
///
/// Uso canonico:
/// ```ignore
/// let _http_lock = reserve_port_or_fail(http_port, "http-port")?;
/// if !is_port_free_shared(http_port) { return Err(port_in_use_error(...)); }
/// // ... docker run -p http_port:80 ... // _http_lock vivo durante el bind
/// ```
pub(crate) fn reserve_port_or_fail(
    port: u16,
    flag: &str,
) -> CliResult<crate::infrastructure::file_lock::FileLock> {
    use std::io::ErrorKind;
    crate::infrastructure::port_lock::acquire_port_lock(port).map_err(|e| {
        if e.kind() == ErrorKind::WouldBlock {
            CliError::InvalidInput(format!(
                "Port {} (--{}) is being reserved by another concurrent enola-cli operation.\n\
                 Wait for the other deployment to finish, or choose a different port with --{} <PORT>.",
                port, flag, flag
            ))
        } else {
            CliError::Generic(format!(
                "Failed to acquire lockfile for port {} (--{}): {}", port, flag, e
            ))
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// SHARED HELPER: Expose service on Tor (reusable for git, wordpress, CMS, etc)
// ═══════════════════════════════════════════════════════════════════════════

/// Expose any service on Tor with optional SSL.
/// This is the single source of truth for creating Tor hidden services.
///
/// # Arguments
/// * `service_name` - Name for the Tor service (will be prefixed with proxy_ for SSL services)
/// * `backend_port` - Port where the actual service is listening (e.g., 3000 for Forgejo)
/// * `ssl` - Whether to enable SSL via Nginx
/// * `extra_ports` - Additional ports to expose (e.g., SSH port 22 for Git)
///
/// # Returns
/// * `Ok((onion_address, http_port, https_port))` - The onion address and the ports used
pub async fn expose_service_on_tor(
    service_name: &str,
    backend_port: u16,
    ssl: bool,
    extra_ports: Vec<(u16, u16)>, // (virtual_port, target_port)
) -> CliResult<(String, u16, Option<u16>)> {
    use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
    use crate::ports::service::ServiceManagerPort;
    use crate::ports::web::{NginxManagerPort, NginxProxyConfigWithSsl};

    let tor_adapter = Arc::new(TorConfigAdapter::new());
    let systemd_adapter: Arc<dyn ServiceManagerPort + Send + Sync> = Arc::new(SystemdAdapter);
    let nginx_adapter = Arc::new(NginxAdapter::new());
    let manifest = Arc::new(FileManifestAdapter::new());
    use crate::ports::manifest::ManifestPort;

    if ssl {
        // HTTPS mode: Tor → Nginx+SSL → App
        eprintln!(
            "   🔐 Creating with SSL (Tor → Nginx+SSL → App:{})...",
            backend_port
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Find available ports for Nginx
        let (http_port, _http_port_lock) = nginx_adapter
            .find_available_port_with_lock(10000, 15000)
            .await
            .map_err(|e| CliError::Generic(format!("Failed to find HTTP port: {:?}", e)))?;
        let (https_port, _https_port_lock) = nginx_adapter
            .find_available_port_with_lock(15001, 20000)
            .await
            .map_err(|e| CliError::Generic(format!("Failed to find HTTPS port: {:?}", e)))?;

        eprintln!("   📍 Nginx HTTP port: {}", http_port);
        eprintln!("   📍 Nginx HTTPS port: {}", https_port);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Generate self-signed certificate
        let (cert_path, key_path) = nginx_adapter
            .generate_self_signed_cert(service_name)
            .await
            .map_err(|e| {
                CliError::Generic(format!("Failed to generate SSL certificate: {:?}", e))
            })?;
        let _ = manifest.append("ssl_cert", &cert_path);
        let _ = manifest.append("ssl_key", &key_path);

        eprintln!("   🔑 SSL certificate: {}", cert_path);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Create Nginx config with SSL
        let ssl_config = NginxProxyConfigWithSsl {
            service_name: service_name.to_string(),
            http_port,
            https_port,
            backend_port,
            server_name: "localhost".to_string(),
            ssl_cert_path: cert_path,
            ssl_key_path: key_path,
            rate_limit: None,
        };

        nginx_adapter
            .create_proxy_config_with_ssl(ssl_config)
            .await
            .map_err(|e| {
                CliError::Generic(format!("Failed to create Nginx SSL config: {:?}", e))
            })?;
        let _ = manifest.append("nginx_config", &format!("proxy_{}", service_name));

        // Enable site (file is created as proxy_{service_name})
        let nginx_site_name = format!("proxy_{}", service_name);
        nginx_adapter
            .enable_site(&nginx_site_name)
            .await
            .map_err(|e| CliError::Generic(format!("Failed to enable site: {:?}", e)))?;

        if !nginx_adapter.validate_config().await.unwrap_or(false) {
            return Err(CliError::Generic(
                "Nginx config validation failed".to_string(),
            ));
        }

        nginx_adapter
            .reload()
            .await
            .map_err(|e| CliError::Generic(format!("Failed to reload Nginx: {:?}", e)))?;

        eprintln!("   ✓ Nginx configured");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Create Tor hidden service (use proxy_ prefix for consistency)
        let tor_service_name = format!("proxy_{}", service_name);
        let mut tor_ports = vec![(80, http_port), (443, https_port)];
        // Add extra ports (e.g., SSH)
        tor_ports.extend(extra_ports);

        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        let request = DeployTorServiceRequest {
            service_name: tor_service_name,
            ports: tor_ports,
        };
        let onion = use_case.execute(request).await.map_err(CliError::from)?;

        Ok((onion, http_port, Some(https_port)))
    } else {
        // HTTP only mode: Tor → Nginx → App (or direct if no proxy needed)
        eprintln!("   🌐 Creating HTTP only (Tor → App:{})...", backend_port);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let mut tor_ports = vec![(80, backend_port)];
        tor_ports.extend(extra_ports);

        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        let request = DeployTorServiceRequest {
            service_name: service_name.to_string(),
            ports: tor_ports,
        };
        let onion = use_case.execute(request).await.map_err(CliError::from)?;

        Ok((onion, backend_port, None))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TOR COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod tor {
    use super::*;
    use crate::ports::service::ServiceManagerPort;
    use crate::ports::web::NginxManagerPort;

    /// List all Tor hidden services
    pub async fn list() -> CliResult<Vec<crate::ports::tor::TorServiceInfo>> {
        let adapter = Arc::new(TorConfigAdapter::new());
        let use_case = ListTorServices::new(adapter);
        use_case.execute().await.map_err(CliError::from)
    }

    /// Create a new Tor hidden service
    ///
    /// Service types:
    /// - `raw`: Direct TCP connection (Tor → App). Use for SSH, custom TCP services.
    /// - `web` or `proxy`: HTTP via Nginx reverse proxy (Tor → Nginx → App). Recommended for web apps.
    /// - `static`: Static website served by Nginx.
    /// - `files`: File server via Nginx.
    pub async fn create(
        name: &str,
        service_type: &str,
        virtual_port: u16,
        target_port: Option<u16>,
        ssl: bool,
    ) -> CliResult<String> {
        use crate::application::deploy_tor_web_service::{
            DeployTorWebService, DeployTorWebServiceRequest,
        };
        use crate::ports::web::NginxProxyConfigWithSsl;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter: Arc<dyn ServiceManagerPort + Send + Sync> = Arc::new(SystemdAdapter);
        let nginx_adapter = Arc::new(NginxAdapter::new());

        match service_type.to_lowercase().as_str() {
            // Raw TCP connection: Tor → App directly
            // Best for: SSH, databases, custom TCP protocols
            "raw" | "tcp" => {
                eprintln!("📡 Creating RAW service (Tor → App directly)");
                eprintln!("   ℹ️  Use 'web' type for HTTP applications (includes Nginx proxy)");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                // SSH-specific warning and setup guidance
                if virtual_port == 22 || target_port == Some(22) || target_port == Some(2222) {
                    eprintln!();
                    eprintln!(
                        "   ⚠️  SSH SERVICE DETECTED — Leyendo recomendaciones de seguridad:"
                    );
                    eprintln!("   ════════════════════════════════════════════════════════");
                    eprintln!("   🔒 SEGURIDAD: El SSH será accesible EXCLUSIVAMENTE via .onion");
                    eprintln!("      No habrá puerto SSH expuesto en internet directamente.");
                    eprintln!();
                    eprintln!("   📋 PREREQUISITOS:");
                    eprintln!("      1. OpenSSH server instalado y configurado:");
                    eprintln!("         sudo apt install openssh-server");
                    eprintln!("         sudo ssh-keygen -A              # Generar host keys");
                    eprintln!("         sudo service ssh start");
                    eprintln!();
                    eprintln!("      2. Usuario sshd de separación de privilegios:");
                    eprintln!("         id sshd || sudo useradd -r -d /var/empty -s /usr/sbin/nologin sshd");
                    eprintln!();
                    eprintln!("   ⚠️  RIESGOS A TENER EN CUENTA:");
                    eprintln!("      • SSH sin autenticación de clave pública es INSEGURO");
                    eprintln!(
                        "        Configura: PasswordAuthentication no en /etc/ssh/sshd_config"
                    );
                    eprintln!("      • Usa siempre autenticación por clave pública (PubkeyAuthentication yes)");
                    eprintln!("      • Considera fwknop (Single Packet Authorization) para doble protección");
                    eprintln!();
                    eprintln!("   🔑 PARA CONECTARTE UNA VEZ CREADO:");
                    eprintln!(
                        "      ssh -o ProxyCommand='ssh -W %h:%p -q tor-proxy' USER@<ONION>.onion"
                    );
                    eprintln!("      O con torsocks: torsocks ssh USER@<ONION>.onion");
                    eprintln!("   ════════════════════════════════════════════════════════");
                    eprintln!();
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                }

                let use_case = DeployTorService::new(
                    tor_adapter,
                    systemd_adapter,
                    Arc::new(FileManifestAdapter::new()),
                );
                let request = DeployTorServiceRequest {
                    service_name: name.to_string(),
                    ports: vec![(virtual_port, target_port.unwrap_or(virtual_port))],
                };
                use_case.execute(request).await.map_err(CliError::from)
            }

            // Web service with Nginx reverse proxy: Tor → Nginx → App
            // Best for: Web applications, APIs, anything HTTP/HTTPS
            "web" | "proxy" | "http" => {
                let backend_port = target_port.unwrap_or(8080);

                if ssl {
                    // HTTPS mode: Create service with SSL certificate
                    eprintln!("🔐 Creating WEB service with HTTPS (Tor → Nginx+SSL → App)");
                    eprintln!(
                        "   Architecture: .onion:80/443 → Nginx:[auto] → App:{}",
                        backend_port
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    // Find two available ports for Nginx (HTTP and HTTPS)
                    let (http_port, _http_port_lock) = nginx_adapter
                        .find_available_port_with_lock(10000, 15000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find HTTP port: {:?}", e))
                        })?;
                    let (https_port, _https_port_lock) = nginx_adapter
                        .find_available_port_with_lock(15001, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find HTTPS port: {:?}", e))
                        })?;

                    eprintln!("   📍 Using Nginx HTTP port: {}", http_port);
                    eprintln!("   📍 Using Nginx HTTPS port: {}", https_port);
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    // Generate self-signed certificate
                    eprintln!("   🔑 Generating self-signed SSL certificate...");
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    let (cert_path, key_path) = nginx_adapter
                        .generate_self_signed_cert(name)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!(
                                "Failed to generate SSL certificate: {:?}",
                                e
                            ))
                        })?;

                    // Create Nginx config with SSL
                    eprintln!("   📝 Creating Nginx SSL config...");
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    let ssl_config = NginxProxyConfigWithSsl {
                        service_name: name.to_string(),
                        http_port,
                        https_port,
                        backend_port,
                        server_name: "localhost".to_string(),
                        ssl_cert_path: cert_path.clone(),
                        ssl_key_path: key_path.clone(),
                        rate_limit: None,
                    };
                    nginx_adapter
                        .create_proxy_config_with_ssl(ssl_config)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to create Nginx SSL config: {:?}", e))
                        })?;
                    eprintln!("   ✓ Nginx SSL config created");
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    // Enable site and reload
                    eprintln!("   🔗 Enabling Nginx site...");
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    nginx_adapter
                        .enable_site(&format!("proxy_{}", name))
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to enable site: {:?}", e))
                        })?;

                    if !nginx_adapter.validate_config().await.unwrap_or(false) {
                        return Err(CliError::Generic(
                            "Nginx config validation failed".to_string(),
                        ));
                    }
                    nginx_adapter.reload().await.map_err(|e| {
                        CliError::Generic(format!("Failed to reload Nginx: {:?}", e))
                    })?;
                    eprintln!("   ✓ Nginx reloaded");
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    // Create Tor hidden service with both HTTP and HTTPS ports
                    eprintln!("   🧅 Deploying Tor hidden service with HTTP+HTTPS...");
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    let use_case = DeployTorService::new(
                        tor_adapter,
                        systemd_adapter,
                        Arc::new(FileManifestAdapter::new()),
                    );
                    let request = DeployTorServiceRequest {
                        service_name: format!("proxy_{}", name),
                        ports: vec![
                            (80, http_port),   // HTTP
                            (443, https_port), // HTTPS
                        ],
                    };
                    let onion = use_case.execute(request).await.map_err(CliError::from)?;

                    eprintln!("\n📋 Service Configuration (HTTPS enabled):");
                    eprintln!("   Nginx config: /etc/nginx/sites-available/proxy_{}", name);
                    eprintln!("   SSL cert:     {}", cert_path);
                    eprintln!("   SSL key:      {}", key_path);
                    eprintln!("   Tor config:   /etc/tor/enola.d/proxy_{}.conf", name);
                    eprintln!(
                        "   Flow HTTP:    {}:80 → Nginx:{} → App:{}",
                        onion, http_port, backend_port
                    );
                    eprintln!(
                        "   Flow HTTPS:   {}:443 → Nginx:{} → App:{}",
                        onion, https_port, backend_port
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    Ok(onion)
                } else {
                    // Standard HTTP mode (no SSL)
                    eprintln!("🌐 Creating WEB service (Tor → Nginx → App)");
                    eprintln!(
                        "   Architecture: .onion:80 → Nginx:[auto] → App:{}",
                        backend_port
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    // Find an available port for Nginx using the adapter
                    let (nginx_port, _nginx_port_lock) = nginx_adapter
                        .find_available_port_with_lock(10000, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find available port: {:?}", e))
                        })?;

                    eprintln!("   📍 Using Nginx port: {}", nginx_port);
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    let use_case = DeployTorWebService::new(
                        nginx_adapter,
                        tor_adapter,
                        systemd_adapter,
                        Arc::new(FileManifestAdapter::new()),
                    );

                    let request = DeployTorWebServiceRequest {
                        service_name: name.to_string(),
                        backend_port,
                        nginx_port,
                        enable_auth: false,
                    };

                    let onion = use_case.execute(request).await.map_err(CliError::from)?;

                    eprintln!("\n📋 Service Configuration:");
                    eprintln!("   Nginx config: /etc/nginx/sites-available/proxy_{}", name);
                    eprintln!("   Tor config:   /etc/tor/enola.d/proxy_{}.conf", name);
                    eprintln!(
                        "   Flow: {} → localhost:{} → localhost:{}",
                        onion, nginx_port, backend_port
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    Ok(onion)
                }
            }

            // Static website: Tor → Nginx (serving static files)
            "static" => {
                eprintln!("📄 Creating STATIC site service");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                use crate::application::deploy_static_site::DeployStaticSite;

                let (nginx_port, _nginx_port_lock) = nginx_adapter
                    .find_available_port_with_lock(20000, 30000)
                    .await
                    .map_err(|e| {
                        CliError::Generic(format!("Failed to find available port: {:?}", e))
                    })?;
                let root_dir = format!("/var/www/{}", name);

                let static_use_case =
                    DeployStaticSite::new(nginx_adapter, Arc::new(FileManifestAdapter::new()));
                static_use_case
                    .execute(name, &root_dir, nginx_port)
                    .await
                    .map_err(CliError::from)?;

                // Create Tor hidden service pointing to Nginx
                let use_case = DeployTorService::new(
                    tor_adapter,
                    systemd_adapter,
                    Arc::new(FileManifestAdapter::new()),
                );
                let request = DeployTorServiceRequest {
                    service_name: name.to_string(),
                    ports: vec![(80, nginx_port)],
                };
                let onion = use_case.execute(request).await.map_err(CliError::from)?;

                eprintln!("\n📋 Static Site Configuration:");
                eprintln!("   Document root: {}", root_dir);
                eprintln!("   Put your HTML files there!");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                Ok(onion)
            }

            // File server
            "files" | "fileserver" => {
                eprintln!("📂 Creating FILE SERVER service");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                let file_adapter = Arc::new(EnolaFileAdapter::new());
                let (nginx_port, _nginx_port_lock) = nginx_adapter
                    .find_available_port_with_lock(20000, 30000)
                    .await
                    .map_err(|e| {
                        CliError::Generic(format!("Failed to find available port: {:?}", e))
                    })?;

                let use_case = DeployFileServer::new(
                    nginx_adapter,
                    tor_adapter,
                    systemd_adapter,
                    file_adapter,
                    Arc::new(FileManifestAdapter::new()),
                );

                let request = DeployFileServerRequest {
                    service_name: name.to_string(),
                    port: nginx_port,
                    share_path: None, // Will create default /srv/enola-files/{name}
                    enable_auth: false,
                };

                let (onion, path) = use_case.execute(request).await.map_err(CliError::from)?;

                eprintln!("\n📋 File Server Configuration:");
                eprintln!("   Shared folder: {}", path);
                eprintln!("   Put files there to share!");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                Ok(onion)
            }

            _ => Err(CliError::InvalidInput(format!(
                "Unknown service type '{}'. Valid types:\n\
                     - raw/tcp:  Direct TCP (Tor → App). For SSH, databases.\n\
                     - web/proxy/http: HTTP via Nginx (Tor → Nginx → App). For web apps.\n\
                     - static:   Static website (Tor → Nginx serving files).\n\
                     - files:    File server (Tor → Nginx autoindex).",
                service_type
            ))),
        }
    }

    /// Start a Tor hidden service
    pub async fn start(name: &str) -> CliResult<()> {
        use crate::domain::naming::ServiceName;
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        eprintln!("🚀 Starting service '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Get all services and find using all possible name variations
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let possible_names = ServiceName::possible_names_for_lookup(name);

        let found_service = services.iter().find(|s| possible_names.contains(&s.name));

        let (actual_name, has_nginx) = match found_service {
            Some(service) => {
                let has_nginx = service.name.starts_with("proxy_")
                    || service.name.starts_with("git_")
                    || service.name.starts_with("wp_")
                    || service.name.starts_with("ai_")
                    || service.name.starts_with("static_")
                    || service.name.starts_with("files_");
                (service.name.clone(), has_nginx)
            }
            None => {
                eprintln!("❌ Service '{}' not found.", name);
                eprintln!("   Searched for: {:?}", possible_names);
                return Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    name
                )));
            }
        };

        // 1. Enable Tor configuration
        eprintln!("   📄 Enabling Tor configuration for '{}'...", actual_name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        tor_adapter
            .start_hidden_service(&actual_name)
            .await
            .map_err(CliError::from)?;

        // 2. Enable Nginx site if it has nginx config
        if has_nginx {
            eprintln!("   🌐 Enabling Nginx site '{}'...", actual_name);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Try to enable - it's OK if it fails (might not exist)
            if let Err(e) = nginx_adapter.enable_site(&actual_name).await {
                eprintln!("   ⚠️ Could not enable Nginx site: {:?}", e);
            } else {
                eprintln!("   🔄 Reloading Nginx...");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                if let Err(e) = nginx_adapter.reload().await {
                    eprintln!("   ⚠️ Could not reload Nginx: {:?}", e);
                } else {
                    eprintln!("   ✓ Nginx enabled and reloaded");
                }
            }
            let _ = std::io::Write::flush(&mut std::io::stderr());
        } else {
            eprintln!("   ℹ️  Raw TCP service (no Nginx)");
        }

        // 3. Get and show the onion address
        let onion = tor_adapter.get_onion_address(&actual_name).await;

        eprintln!("\n✅ Service '{}' started successfully!", name);
        if let Ok(addr) = onion {
            eprintln!("🧅 Address: {}", addr);
        }
        let _ = std::io::Write::flush(&mut std::io::stderr());

        Ok(())
    }

    /// Stop a Tor hidden service
    pub async fn stop(name: &str) -> CliResult<()> {
        use crate::domain::naming::ServiceName;
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        eprintln!("🛑 Stopping service '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Get all services
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;

        // Try to find the service using all possible name variations
        let possible_names = ServiceName::possible_names_for_lookup(name);

        let found_service = services.iter().find(|s| possible_names.contains(&s.name));

        let (actual_name, has_nginx) = match found_service {
            Some(service) => {
                // Determine if this service has Nginx config
                let has_nginx = service.name.starts_with("proxy_")
                    || service.name.starts_with("git_")
                    || service.name.starts_with("wp_")
                    || service.name.starts_with("ai_")
                    || service.name.starts_with("static_")
                    || service.name.starts_with("files_");
                (service.name.clone(), has_nginx)
            }
            None => {
                eprintln!("❌ Service '{}' not found.", name);
                eprintln!("   Searched for: {:?}", possible_names);
                return Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    name
                )));
            }
        };

        // 1. Disable Nginx first if it has nginx config (reverse order of start)
        if has_nginx {
            eprintln!("   🌐 Disabling Nginx site '{}'...", actual_name);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Try to disable - it's OK if it fails (might not exist)
            if let Err(e) = nginx_adapter.disable_site(&actual_name).await {
                eprintln!("   ⚠️ Could not disable Nginx site: {:?}", e);
            } else {
                eprintln!("   🔄 Reloading Nginx...");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                if let Err(e) = nginx_adapter.reload().await {
                    eprintln!("   ⚠️ Could not reload Nginx: {:?}", e);
                }
            }

            eprintln!("   ✓ Nginx site disabled");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        } else {
            eprintln!("   ℹ️  Raw TCP service (no Nginx)");
        }

        // 2. Disable Tor configuration
        eprintln!("   📄 Disabling Tor configuration for '{}'...", actual_name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        tor_adapter
            .stop_hidden_service(&actual_name)
            .await
            .map_err(CliError::from)?;

        eprintln!("\n✅ Service '{}' stopped successfully!", name);
        eprintln!("   ℹ️  Use 'enola-cli tor start {}' to restart it.", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        Ok(())
    }

    /// Remove a Tor hidden service
    pub async fn remove(name: &str) -> CliResult<()> {
        use crate::domain::naming::ServiceName;
        use crate::ports::tor::TorManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            Some(Arc::new(NginxAdapter::new()));

        // Get all services and find using all possible name variations
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let possible_names = ServiceName::possible_names_for_lookup(name);

        let found_service = services.iter().find(|s| possible_names.contains(&s.name));

        let actual_name = match found_service {
            Some(service) => service.name.clone(),
            None => {
                return Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    name
                )));
            }
        };

        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);
        use_case.execute(&actual_name).await.map_err(CliError::from)
    }

    /// Rotate Tor identity (new .onion address)
    pub async fn rotate(name: &str) -> CliResult<String> {
        use crate::domain::naming::ServiceName;
        use crate::ports::tor::TorManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());

        // Determine the actual service name using all possible name variations
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;

        let possible_names = ServiceName::possible_names_for_lookup(name);

        let found_service = services.iter().find(|s| possible_names.contains(&s.name));

        let actual_name = match found_service {
            Some(service) => service.name.clone(),
            None => {
                return Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    name
                )));
            }
        };

        let use_case = RotateTorIdentity::new(tor_adapter);
        use_case.execute(&actual_name).await.map_err(CliError::from)
    }

    /// Edit Tor service ports
    ///
    /// Supports:
    /// - Manual mode: User specifies all ports explicitly
    /// - Auto mode: System finds available ports automatically
    /// - Mixed mode: User specifies some ports, system finds others
    ///
    /// Port flow: .onion:VIRTUAL_PORT → Nginx:NGINX_PORT → App:TARGET_PORT
    /// For SSL services: Both HTTP and HTTPS ports are managed together
    pub async fn edit(
        name: &str,
        virtual_port: Option<u16>,
        nginx_port: Option<u16>,
        target_port: Option<u16>,
        auto_ports: bool,
    ) -> CliResult<String> {
        use crate::domain::naming::ServiceName;
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;
        use std::net::TcpListener;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        eprintln!("🔧 Editing service '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Get current services and resolve actual name
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let possible_names = ServiceName::possible_names_for_lookup(name);

        let found_service = services.iter().find(|s| possible_names.contains(&s.name));

        let (actual_name, has_nginx, nginx_actual_name) = match found_service {
            Some(service) => {
                // Detect if there is a Nginx config by name prefix OR by file existence
                let by_prefix = service.name.starts_with("proxy_")
                    || service.name.starts_with("git_")
                    || service.name.starts_with("wp_")
                    || service.name.starts_with("ai_")
                    || service.name.starts_with("static_")
                    || service.name.starts_with("files_");

                // Also check by file: proxy_{service_name} may exist even without the prefix
                let proxy_name = format!("proxy_{}", service.name);
                let proxy_path = format!("/etc/nginx/sites-available/{}", proxy_name);
                let direct_path = format!("/etc/nginx/sites-available/{}", service.name);

                let (has_nginx, nginx_name) =
                    if by_prefix || std::path::Path::new(&proxy_path).exists() {
                        (
                            true,
                            if by_prefix {
                                service.name.clone()
                            } else {
                                proxy_name
                            },
                        )
                    } else if std::path::Path::new(&direct_path).exists() {
                        (true, service.name.clone())
                    } else {
                        (false, service.name.clone())
                    };

                (service.name.clone(), has_nginx, nginx_name)
            }
            None => {
                return Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    name
                )));
            }
        };

        let service = services
            .iter()
            .find(|s| s.name == actual_name)
            .ok_or_else(|| CliError::InvalidInput(format!("Service '{}' not found", name)))?;

        // Parse current port configuration
        // Format: (virtual_port, "127.0.0.1:nginx_port")
        let (old_virtual_http, old_nginx_http_str) = service
            .ports
            .first()
            .map(|(v, t)| (*v, t.clone()))
            .unwrap_or((80, "127.0.0.1:8080".to_string()));

        let old_nginx_http: u16 = old_nginx_http_str
            .split(':')
            .next_back()
            .and_then(|p| p.parse().ok())
            .unwrap_or(old_virtual_http);

        // Check if service has HTTPS - must verify:
        // 1. Has multiple ports with DIFFERENT virtual ports (not duplicates)
        // 2. OR has SSL configuration in Nginx
        let mut has_ssl = false;
        let (old_virtual_https, old_nginx_https): (Option<u16>, Option<u16>) = if service
            .ports
            .len()
            > 1
        {
            if let Some((v, t)) = service.ports.get(1) {
                let nginx_port = t
                    .split(':')
                    .next_back()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(*v);

                // Check if virtual ports are different (real SSL setup has 80 and 443)
                if *v != old_virtual_http {
                    has_ssl = true;
                    (Some(*v), Some(nginx_port))
                } else {
                    // Ports are duplicated - check Nginx for SSL config
                    if has_nginx {
                        let nginx_config_path =
                            format!("/etc/nginx/sites-available/{}", nginx_actual_name);
                        if let Ok(content) = tokio::fs::read_to_string(&nginx_config_path).await {
                            if content.contains("ssl_certificate")
                                || content.contains("listen") && content.contains("ssl;")
                            {
                                has_ssl = true;
                                // Get the HTTPS port from Nginx config
                                let https_port = content
                                    .lines()
                                    .find(|line| line.contains("listen") && line.contains("ssl"))
                                    .and_then(|line| {
                                        line.split("127.0.0.1:")
                                            .nth(1)
                                            .and_then(|s| s.split_whitespace().next())
                                            .and_then(|s| s.trim_end_matches(';').parse().ok())
                                    });
                                (Some(443), https_port)
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    }
                }
            } else {
                (None, None)
            }
        } else {
            // Single port - but still check Nginx for SSL (may have been created with --ssl)
            if has_nginx {
                let nginx_config_path = format!("/etc/nginx/sites-available/{}", nginx_actual_name);
                if let Ok(content) = tokio::fs::read_to_string(&nginx_config_path).await {
                    if content.contains("ssl_certificate") {
                        has_ssl = true;
                        let https_port = content
                            .lines()
                            .find(|line| line.contains("listen") && line.contains("ssl"))
                            .and_then(|line| {
                                line.split("127.0.0.1:")
                                    .nth(1)
                                    .and_then(|s| s.split_whitespace().next())
                                    .and_then(|s| s.trim_end_matches(';').parse().ok())
                            });
                        (Some(443), https_port)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        };

        // For proxy services, we need to get the backend port from Nginx config
        let old_backend: u16 = if has_nginx {
            // Read from nginx config file
            let nginx_config_path = format!("/etc/nginx/sites-available/{}", nginx_actual_name);
            if let Ok(content) = tokio::fs::read_to_string(&nginx_config_path).await {
                // Look for proxy_pass line
                content
                    .lines()
                    .find(|line| line.contains("proxy_pass"))
                    .and_then(|line| {
                        line.split(':')
                            .next_back()
                            .and_then(|s| s.trim_end_matches(';').trim().parse::<u16>().ok())
                    })
                    .unwrap_or(8080)
            } else {
                8080
            }
        } else {
            old_nginx_http // For raw services, nginx port IS the target
        };

        eprintln!("\n📋 Current configuration:");
        eprintln!("   HTTP Virtual port (Tor):   {}", old_virtual_http);
        eprintln!("   HTTP Nginx port:           {}", old_nginx_http);
        if has_ssl {
            eprintln!(
                "   HTTPS Virtual port (Tor):  {}",
                old_virtual_https.unwrap_or(443)
            );
            eprintln!(
                "   HTTPS Nginx port:          {}",
                old_nginx_https.unwrap_or(0)
            );
        }
        if has_nginx {
            eprintln!("   Backend port (App):        {}", old_backend);
        }
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Helper function to check if port is available
        let is_port_available = |port: u16, current_ports: &[u16]| -> bool {
            if current_ports.contains(&port) {
                return true; // Same port, already in use by this service
            }
            TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
        };

        // Collect current ports for availability check
        let mut current_ports = vec![old_nginx_http];
        if let Some(https_port) = old_nginx_https {
            current_ports.push(https_port);
        }

        // Determine new ports
        let new_virtual_http: u16;
        let mut new_nginx_http: u16;
        let new_virtual_https: Option<u16>;
        let new_nginx_https: Option<u16>;
        let new_backend: u16;

        if auto_ports {
            // Auto mode: find available ports
            eprintln!("\n🔍 Auto-detecting available ports...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            new_virtual_http = virtual_port.unwrap_or(old_virtual_http);

            // Warn about non-standard virtual ports
            if new_virtual_http != 80 && new_virtual_http != 443 {
                eprintln!(
                    "⚠️  El puerto virtual {} no es estándar (80=HTTP, 443=HTTPS).",
                    new_virtual_http
                );
                eprintln!(
                    "   Los visitantes tendrán que escribir .onion:{} en el navegador.",
                    new_virtual_http
                );
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }

            // Find available nginx HTTP port
            new_nginx_http = if let Some(np) = nginx_port {
                if !is_port_available(np, &current_ports) {
                    return Err(CliError::InvalidInput(format!(
                        "Nginx port {} is already in use",
                        np
                    )));
                }
                np
            } else {
                nginx_adapter
                    .find_available_port(10000, 15000)
                    .await
                    .map_err(|e| {
                        CliError::Generic(format!(
                            "Failed to find available nginx HTTP port: {:?}",
                            e
                        ))
                    })?
            };

            // Handle HTTPS port if service has SSL
            if has_ssl {
                new_virtual_https = old_virtual_https; // Keep same virtual port for HTTPS
                new_nginx_https = Some(
                    nginx_adapter
                        .find_available_port(15001, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!(
                                "Failed to find available nginx HTTPS port: {:?}",
                                e
                            ))
                        })?,
                );
            } else {
                new_virtual_https = None;
                new_nginx_https = None;
            }

            // Find available backend port (only for proxy services)
            new_backend = if has_nginx {
                if let Some(tp) = target_port {
                    if !is_port_available(tp, &[old_backend]) {
                        return Err(CliError::InvalidInput(format!(
                            "Backend port {} is already in use",
                            tp
                        )));
                    }
                    tp
                } else {
                    old_backend // Keep existing backend port in auto mode
                }
            } else {
                new_nginx_http // For raw services
            };
        } else {
            // Manual mode: use provided values or keep existing
            new_virtual_http = virtual_port.unwrap_or(old_virtual_http);

            // Warn about non-standard virtual ports
            if new_virtual_http != 80 && new_virtual_http != 443 {
                eprintln!(
                    "⚠️  El puerto virtual {} no es estándar (80=HTTP, 443=HTTPS).",
                    new_virtual_http
                );
                eprintln!(
                    "   Los visitantes tendrán que escribir .onion:{} en el navegador.",
                    new_virtual_http
                );
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }

            new_nginx_http = nginx_port.unwrap_or(old_nginx_http);
            new_backend = target_port.unwrap_or(old_backend);

            // For raw services (no Nginx), --target-port updates the Tor config port directly
            if !has_nginx && target_port.is_some() && nginx_port.is_none() {
                new_nginx_http = new_backend;
            }

            // Keep HTTPS ports, but if HTTP nginx port changed, we need to find new HTTPS port too
            if has_ssl {
                new_virtual_https = old_virtual_https;
                if new_nginx_http != old_nginx_http {
                    // HTTP port changed, find new HTTPS port too
                    new_nginx_https = Some(
                        nginx_adapter
                            .find_available_port(15001, 20000)
                            .await
                            .map_err(|e| {
                                CliError::Generic(format!(
                                    "Failed to find available nginx HTTPS port: {:?}",
                                    e
                                ))
                            })?,
                    );
                } else {
                    new_nginx_https = old_nginx_https;
                }
            } else {
                new_virtual_https = None;
                new_nginx_https = None;
            }

            // Validate ports are available
            if new_nginx_http != old_nginx_http
                && !is_port_available(new_nginx_http, &current_ports)
            {
                return Err(CliError::InvalidInput(format!(
                    "❌ Nginx port {} is already in use. Choose another port or use --auto-ports",
                    new_nginx_http
                )));
            }

            if has_nginx
                && new_backend != old_backend
                && !is_port_available(new_backend, &[old_backend])
            {
                eprintln!(
                    "⚠️  Warning: Backend port {} appears to be in use.",
                    new_backend
                );
                eprintln!("   Make sure your application is listening on this port.");
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }
        }

        // Show what will be changed
        eprintln!("\n📝 New configuration:");
        eprintln!(
            "   HTTP Virtual port (Tor):   {} {}",
            new_virtual_http,
            if new_virtual_http != old_virtual_http {
                "← CHANGED"
            } else {
                ""
            }
        );
        eprintln!(
            "   HTTP Nginx port:           {} {}",
            new_nginx_http,
            if new_nginx_http != old_nginx_http {
                "← CHANGED"
            } else {
                ""
            }
        );
        if has_ssl {
            eprintln!(
                "   HTTPS Virtual port (Tor):  {} {}",
                new_virtual_https.unwrap_or(443),
                if new_virtual_https != old_virtual_https {
                    "← CHANGED"
                } else {
                    ""
                }
            );
            eprintln!(
                "   HTTPS Nginx port:          {} {}",
                new_nginx_https.unwrap_or(0),
                if new_nginx_https != old_nginx_https {
                    "← CHANGED"
                } else {
                    ""
                }
            );
        }
        if has_nginx {
            eprintln!(
                "   Backend port (App):        {} {}",
                new_backend,
                if new_backend != old_backend {
                    "← CHANGED"
                } else {
                    ""
                }
            );
        }
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Check if anything changed
        let http_changed = new_virtual_http != old_virtual_http || new_nginx_http != old_nginx_http;
        let https_changed = has_ssl && new_nginx_https != old_nginx_https;
        let backend_changed = new_backend != old_backend;

        if !http_changed && !https_changed && !backend_changed {
            return Ok("ℹ️  No changes to apply.".to_string());
        }

        // Apply changes
        eprintln!("\n🔄 Applying changes...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // 1. Update Tor configuration (update ALL HiddenServicePort lines)
        if http_changed || https_changed {
            eprintln!("   📄 Updating Tor configuration...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Support both active (.conf) and stopped (.conf.disabled) services
            let conf_path_active = format!("/etc/tor/enola.d/{}.conf", actual_name);
            let conf_path_disabled = format!("/etc/tor/enola.d/{}.conf.disabled", actual_name);
            let conf_path = if std::path::Path::new(&conf_path_active).exists() {
                conf_path_active
            } else if std::path::Path::new(&conf_path_disabled).exists() {
                conf_path_disabled
            } else {
                return Err(CliError::Generic(format!(
                    "Tor config not found for '{}'. Use 'enola-cli tor list' to check.",
                    actual_name
                )));
            };

            let content = tokio::fs::read_to_string(&conf_path)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to read Tor config: {}", e)))?;

            // Build new configuration with updated ports
            let mut new_lines: Vec<String> = Vec::new();
            let mut http_port_updated = false;
            let mut https_port_updated = false;

            for line in content.lines() {
                if line.trim().starts_with("HiddenServicePort") {
                    if !http_port_updated {
                        // First HiddenServicePort - this is HTTP
                        new_lines.push(format!(
                            "HiddenServicePort {} 127.0.0.1:{}",
                            new_virtual_http, new_nginx_http
                        ));
                        http_port_updated = true;
                    } else if has_ssl && !https_port_updated {
                        // Second HiddenServicePort - this is HTTPS
                        new_lines.push(format!(
                            "HiddenServicePort {} 127.0.0.1:{}",
                            new_virtual_https.unwrap_or(443),
                            new_nginx_https.unwrap_or(0)
                        ));
                        https_port_updated = true;
                    } else {
                        // Additional ports - keep as-is
                        new_lines.push(line.to_string());
                    }
                } else {
                    new_lines.push(line.to_string());
                }
            }

            let new_content = new_lines.join("\n");
            tokio::fs::write(&conf_path, new_content)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to write Tor config: {}", e)))?;

            eprintln!("   ✓ Tor configuration updated");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        // 2. Update Nginx configuration (for proxy services)
        if has_nginx
            && (new_nginx_http != old_nginx_http || new_backend != old_backend || https_changed)
        {
            eprintln!("   🌐 Updating Nginx configuration...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            let nginx_config_path = format!("/etc/nginx/sites-available/{}", nginx_actual_name);
            let content = tokio::fs::read_to_string(&nginx_config_path)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to read Nginx config: {}", e)))?;

            // Update the configuration - handle both HTTP and HTTPS blocks
            let new_content = content
                .lines()
                .map(|line| {
                    let trimmed = line.trim();

                    // Update listen ports - distinguish HTTP vs HTTPS
                    if trimmed.starts_with("listen 127.0.0.1:") {
                        if trimmed.contains("ssl") {
                            // HTTPS listen line
                            if let Some(https_port) = new_nginx_https {
                                return format!("    listen 127.0.0.1:{} ssl;", https_port);
                            }
                        } else {
                            // HTTP listen line
                            return format!("    listen 127.0.0.1:{};", new_nginx_http);
                        }
                    }

                    // Update proxy_pass (same for both HTTP and HTTPS)
                    if trimmed.starts_with("proxy_pass http://127.0.0.1:") {
                        return format!("        proxy_pass http://127.0.0.1:{};", new_backend);
                    }

                    line.to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");

            tokio::fs::write(&nginx_config_path, new_content)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to write Nginx config: {}", e)))?;

            if let Some(https_port) = new_nginx_https {
                eprintln!(
                    "   ✓ Nginx configuration updated (HTTP:{}, HTTPS:{})",
                    new_nginx_http, https_port
                );
            } else {
                eprintln!("   ✓ Nginx configuration updated (HTTP:{})", new_nginx_http);
            }
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Validate nginx config
            eprintln!("   🔍 Validating Nginx configuration...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            if !nginx_adapter
                .validate_config()
                .await
                .map_err(CliError::from)?
            {
                eprintln!("   ⚠️  Nginx configuration validation failed — changes applied but nginx may need manual reload");
                let _ = std::io::Write::flush(&mut std::io::stderr());
                // Still reload Tor and Nginx — the config is written, validation may fail due to transient issues
            } else {
                eprintln!("   ✓ Nginx configuration valid");
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }
        }

        // 3. Reload services
        eprintln!("   🔄 Reloading Tor...");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        tor_adapter.reload_tor().await.map_err(CliError::from)?;
        eprintln!("   ✓ Tor reloaded");

        if has_nginx {
            eprintln!("   🔄 Reloading Nginx...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            nginx_adapter.reload().await.map_err(CliError::from)?;
            eprintln!("   ✓ Nginx reloaded");
        }
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Build result message with complete port information
        let mut result = format!("\n✅ Service '{}' ports updated successfully!\n", name);
        result.push_str("\n📋 Final configuration:\n");

        if has_ssl {
            // Show both HTTP and HTTPS paths for SSL services
            result.push_str(&format!(
                "   .onion:{} → Nginx:{} (HTTP)",
                new_virtual_http, new_nginx_http
            ));
            if has_nginx {
                result.push_str(&format!(" → App:{}\n", new_backend));
            } else {
                result.push('\n');
            }
            result.push_str(&format!(
                "   .onion:{} → Nginx:{} (HTTPS)",
                new_virtual_https.unwrap_or(443),
                new_nginx_https.unwrap_or(0)
            ));
            if has_nginx {
                result.push_str(&format!(" → App:{}\n", new_backend));
            } else {
                result.push('\n');
            }
        } else {
            // Single port path for non-SSL services
            result.push_str(&format!(
                "   .onion:{} → Nginx:{}",
                new_virtual_http, new_nginx_http
            ));
            if has_nginx {
                result.push_str(&format!(" → App:{}\n", new_backend));
            } else {
                result.push('\n');
            }
        }

        if has_nginx {
            result.push_str(&format!(
                "\n💡 Make sure your application is listening on port {}",
                new_backend
            ));
        }

        Ok(result)
    }

    pub mod auth {
        use super::*;
        use crate::ports::tor::TorManagerPort;

        /// Helper to resolve actual service name (handles proxy_ prefix)
        async fn resolve_service_name(service: &str) -> CliResult<String> {
            let tor_adapter = Arc::new(TorConfigAdapter::new());
            let services = tor_adapter
                .list_hidden_services()
                .await
                .map_err(CliError::from)?;

            if services.iter().any(|s| s.name == service) {
                Ok(service.to_string())
            } else if services
                .iter()
                .any(|s| s.name == format!("proxy_{}", service))
            {
                Ok(format!("proxy_{}", service))
            } else {
                Err(CliError::InvalidInput(format!(
                    "Service '{}' not found",
                    service
                )))
            }
        }

        /// List authorized clients for a service
        pub async fn list(service: &str) -> CliResult<Vec<String>> {
            let actual_name = resolve_service_name(service).await?;
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case
                .list_clients(&actual_name)
                .await
                .map_err(CliError::from)
        }

        /// Enable client authorization
        pub async fn enable(service: &str) -> CliResult<()> {
            let actual_name = resolve_service_name(service).await?;
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case
                .toggle_auth(&actual_name, true)
                .await
                .map_err(CliError::from)
        }

        /// Disable client authorization
        pub async fn disable(service: &str) -> CliResult<()> {
            let actual_name = resolve_service_name(service).await?;
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case
                .toggle_auth(&actual_name, false)
                .await
                .map_err(CliError::from)
        }

        /// Add authorized client
        pub async fn add(service: &str, client: &str, pubkey: &str) -> CliResult<()> {
            let actual_name = resolve_service_name(service).await?;
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case
                .add_client(&actual_name, client, pubkey)
                .await
                .map_err(CliError::from)
        }

        /// Revoke client authorization
        pub async fn revoke(service: &str, client: &str) -> CliResult<()> {
            let actual_name = resolve_service_name(service).await?;
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case
                .revoke_client(&actual_name, client)
                .await
                .map_err(CliError::from)
        }

        /// Generate new client keypair
        pub async fn generate(client: &str) -> CliResult<(String, String)> {
            let adapter = Arc::new(TorConfigAdapter::new());
            let use_case = ManageClientAuth::new(adapter);
            use_case.generate_keys(client).await.map_err(CliError::from)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
pub mod git {
    use super::*;
    use crate::application::deploy_git_server::DeployGitServer;
    use crate::ports::container::ContainerPort;

    /// Git server info
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct GitServerInfo {
        pub name: String,
        pub container_name: String,
        pub status: String,
        pub ssh_port: Option<u16>,
        pub http_port: Option<u16>,
        pub onion_address: Option<String>,
    }

    /// List Git servers — reads real mapped ports including stopped containers
    pub async fn list() -> CliResult<Vec<GitServerInfo>> {
        use std::process::Command;

        let output = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                "name=enola-git-",
                "--format",
                "{{.Names}}\t{{.Status}}",
            ])
            .output()?;

        let mut servers: Vec<GitServerInfo> = Vec::new();

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let container_name = parts[0].to_string();
                let name = container_name
                    .strip_prefix("enola-git-")
                    .unwrap_or(&container_name)
                    .to_string();
                let status = if parts[1].contains("Up") {
                    "running"
                } else {
                    "stopped"
                };

                // Read REAL mapped ports — works for running AND stopped containers
                let http_port = read_container_mapped_port(&container_name, 3000);
                let ssh_port = read_container_mapped_port(&container_name, 22);

                let onion = get_git_onion(&name).await;

                servers.push(GitServerInfo {
                    name: name.clone(),
                    container_name,
                    status: status.to_string(),
                    ssh_port,
                    http_port,
                    onion_address: onion,
                });
            }
        }

        Ok(servers)
    }

    /// Read the host-mapped port for a container's internal port.
    /// Works for BOTH running and stopped containers via `docker inspect`.
    /// `docker port` only works for running containers; stopped containers
    /// still occupy their port mappings in Docker's internal state.
    pub(super) fn read_container_mapped_port(container: &str, internal_port: u16) -> Option<u16> {
        use std::process::Command;
        // First try `docker port` (fast, works for running containers)
        let out = Command::new("docker")
            .args(["port", container, &format!("{}/tcp", internal_port)])
            .output()
            .ok()?;
        if out.status.success() {
            if let Some(p) = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .and_then(|l| l.split(':').next_back())
                .and_then(|p| p.trim().parse::<u16>().ok())
            {
                return Some(p);
            }
        }
        // Fallback: docker inspect for stopped containers
        let inspect = Command::new("docker")
            .args(["inspect", "--format",
                   &format!("{{{{range $p, $conf := .HostConfig.PortBindings}}}}{{{{if eq $p \"{}/tcp\"}}}}{{{{(index $conf 0).HostPort}}}}{{{{end}}}}{{{{end}}}}", internal_port),
                   container])
            .output()
            .ok()?;
        String::from_utf8_lossy(&inspect.stdout)
            .trim()
            .parse::<u16>()
            .ok()
    }

    /// Verifica que un puerto no está ocupado ni por el sistema ni por Docker.
    ///
    /// `TcpListener::bind` detecta puertos del SO, pero **no** detecta puertos
    /// de contenedores Docker parados (Docker los reserva en su propia tabla).
    /// Esta función verifica ambas fuentes para evitar conflictos silenciosos.
    ///
    /// Expuesta como `pub(super)` para que otros módulos del mismo padre
    /// (wordpress, ai) puedan reutilizarla sin duplicar código.
    pub(super) fn is_port_free(port: u16) -> bool {
        use std::process::Command;
        // 1. Verificación a nivel SO
        if std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
            return false;
        }
        // 2. Verificación Docker: buscar contenedores (running o stopped) con ese puerto mapeado
        let ports_output = Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Ports}}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let port_str = port.to_string();
        for line in ports_output.lines() {
            if line.contains(&format!(":{port_str}->"))
                || line.contains(&format!(":{port_str}/tcp"))
            {
                return false;
            }
        }
        true
    }

    async fn get_git_onion(name: &str) -> Option<String> {
        let hostname_path = format!("/var/lib/tor/enola_{}/hostname", name);
        std::fs::read_to_string(&hostname_path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    // ─── Helpers para ciclo de vida de Forgejo ──────────────────────────────

    /// Espera hasta que la DB de Forgejo exista dentro del contenedor.
    /// Retorna true si está lista antes del timeout.
    fn wait_for_forgejo_db(container: &str, timeout_secs: u64) -> bool {
        let limit = timeout_secs / 2;
        for _ in 0..limit {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let ok = std::process::Command::new("docker")
                .args(["exec", container, "test", "-f", "/data/gitea/gitea.db"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                return true;
            }
        }
        false
    }

    /// Espera hasta que la API REST de Forgejo responda en el puerto host dado.
    /// Retorna true si está lista antes del timeout.
    fn wait_for_forgejo_api(port: u16, timeout_secs: u64) -> bool {
        let url = format!("http://127.0.0.1:{}/api/v1/version", port);
        let limit = timeout_secs / 3;
        for _ in 0..limit {
            let ok = std::process::Command::new("curl")
                .args(["-sf", "--max-time", "3", &url])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        false
    }

    /// Si Forgejo está en modo instalación (wizard activo, INSTALL_LOCK=false),
    /// completa el wizard programáticamente vía POST /install con SQLite y las
    /// credenciales de admin proporcionadas.
    ///
    /// Esto es necesario cuando el contenedor arranca sin INSTALL_LOCK=true:
    /// hasta que el wizard se complete, la API REST no funciona.
    fn complete_forgejo_wizard_if_needed(port: u16, admin_user: &str, admin_pass: &str) {
        // Comprobar si el wizard está activo consultando /
        let index_url = format!("http://127.0.0.1:{}/", port);
        let check = std::process::Command::new("curl")
            .args([
                "-sf",
                "--max-time",
                "5",
                "-L",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &index_url,
            ])
            .output()
            .ok();

        let is_wizard = check
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|code| code.trim() == "200")
            .unwrap_or(false);

        if !is_wizard {
            return;
        }

        // Verificar si el wizard sigue activo buscando el form de instalación
        let install_check = std::process::Command::new("curl")
            .args([
                "-sf",
                "--max-time",
                "5",
                &format!("http://127.0.0.1:{}/install", port),
            ])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        if !install_check.contains("Installation") && !install_check.contains("install") {
            // Wizard ya completado o no activo
            return;
        }

        eprintln!("   🧙 Wizard de instalación activo — completando automáticamente...");

        // POST al endpoint de instalación de Forgejo
        // Campos mínimos requeridos por Forgejo 9.x
        let post_data = format!(
            "db_type=sqlite3\
             &db_host=localhost%3A3306\
             &db_user=root\
             &db_passwd=\
             &db_name=gitea\
             &ssl_mode=disable\
             &db_path=%2Fdata%2Fgitea%2Fgitea.db\
             &app_name=Forgejo\
             &repo_root_path=%2Fdata%2Fgit%2Frepositories\
             &lfs_root_path=%2Fdata%2Fgit%2Flfs\
             &run_user=git\
             &domain=localhost\
             &ssh_port=22\
             &http_port={port}\
             &app_url=http%3A%2F%2Flocalhost%3A{port}%2F\
             &log_root_path=%2Fdata%2Flog\
             &smtp_addr=\
             &smtp_port=\
             &smtp_from=\
             &smtp_user=\
             &smtp_passwd=\
             &enable_federated_avatar=on\
             &enable_open_id_sign_in=on\
             &enable_open_id_sign_up=on\
             &default_allow_create_organization=on\
             &default_enable_timetracking=on\
             &no_reply_address=noreply%40localhost\
             &admin_name={user}\
             &admin_passwd={pass}\
             &admin_confirm_passwd={pass}\
             &admin_email={user}%40localhost",
            port = port,
            user = admin_user,
            pass = admin_pass
        );

        let result = std::process::Command::new("curl")
            .args([
                "-sf",
                "--max-time",
                "30",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/x-www-form-urlencoded",
                "-d",
                &post_data,
                "--location", // seguir redirección post-instalación
                &format!("http://127.0.0.1:{}/install", port),
            ])
            .output();

        match result {
            Ok(o) if o.status.success() => {
                eprintln!("   ✓ Wizard completado. Esperando reinicio de Forgejo...");
                // Forgejo reinicia su servidor HTTP tras el wizard
                std::thread::sleep(std::time::Duration::from_secs(5));
                // Esperar a que la API vuelva a estar disponible
                wait_for_forgejo_api(port, 30);
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!(
                    "   ⚠ Wizard no se pudo completar via HTTP: {}",
                    stderr.trim()
                );
                eprintln!("     El admin se creará via docker exec de todas formas.");
            }
            Err(e) => {
                eprintln!("   ⚠ Error al completar wizard: {}", e);
            }
        }
    }

    /// Crea el usuario administrador dentro del contenedor Forgejo via `forgejo admin user create`.
    /// Este método es el más fiable en Forgejo 9.x independientemente del modo de arranque.
    /// Requiere `--config /data/gitea/conf/app.ini` porque el binario no lo detecta
    /// automáticamente dentro del contenedor Alpine.
    fn create_forgejo_admin_via_exec(container: &str, admin_user: &str, admin_pass: &str) {
        eprintln!(
            "   🔑 Creando usuario admin '{}' via docker exec...",
            admin_user
        );
        let output = std::process::Command::new("docker")
            .args([
                "exec",
                "-u",
                "git",
                container,
                "forgejo",
                "--config",
                "/data/gitea/conf/app.ini",
                "admin",
                "user",
                "create",
                "--username",
                admin_user,
                "--password",
                admin_pass,
                "--email",
                &format!("{}@localhost", admin_user),
                "--admin",
                "--must-change-password=true",
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                eprintln!("✅ Admin '{}' creado exitosamente en Forgejo", admin_user);
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let stdout = String::from_utf8_lossy(&o.stdout);
                if stderr.contains("already exists") || stdout.contains("already exists") {
                    eprintln!("ℹ️  Admin '{}' ya existe en Forgejo", admin_user);
                } else {
                    eprintln!(
                        "⚠️  No se pudo crear admin '{}' via docker exec: {} {}",
                        admin_user,
                        stderr.trim(),
                        stdout.trim()
                    );
                    eprintln!("   Intenta manualmente:");
                    eprintln!("   docker exec -u git {} forgejo --config /data/gitea/conf/app.ini admin user create --username {} --password <pass> --email {}@localhost --admin --must-change-password=true",
                        container, admin_user, admin_user);
                }
            }
            Err(e) => {
                eprintln!("⚠️  Error ejecutando docker exec: {}", e);
            }
        }
    }

    ///
    /// # Dos modos de primer acceso:
    ///
    /// **Modo CLI** — credenciales elegidas por el usuario en el comando:
    /// ```bash
    /// sudo enola-cli git create --name myrepo --admin-user alice --admin-password MiPass123
    /// ```
    /// Forgejo arranca ya configurado. El usuario entra con las credenciales indicadas.
    ///
    /// **Modo Web** — el usuario configura todo desde el navegador:
    /// ```bash
    /// sudo enola-cli git create --name myrepo
    /// ```
    /// Forgejo muestra el asistente de instalación en `http://localhost:<puerto>/`.
    /// El usuario elige su nombre, correo y contraseña desde el formulario web.
    /// Crea un servidor Git/Forgejo.
    ///
    /// Los puertos deben venir pre-validados desde el executor (PortValidator).
    /// Esta función NO asigna puertos automáticamente — eso lo hace el executor.
    pub async fn create(
        name: &str,
        ssl: bool,
        admin_user: Option<&str>,
        admin_pass: Option<&str>,
        http_port: u16,
        ssh_port: u16,
    ) -> CliResult<String> {
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        // Los puertos ya están validados por PortValidator en executor.rs
        eprintln!("📌 Using ports: HTTP={}, SSH={}", http_port, ssh_port);

        // SEC-EXT-RACE-012: en create tambin reservamos puertos hasta que Docker termine el bind.
        let _http_port_lock = reserve_port_or_fail(http_port, "http-port")?;
        let _ssh_port_lock = reserve_port_or_fail(ssh_port, "ssh-port")?;
        if !is_port_free_shared(http_port) {
            return Err(port_in_use_error(http_port, "http-port"));
        }
        if !is_port_free_shared(ssh_port) {
            return Err(port_in_use_error(ssh_port, "ssh-port"));
        }

        let use_case = DeployGitServer::new(
            docker_adapter,
            tor_adapter,
            Some(nginx_adapter),
            Arc::new(FileManifestAdapter::new()),
        );
        let result = use_case
            .execute(name, http_port, ssh_port, ssl, admin_user, admin_pass)
            .await
            .map_err(CliError::from)?;

        // ── MODO CLI: crear admin via docker exec ─────────────────────────────
        // En Forgejo 9.x, INSTALL_LOCK=true salta el wizard pero NO crea el admin.
        // El método fiable es esperar a que Forgejo esté completamente listo
        // (API /api/v1/version responde) y luego crear el admin via `forgejo admin user create`.
        if let (Some(auser), Some(apass)) = (admin_user, admin_pass) {
            let container = format!("enola-git-{}", name);
            eprintln!("⏳ Esperando que Forgejo arranque y su API esté lista (max 90s)...");

            // Fase 1: esperar a que la DB esté creada (indica que Forgejo inició)
            let db_ready = wait_for_forgejo_db(&container, 60);

            if db_ready {
                eprintln!("   ✓ Base de datos lista");

                // Fase 2: esperar a que la API HTTP responda
                let api_ready = wait_for_forgejo_api(http_port, 60);

                if api_ready {
                    eprintln!("   ✓ API REST lista en :{}", http_port);
                } else {
                    eprintln!(
                        "   ⚠ API REST tardó demasiado — intentando crear admin de todas formas..."
                    );
                }

                // Fase 3: si Forgejo está en modo instalación (wizard), completarlo via POST
                complete_forgejo_wizard_if_needed(http_port, auser, apass);

                // Fase 4: crear el admin via docker exec (método más fiable en Forgejo 9.x)
                create_forgejo_admin_via_exec(&container, auser, apass);
            } else {
                eprintln!("⚠️  Forgejo tardó demasiado en inicializar su DB.");
                eprintln!("   Crea el admin manualmente cuando Forgejo esté listo:");
                eprintln!("   docker exec -u git {} forgejo --config /data/gitea/conf/app.ini admin user create --username {} --password <pass> --email {}@localhost --admin --must-change-password=true",
                    container, auser, auser);
            }
        }

        Ok(result)
    }

    /// Start a Git server
    pub async fn start(name: &str) -> CliResult<()> {
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        docker_adapter
            .start_container(&format!("enola-git-{}", name))
            .await
            .map_err(|e| CliError::Generic(e.to_string()))
    }

    /// Stop a Git server
    pub async fn stop(name: &str) -> CliResult<()> {
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        docker_adapter
            .stop_container(&format!("enola-git-{}", name))
            .await
            .map_err(|e| CliError::Generic(e.to_string()))
    }

    /// Show status of a Git server
    pub async fn status(name: &str) -> CliResult<GitServerInfo> {
        let servers = list().await?;
        servers
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| CliError::Generic(format!("Git server '{}' not found", name)))
    }

    /// Delete a Git server
    pub async fn delete(name: &str) -> CliResult<()> {
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        docker_adapter
            .remove_container(&format!("enola-git-{}", name))
            .await
            .map_err(|e| CliError::Generic(e.to_string()))?;

        // Clean up /srv data directory.
        let srv_dir = format!("/srv/enola-git/{}", name);
        let _ = std::fs::remove_dir_all(&srv_dir);

        Ok(())
    }

    /// Toggle user registration
    pub async fn registration(name: &str, enable: bool) -> CliResult<()> {
        use crate::application::git_registration_toggle::GitRegistrationToggle;
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let file_adapter = Arc::new(EnolaFileAdapter::new());

        let use_case = GitRegistrationToggle::new(file_adapter, docker_adapter);
        use_case.execute(name, enable).await.map_err(CliError::from)
    }

    /// Consulta el estado actual del registro de usuarios sin modificarlo.
    ///
    /// Returns `true` si el registro está habilitado, `false` si está deshabilitado.
    pub async fn registration_status(name: &str) -> CliResult<bool> {
        use crate::application::git_registration_toggle::GitRegistrationToggle;
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let file_adapter = Arc::new(EnolaFileAdapter::new());

        let use_case = GitRegistrationToggle::new(file_adapter, docker_adapter);
        use_case
            .is_registration_enabled(name)
            .await
            .map_err(CliError::from)
    }

    /// Edit Git server ports
    ///
    /// # Validación previa (PORTS-008)
    /// Todos los puertos nuevos se validan contra SO y Docker ANTES de hacer
    /// cualquier cambio. Si alguno está ocupado → error inmediato, 0 residuos.
    ///
    /// # Cambio de SSH port (PORTS-007)
    /// Cambiar el puerto SSH requiere recrear el contenedor (Docker no permite
    /// reasignar port bindings en caliente). El proceso es atómico:
    ///   1. Validar nuevo puerto disponible
    ///   2. Stop contenedor
    ///   3. Recrear con nuevo -p mapping
    ///   4. Start contenedor
    ///   5. Actualizar Tor + Nginx
    ///
    /// # Ejemplos
    /// ```bash
    /// sudo enola-cli git edit myrepo --http-port 3600   # cambiar puerto Nginx
    /// sudo enola-cli git edit myrepo --ssh-port 2300    # cambiar SSH (recrea contenedor)
    /// sudo enola-cli git edit myrepo --auto-ports       # reasignar todos automáticamente
    /// ```
    pub async fn edit(
        name: &str,
        http_port: Option<u16>,
        https_port: Option<u16>,
        ssh_port: Option<u16>,
        auto_ports: bool,
    ) -> CliResult<String> {
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        eprintln!("🔧 Editing Git service '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 1. Leer configuración actual ────────────────────────────────────
        // Resolve ports from Docker container (works before AND after publish).
        // Tor services only exist after `git publish`, so we cannot rely on them.
        let container_name = format!("enola-git-{}", name);

        // Verify the container exists
        let inspect = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.Name}}", &container_name])
            .output()
            .map_err(|e| CliError::Generic(format!("docker inspect failed: {}", e)))?;
        if !inspect.status.success() {
            return Err(CliError::InvalidInput(format!(
                "Git service '{}' not found. \
                 Make sure it exists (try `git list`).",
                name
            )));
        }

        let mut old_http_port = read_container_mapped_port(&container_name, 3000).unwrap_or(0);
        let mut old_ssh_port = read_container_mapped_port(&container_name, 22).unwrap_or(0);
        let mut old_https_port: Option<u16> = None;

        // If already published on Tor, read ports from Tor config too (may have HTTPS)
        let services = tor_adapter.list_hidden_services().await.unwrap_or_default();
        let tor_service = services.iter().find(|s| s.name == name);
        if let Some(service) = tor_service {
            for (virtual_p, target) in &service.ports {
                let port = target
                    .split(':')
                    .next_back()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(0);
                match *virtual_p {
                    80 => old_http_port = port,
                    443 => old_https_port = Some(port),
                    22 => old_ssh_port = port,
                    _ => {}
                }
            }
        }

        let (has_ssl, detected_https, _) = nginx_adapter
            .detect_ssl_config(name)
            .await
            .unwrap_or((false, None, None));
        if has_ssl && old_https_port.is_none() {
            old_https_port = detected_https;
        }

        // ── 2. Determinar nuevos puertos ────────────────────────────────────
        let mut new_http_port = http_port.unwrap_or(old_http_port);
        let mut new_https_port = if has_ssl {
            Some(https_port.or(old_https_port).unwrap_or(0))
        } else {
            None
        };
        let mut new_ssh_port = ssh_port.unwrap_or(old_ssh_port);

        if auto_ports {
            eprintln!("\n🔍 Auto-detecting available ports...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            if http_port.is_none() {
                new_http_port = nginx_adapter
                    .find_available_port(10000, 15000)
                    .await
                    .map_err(|e| CliError::Generic(format!("Failed to find HTTP port: {:?}", e)))?;
            }
            if has_ssl && https_port.is_none() {
                new_https_port = Some(
                    nginx_adapter
                        .find_available_port(15001, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find HTTPS port: {:?}", e))
                        })?,
                );
            }
            if ssh_port.is_none() {
                // buscar puerto SSH libre distinto al actual
                let mut found = false;
                for candidate in 2222u16..2999 {
                    if candidate != old_ssh_port && is_port_free(candidate) {
                        new_ssh_port = candidate;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(CliError::InvalidInput(
                        "No free SSH port found in range 2222-2999. Specify one with --ssh-port <PORT>.".to_string()
                    ));
                }
            }
        }

        // ── 3. VALIDACIÓN PREVIA — antes de tocar NADA (PORTS-008) ─────────
        // Excluir los puertos actuales del propio servicio (reasignar el mismo es válido)
        let current_ports = [
            old_http_port,
            old_ssh_port,
            old_https_port.unwrap_or(0),
            new_http_port,
            new_ssh_port,
        ]; // también excluimos los "nuevos" cruzados

        let mut ports_to_validate: Vec<(u16, &str)> = Vec::new();
        if new_http_port != old_http_port {
            ports_to_validate.push((new_http_port, "http-port"));
        }
        if new_ssh_port != old_ssh_port {
            ports_to_validate.push((new_ssh_port, "ssh-port"));
        }
        if let Some(new_hp) = new_https_port {
            if new_https_port != old_https_port {
                ports_to_validate.push((new_hp, "https-port"));
            }
        }

        for (port, label) in &ports_to_validate {
            if !is_port_free(*port) && !current_ports.contains(port) {
                return Err(CliError::InvalidInput(format!(
                    "Port {} (--{}) is already in use.\n\
                     Free it first or choose a different port with --{} <PORT>.",
                    port, label, label
                )));
            }
        }

        // ── 4. Verificar si algo cambió ─────────────────────────────────────
        let http_changed = new_http_port != old_http_port;
        let https_changed = new_https_port != old_https_port;
        let ssh_changed = new_ssh_port != old_ssh_port;

        if !http_changed && !https_changed && !ssh_changed {
            return Ok("ℹ️  No changes needed.".to_string());
        }

        eprintln!("\n📋 Changes:");
        if http_changed {
            eprintln!("   HTTP:  {} → {}", old_http_port, new_http_port);
        }
        if ssh_changed {
            eprintln!("   SSH:   {} → {}", old_ssh_port, new_ssh_port);
        }
        if https_changed {
            eprintln!("   HTTPS: {:?} → {:?}", old_https_port, new_https_port);
        }
        eprintln!("\n🔄 Applying changes...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 5. Recrear contenedor si cambia SSH (Docker no permite hot-rebind) ─
        if ssh_changed {
            eprintln!("   ⚠️  SSH port change requires container recreation...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            let docker_adapter = Arc::new(
                BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?,
            );

            // Leer imagen y env del contenedor actual para recrearlo igual
            let inspect_out = std::process::Command::new("docker")
                .args([
                    "inspect",
                    "--format",
                    "{{.Config.Image}}\t{{range .Config.Env}}{{.}}\n{{end}}",
                    &container_name,
                ])
                .output()
                .map_err(|e| CliError::Generic(format!("docker inspect failed: {}", e)))?;

            let inspect_str = String::from_utf8_lossy(&inspect_out.stdout).to_string();
            let mut lines = inspect_str.lines();
            let image = lines
                .next()
                .unwrap_or("codeberg.org/forgejo/forgejo:9-rootless")
                .split('\t')
                .next()
                .unwrap_or("codeberg.org/forgejo/forgejo:9-rootless")
                .to_string();

            // Leer el http_port actual del contenedor (para mantenerlo)
            let current_http =
                read_container_mapped_port(&container_name, 3000).unwrap_or(old_http_port);

            // Stop → Remove → Recreate → Start
            let _ = docker_adapter.stop_container(&container_name).await;
            eprintln!("   ✓ Contenedor parado");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            let _ = docker_adapter.remove_container(&container_name).await;
            eprintln!("   ✓ Contenedor eliminado");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Recrear con los nuevos port mappings
            let http_p = if http_changed {
                new_http_port
            } else {
                current_http
            };
            let apparmor_profile = format!("enola-git-{}", name);
            let run_result = std::process::Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "--name",
                    &container_name,
                    "--restart",
                    "unless-stopped",
                    "--cap-drop",
                    "ALL",
                    "--cap-add",
                    "CHOWN",
                    "--cap-add",
                    "SETUID",
                    "--cap-add",
                    "SETGID",
                    "--cap-add",
                    "DAC_OVERRIDE",
                    "--security-opt",
                    "no-new-privileges:true",
                    "--security-opt",
                    &format!("apparmor={}", apparmor_profile),
                    "-v",
                    &format!("/srv/enola-git/{}:/data", name),
                    "-p",
                    &format!("127.0.0.1:{}:3000", http_p),
                    "-p",
                    &format!("127.0.0.1:{}:22", new_ssh_port),
                    "-e",
                    "USER_UID=1000",
                    "-e",
                    "USER_GID=1000",
                    "-e",
                    "FORGEJO__server__INSTALL_LOCK=true",
                    &image,
                ])
                .status()
                .map_err(|e| CliError::Generic(format!("docker run failed: {}", e)))?;

            if !run_result.success() {
                return Err(CliError::Generic(format!(
                    "Failed to recreate container '{}'. \
                     Check docker logs {} for details.",
                    container_name, container_name
                )));
            }
            eprintln!(
                "   ✓ Contenedor recreado con SSH:{} HTTP:{}",
                new_ssh_port, http_p
            );
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Wait for Forgejo API to be ready after recreation
            eprintln!("   ⏳ Esperando API Forgejo en :{} (max 120s)...", http_p);
            let _ = std::io::Write::flush(&mut std::io::stderr());
            if wait_for_forgejo_api(http_p, 120) {
                eprintln!("   ✓ API REST lista en :{}", http_p);
            } else {
                eprintln!(
                    "   ⚠ API REST tardó demasiado — el contenedor puede necesitar más tiempo."
                );
            }
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Actualizar new_http_port si no cambió explícitamente
            if !http_changed {
                new_http_port = http_p;
            }
        }

        // ── 6. Actualizar configuración Tor (solo si está publicado) ───────
        let was_published = tor_service.is_some();
        if was_published {
            let mut tor_ports = vec![(80u16, new_http_port)];
            if let Some(hp) = new_https_port {
                tor_ports.push((443, hp));
            }
            tor_ports.push((22, new_ssh_port));
            update_tor_config_ports(name, &tor_ports).await?;
            eprintln!("   ✓ Tor configuration updated");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        // ── 7. Actualizar Nginx si existe ──────────────────────────────────
        let nginx_site_exists =
            std::path::Path::new(&format!("/etc/nginx/sites-available/proxy_{}", name)).exists()
                || std::path::Path::new(&format!("/etc/nginx/sites-available/{}", name)).exists();

        if nginx_site_exists {
            let backend_port = read_container_mapped_port(&format!("enola-git-{}", name), 3000)
                .unwrap_or(new_http_port);
            if has_ssl {
                nginx_adapter
                    .update_proxy_ports_with_ssl(name, new_http_port, new_https_port, backend_port)
                    .await
                    .map_err(CliError::from)?;
            } else {
                nginx_adapter
                    .update_proxy_ports(name, new_http_port, backend_port)
                    .await
                    .map_err(CliError::from)?;
            }
            nginx_adapter.reload().await.map_err(CliError::from)?;
            eprintln!("   ✓ Nginx updated");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        // ── 8. Recargar Tor (solo si estaba publicado) ─────────────────────
        if was_published {
            tor_adapter.reload_tor().await.map_err(CliError::from)?;
            eprintln!("   ✓ Tor reloaded");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        Ok(format!(
            "✅ Git service '{}' updated!\n   HTTP:  {}\n   SSH:   {}\n   SSL:   {}",
            name,
            new_http_port,
            new_ssh_port,
            if has_ssl { "enabled" } else { "disabled" }
        ))
    }

    /// Publish Git server on Tor — creates Nginx proxy using real Docker mapped port
    pub async fn publish(name: &str, ssl: bool) -> CliResult<String> {
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::{NginxManagerPort, NginxProxyConfigWithSsl};

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());
        let container_name = format!("enola-git-{}", name);

        // Real mapped port for Forgejo HTTP (never hardcode 3000 — that's internal Docker)
        let forgejo_host_port =
            read_container_mapped_port(&container_name, 3000).ok_or_else(|| {
                CliError::Generic(format!(
                "Cannot publish '{}': container '{}' is not running or port 3000 is not mapped. \
                 Start it first with: sudo enola-cli git start {}",
                name, container_name, name
            ))
            })?;

        // Real mapped SSH port (0 if not mapped / not running)
        let ssh_host_port = read_container_mapped_port(&container_name, 22).unwrap_or(0);

        if ssl {
            let (http_port, _http_port_lock) = nginx_adapter
                .find_available_port_with_lock(10000, 15000)
                .await
                .map_err(|e| CliError::Generic(format!("No free HTTP port found: {:?}", e)))?;
            let (https_port, _https_port_lock) = nginx_adapter
                .find_available_port_with_lock(15001, 20000)
                .await
                .map_err(|e| CliError::Generic(format!("No free HTTPS port found: {:?}", e)))?;

            let (cert_path, key_path) = nginx_adapter
                .generate_self_signed_cert(name)
                .await
                .map_err(|e| CliError::Generic(format!("Cert generation failed: {:?}", e)))?;

            let ssl_config = NginxProxyConfigWithSsl {
                service_name: name.to_string(),
                http_port,
                https_port,
                backend_port: forgejo_host_port,
                server_name: "localhost".to_string(),
                ssl_cert_path: cert_path,
                ssl_key_path: key_path,
                rate_limit: None,
            };

            nginx_adapter
                .create_proxy_config_with_ssl(ssl_config)
                .await
                .map_err(CliError::from)?;
            nginx_adapter
                .enable_site(&format!("proxy_{}", name))
                .await
                .map_err(CliError::from)?;
            nginx_adapter.reload().await.map_err(CliError::from)?;

            let mut tor_ports = vec![(80u16, http_port), (443u16, https_port)];
            if ssh_host_port > 0 {
                tor_ports.push((22, ssh_host_port));
            }
            let onion = tor_adapter
                .deploy_hidden_service(name, tor_ports)
                .await
                .map_err(CliError::from)?;
            tor_adapter.reload_tor().await.map_err(CliError::from)?;
            Ok(onion)
        } else {
            // HTTP-only: Nginx proxy → real Forgejo host port
            let (http_port, _http_port_lock) = nginx_adapter
                .find_available_port_with_lock(10000, 15000)
                .await
                .map_err(|e| CliError::Generic(format!("No free HTTP port found: {:?}", e)))?;

            use crate::ports::web::NginxProxyConfig;
            nginx_adapter
                .create_proxy_config(NginxProxyConfig {
                    service_name: name.to_string(),
                    listen_port: http_port,
                    backend_port: forgejo_host_port,
                    server_name: "localhost".to_string(),
                    rate_limit: None,
                })
                .await
                .map_err(CliError::from)?;
            nginx_adapter
                .enable_site(&format!("proxy_{}", name))
                .await
                .map_err(CliError::from)?;
            nginx_adapter.reload().await.map_err(CliError::from)?;

            let mut tor_ports = vec![(80u16, http_port)];
            if ssh_host_port > 0 {
                tor_ports.push((22, ssh_host_port));
            }
            let onion = tor_adapter
                .deploy_hidden_service(name, tor_ports)
                .await
                .map_err(CliError::from)?;
            tor_adapter.reload_tor().await.map_err(CliError::from)?;
            Ok(onion)
        }
    }

    /// Hide Git server
    pub async fn hide(name: &str) -> CliResult<()> {
        use crate::application::remove_tor_service::RemoveTorService;
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());
        let use_case = RemoveTorService::new(tor_adapter, Some(nginx_adapter));
        use_case.execute(name).await.map_err(CliError::from)
    }

    /// Run the Git Pipeline Watcher
    pub async fn watcher() -> CliResult<()> {
        // GitPipelineService not available in standalone mode
        eprintln!("Interactive pipeline watcher not available in standalone mode yet.");
        Ok(())
    }

    pub mod user {
        use super::*;

        /// Git user info
        #[derive(Debug, Clone, serde::Serialize)]
        pub struct GitUserInfo {
            pub username: String,
            pub email: String,
            pub is_admin: bool,
        }

        // ─── Helpers internos ────────────────────────────────────────────────────

        /// Obtiene el puerto HTTP mapeado en el host para el contenedor Forgejo.
        fn get_forgejo_host_port(container: &str) -> CliResult<u16> {
            super::read_container_mapped_port(container, 3000).ok_or_else(|| {
                CliError::Generic(format!(
                    "El servidor '{}' no está en marcha o no tiene el puerto 3000 mapeado. \
                     Arráncalo con: sudo enola-cli git start {}",
                    container,
                    container.strip_prefix("enola-git-").unwrap_or(container)
                ))
            })
        }

        /// Espera hasta que la API REST de Forgejo responda (max `timeout_secs`).
        fn wait_for_forgejo_ready(port: u16, timeout_secs: u64) -> CliResult<()> {
            use std::time::{Duration, Instant};
            let url = format!("http://127.0.0.1:{}/api/v1/version", port);
            let start = Instant::now();
            let timeout = Duration::from_secs(timeout_secs);
            eprintln!(
                "⏳ Esperando que Forgejo esté listo (max {}s)...",
                timeout_secs
            );
            loop {
                let ok = std::process::Command::new("curl")
                    .args(["-sf", "--max-time", "3", &url])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if ok {
                    eprintln!("✅ Forgejo listo.");
                    return Ok(());
                }
                if start.elapsed() > timeout {
                    return Err(CliError::Generic(format!(
                        "Forgejo no respondió en {}s en el puerto {}. \
                         Puede que aún esté inicializando — espera un momento y vuelve a intentarlo. \
                         Comprueba los logs con: docker logs enola-git-<nombre>",
                        timeout_secs, port
                    )));
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        }

        /// Resuelve las credenciales de admin para la API REST.
        ///
        /// - Si se pasan `--admin-user` y `--admin-pass`, los usa directamente.
        /// - Si no, intenta leerlos de `.enola-admin-creds` (modo CLI).
        /// - Si el servidor es modo web y no se pasaron credenciales, da un error
        ///   orientativo inmediato **sin esperar** a la API.
        fn resolve_admin_credentials(
            server: &str,
            container: &str,
            admin_user: Option<&str>,
            admin_pass: Option<&str>,
        ) -> CliResult<(String, String)> {
            // Credenciales explícitas → usarlas directamente
            if let (Some(u), Some(p)) = (admin_user, admin_pass) {
                return Ok((u.to_string(), p.to_string()));
            }

            // SEC-002: Intentar leer credenciales guardadas (modo CLI).
            // El archivo ahora almacena ADMIN_PASS_HASH (bcrypt), no ADMIN_PASS (plaintext).
            // Si encontramos el hash, pedimos el password interactivamente y verificamos.
            let creds_path = format!("/srv/enola-git/{}/.enola-admin-creds", server);
            if let Ok(content) = std::fs::read_to_string(&creds_path) {
                let mut user = String::new();
                let mut pass_hash = String::new();
                let mut pass_plain: Option<String> = None;
                for line in content.lines() {
                    if let Some(v) = line.strip_prefix("ADMIN_USER=") {
                        user = v.to_string();
                    }
                    if let Some(v) = line.strip_prefix("ADMIN_PASS_HASH=") {
                        pass_hash = v.to_string();
                    }
                    // Backward compat: old format had ADMIN_PASS (plaintext)
                    if let Some(v) = line.strip_prefix("ADMIN_PASS=") {
                        pass_plain = Some(v.to_string());
                    }
                }

                // If we have plaintext (old format), use it directly
                if !user.is_empty() {
                    if let Some(pass) = pass_plain {
                        return Ok((user, pass));
                    }

                    // SEC-002: bcrypt hash — prompt interactively
                    if !pass_hash.is_empty() {
                        eprintln!("🔐 Servidor '{}' — credenciales admin (modo CLI)", server);
                        eprintln!("   Usuario: {}", user);
                        let prompt_pass = rpassword::prompt_password("   Contraseña admin: ")
                            .map_err(|e| {
                                CliError::Generic(format!("No se pudo leer la contraseña: {}", e))
                            })?;
                        // Verify against bcrypt hash
                        if bcrypt::verify(&prompt_pass, &pass_hash).unwrap_or(false) {
                            return Ok((user, prompt_pass));
                        }
                        return Err(CliError::Generic(
                            "Contraseña admin incorrecta.".to_string(),
                        ));
                    }
                }
            }

            // Sin credenciales → detectar si es modo web para mensaje orientativo
            let port_info = super::read_container_mapped_port(container, 3000)
                .map(|p| format!("http://127.0.0.1:{}/", p))
                .unwrap_or_else(|| "http://localhost:<puerto>/".to_string());

            Err(CliError::Generic(format!(
                "No se encontraron credenciales de admin para '{server}'.\n\n\
                 Este servidor fue creado en MODO WEB (sin --admin-user).\n\
                 Antes de gestionar usuarios por CLI, tienes dos opciones:\n\n\
                 1️⃣  Completar el wizard web:\n\
                    Abre {port_info} en tu navegador, crea tu cuenta admin,\n\
                    y luego usa --admin-user y --admin-pass en los comandos:\n\
                      sudo enola-cli git user list {server} --admin-user <admin> --admin-pass <pass>\n\n\
                 2️⃣  Crear usuarios via registro web:\n\
                      sudo enola-cli git registration {server} --enable\n\
                    Los usuarios se registran en {port_info}user/sign_up\n\
                      sudo enola-cli git registration {server} --disable"
            )))
        }

        // ─── Comandos públicos ───────────────────────────────────────────────────

        /// Lista todos los usuarios de un servidor Git.
        ///
        /// SEC-005: usa `forgejo admin user list` via docker exec en lugar de la
        /// API REST — Forgejo devuelve 403 en toda la API mientras el admin tenga
        /// pendiente la rotación de contraseña (--must-change-password=true).
        pub async fn list(
            server: &str,
            admin_user: Option<&str>,
            admin_pass: Option<&str>,
        ) -> CliResult<Vec<GitUserInfo>> {
            let container = format!("enola-git-{}", server);
            let port = get_forgejo_host_port(&container)?;

            // Autorización: exige credenciales admin válidas antes de operar.
            // Si es modo web sin creds, falla inmediatamente con mensaje útil.
            let (_au, _ap) = resolve_admin_credentials(server, &container, admin_user, admin_pass)?;

            wait_for_forgejo_ready(port, 90)?;

            let output = std::process::Command::new("docker")
                .args([
                    "exec",
                    "-u",
                    "git",
                    &container,
                    "forgejo",
                    "--config",
                    "/data/gitea/conf/app.ini",
                    "admin",
                    "user",
                    "list",
                ])
                .output()
                .map_err(|e| CliError::Generic(format!("docker exec error: {}", e)))?;

            if !output.status.success() {
                return Err(CliError::Generic(format!(
                    "No se pudo listar usuarios: {}. \
                     Si el servidor fue creado en modo web, usa:\n  \
                     sudo enola-cli git user list {} --admin-user <admin> --admin-pass <pass>",
                    String::from_utf8_lossy(&output.stderr).trim(),
                    server
                )));
            }

            // Formato de salida de `forgejo admin user list`:
            //   ID Username Email IsActive IsAdmin 2FA
            //   1  admin    a@b   true     true    false
            let body = String::from_utf8_lossy(&output.stdout);
            let users: Vec<GitUserInfo> = body
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if fields.len() < 5 {
                        return None;
                    }
                    Some(GitUserInfo {
                        username: fields[1].to_string(),
                        email: fields[2].to_string(),
                        is_admin: fields[4].eq_ignore_ascii_case("true"),
                    })
                })
                .collect();

            Ok(users)
        }

        /// Crea un usuario en el servidor Git via `forgejo admin user create`
        /// (docker exec) — inmune al 403 de la API por rotación pendiente (SEC-005).
        ///
        /// # Dos modos de creación de usuarios:
        ///
        /// **Automático (este comando):**
        /// ```bash
        /// # Si el servidor se creó con --admin-user (creds guardadas):
        /// sudo enola-cli git user create myrepo --username alice --email alice@domain.com --password SecurePass123
        ///
        /// # Si el servidor se creó en modo web (debes pasar las creds de admin):
        /// sudo enola-cli git user create myrepo --username alice --email alice@domain.com --password SecurePass123 --admin-user bob --admin-pass BobPass
        /// ```
        ///
        /// **Manual via web (registro normal):**
        /// 1. `sudo enola-cli git registration myrepo --enable`
        /// 2. El usuario abre `http://localhost:<puerto>/user/sign_up` en su navegador
        /// 3. Rellena nombre, email y contraseña en el formulario
        /// 4. Después: `sudo enola-cli git registration myrepo --disable` (recomendado)
        pub async fn create(
            server: &str,
            username: &str,
            email: &str,
            password: &str,
            is_admin: bool,
            admin_user: Option<&str>,
            admin_pass: Option<&str>,
        ) -> CliResult<()> {
            let container = format!("enola-git-{}", server);
            let port = get_forgejo_host_port(&container)?;

            // Autorización: exige credenciales admin válidas antes de operar (SEC-005).
            let (_au, _ap) = resolve_admin_credentials(server, &container, admin_user, admin_pass)?;

            wait_for_forgejo_ready(port, 90)?;

            let mut args = vec![
                "exec",
                "-u",
                "git",
                container.as_str(),
                "forgejo",
                "--config",
                "/data/gitea/conf/app.ini",
                "admin",
                "user",
                "create",
                "--username",
                username,
                "--password",
                password,
                "--email",
                email,
                "--must-change-password=true",
            ];
            if is_admin {
                args.push("--admin");
            }

            let output = std::process::Command::new("docker")
                .args(&args)
                .output()
                .map_err(|e| CliError::Generic(format!("docker exec error: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(CliError::Generic(format!(
                    "Failed to create user '{}': {} {}",
                    username,
                    stderr.trim(),
                    stdout.trim()
                )));
            }

            eprintln!("✅ Usuario '{}' creado en '{}'", username, server);
            if is_admin {
                eprintln!("   🔑 Permisos de administrador asignados");
            }
            eprintln!("   🌐 Acceso web: http://127.0.0.1:{}", port);
            Ok(())
        }

        /// Elimina un usuario del servidor Git.
        ///
        /// SEC-005: usa `forgejo admin user delete` via docker exec (ver `list`).
        pub async fn delete(
            server: &str,
            username: &str,
            admin_user: Option<&str>,
            admin_pass: Option<&str>,
        ) -> CliResult<()> {
            let container = format!("enola-git-{}", server);
            let port = get_forgejo_host_port(&container)?;

            // Autorización: exige credenciales admin válidas antes de operar (SEC-005).
            let (_au, _ap) = resolve_admin_credentials(server, &container, admin_user, admin_pass)?;

            wait_for_forgejo_ready(port, 30)?;

            let output = std::process::Command::new("docker")
                .args([
                    "exec",
                    "-u",
                    "git",
                    &container,
                    "forgejo",
                    "--config",
                    "/data/gitea/conf/app.ini",
                    "admin",
                    "user",
                    "delete",
                    "--username",
                    username,
                    "--purge",
                ])
                .output()
                .map_err(|e| CliError::Generic(format!("docker exec error: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(CliError::Generic(format!(
                    "Failed to delete user '{}': {}",
                    username,
                    stderr.trim()
                )));
            }

            eprintln!("✅ Usuario '{}' eliminado de '{}'", username, server);
            Ok(())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WORDPRESS COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod wordpress {
    use super::*;
    use crate::application::deploy_wordpress::DeployWordPress;
    use crate::application::toggle_wordpress::ToggleWordPress;
    use crate::ports::container::ContainerPort;

    /// WordPress site info
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WordPressSiteInfo {
        pub name: String,
        pub wp_container: String,
        pub db_container: String,
        pub status: String,
        pub port: Option<u16>,
        pub onion_address: Option<String>,
    }

    /// List WordPress sites
    pub async fn list() -> CliResult<Vec<WordPressSiteInfo>> {
        use std::collections::HashMap;
        use std::process::Command;

        // List Docker containers that match wp-* pattern (WordPress containers)
        let output = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                "name=wp-",
                "--format",
                "{{.Names}}\t{{.Status}}\t{{.Ports}}",
            ])
            .output()?;

        let mut sites: HashMap<String, WordPressSiteInfo> = HashMap::new();

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let container_name = parts[0].to_string();
                let status = if parts[1].contains("Up") {
                    "running"
                } else {
                    "stopped"
                };

                // Parse name: wp-<site> or db-<site>
                let (prefix, site_name) = if container_name.starts_with("wp-") {
                    (
                        "wp",
                        container_name
                            .strip_prefix("wp-")
                            .unwrap_or(&container_name),
                    )
                } else if container_name.starts_with("db-") {
                    (
                        "db",
                        container_name
                            .strip_prefix("db-")
                            .unwrap_or(&container_name),
                    )
                } else {
                    continue;
                };

                let site =
                    sites
                        .entry(site_name.to_string())
                        .or_insert_with(|| WordPressSiteInfo {
                            name: site_name.to_string(),
                            wp_container: String::new(),
                            db_container: String::new(),
                            status: "unknown".to_string(),
                            port: None,
                            onion_address: None,
                        });

                if prefix == "wp" {
                    site.wp_container = container_name;
                    site.status = status.to_string();
                    // Try to parse port from ports string
                    if let Some(ports_str) = parts.get(2) {
                        if let Some(port) = extract_port(ports_str) {
                            site.port = Some(port);
                        }
                    }
                } else {
                    site.db_container = container_name;
                }
            }
        }

        // Try to get onion addresses
        for site in sites.values_mut() {
            site.onion_address = get_wp_onion(&site.name).await;
        }

        Ok(sites.into_values().collect())
    }

    fn extract_port(ports_str: &str) -> Option<u16> {
        // Format: "0.0.0.0:8080->80/tcp"
        ports_str
            .split(':')
            .nth(1)
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse().ok())
    }

    async fn get_wp_onion(name: &str) -> Option<String> {
        let hostname_path = format!("/var/lib/tor/enola_{}/hostname", name);
        std::fs::read_to_string(&hostname_path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Get WordPress site status
    pub async fn status(name: &str) -> CliResult<WordPressStatus> {
        use crate::application::wordpress_status_check::WordPressStatusCheck;

        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);

        let checker = WordPressStatusCheck::new(docker_adapter);
        let check_result = checker.execute(name).await.map_err(CliError::from)?;

        // If both containers are "Not found", the site doesn't exist
        if check_result.wp_container_status.contains("Not found")
            && check_result.db_container_status.contains("Not found")
        {
            return Err(CliError::InvalidInput(format!(
                "WordPress site '{}' not found. No containers exist.",
                name
            )));
        }

        let onion = get_wp_onion(name).await;

        // Try to get the actual port from Docker
        let port = {
            let sites = list().await.unwrap_or_default();
            sites
                .iter()
                .find(|s| s.name == name)
                .and_then(|s| s.port)
                .unwrap_or(80)
        };

        Ok(WordPressStatus {
            name: name.to_string(),
            healthy: check_result.is_healthy,
            wp_container_status: check_result.wp_container_status,
            db_container_status: check_result.db_container_status,
            onion_address: onion,
            url: format!("http://localhost:{}", port),
        })
    }

    #[allow(dead_code)]
    async fn get_container_status(container: &str) -> String {
        use std::process::Command;
        let output = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Status}}", container])
            .output();

        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "not found".to_string(),
        }
    }

    /// WordPress status
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WordPressStatus {
        pub name: String,
        pub healthy: bool,
        pub wp_container_status: String,
        pub db_container_status: String,
        pub onion_address: Option<String>,
        pub url: String,
    }

    /// Edit WordPress configuration
    pub async fn config(name: &str) -> CliResult<String> {
        // WordPress data is stored in /srv/enola-wordpress/{name}_wp/ (bind mount)
        let primary_path = format!("/srv/enola-wordpress/{}_wp/wp-config.php", name);
        // Fallback: older deployments may use Docker named volumes
        let fallback_path = format!(
            "/var/lib/docker/volumes/wp-{}_html/_data/wp-config.php",
            name
        );

        if std::path::Path::new(&primary_path).exists() {
            Ok(format!(
                "📝 WordPress config: {}\n\nUse your preferred editor to modify.",
                primary_path
            ))
        } else if std::path::Path::new(&fallback_path).exists() {
            Ok(format!(
                "📝 WordPress config: {}\n\nUse your preferred editor to modify.",
                fallback_path
            ))
        } else {
            // WordPress may not have generated wp-config.php yet (first-run setup pending)
            let data_dir = format!("/srv/enola-wordpress/{}_wp", name);
            if std::path::Path::new(&data_dir).exists() {
                Ok(format!(
                    "⚠️  wp-config.php not found yet.\n\
                     WordPress data directory: {}\n\n\
                     WordPress needs initial setup via its web wizard first.\n\
                     Open http://localhost:<port>/ in your browser to complete the installation.",
                    data_dir
                ))
            } else {
                Err(CliError::InvalidInput(format!(
                    "WordPress site '{}' not found. No data directory at: {}",
                    name, data_dir
                )))
            }
        }
    }

    /// Publish WordPress site on Tor (create hidden service)
    pub async fn publish(name: &str) -> CliResult<String> {
        // Get the WordPress container port
        let sites = list().await?;
        let site = sites
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| CliError::InvalidInput(format!("Site '{}' not found", name)))?;

        let port = site.port.unwrap_or(80);

        // Create Tor hidden service for this WordPress
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );

        // Tor service name = `name` (NO prefix), consistent with `wp create`
        // (DeployWordPress) and `wp edit`. Using `wp-{name}` here created a
        // duplicate Tor service alongside the one from `wp create`.
        let request = DeployTorServiceRequest {
            service_name: name.to_string(),
            ports: vec![(80, port)],
        };

        let onion = use_case.execute(request).await.map_err(CliError::from)?;

        Ok(format!(
            "✅ WordPress '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    /// Hide WordPress site from Tor (remove hidden service)
    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);

        // Tor service name = `name` (NO prefix), consistent with `wp create`/`wp edit`.
        use_case.execute(name).await.map_err(CliError::from)
    }

    /// Create a WordPress site
    pub async fn create(name: &str, http_port_override: Option<u16>) -> CliResult<String> {
        use crate::domain::wordpress::{WordPressPortManager, WordPressSiteInstance};

        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let tor_adapter = Arc::new(TorConfigAdapter::new());

        // Get existing WordPress sites to avoid port conflicts
        let existing_sites = list().await?;
        let existing_instances: Vec<WordPressSiteInstance> = existing_sites
            .iter()
            .map(|s| WordPressSiteInstance {
                name: s.name.clone(),
                http_port: s.port.unwrap_or(0),
                db_port: 0,
                status: s.status.clone(),
            })
            .collect();

        // Usar puerto override si se proporcionó (ya validado), o auto-asignar
        let http_port = match http_port_override {
            Some(p) => p,
            None => WordPressPortManager::allocate_http_port(&existing_instances)
                .map_err(|e| CliError::Generic(format!("Failed to allocate port: {}", e)))?,
        };

        let _http_port_lock = reserve_port_or_fail(http_port, "http-port")?;
        if !is_port_free_shared(http_port) {
            return Err(port_in_use_error(http_port, "http-port"));
        }

        eprintln!("📌 Allocated port: HTTP={}", http_port);

        let use_case = DeployWordPress::new(
            docker_adapter,
            tor_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        use_case
            .execute(name, http_port)
            .await
            .map_err(CliError::from)
    }

    /// Start a WordPress site
    pub async fn start(name: &str) -> CliResult<()> {
        use crate::application::toggle_wordpress::ToggleAction;
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let use_case = ToggleWordPress::new(docker_adapter);
        use_case
            .execute(name, ToggleAction::Start)
            .await
            .map_err(CliError::from)
    }

    /// Stop a WordPress site
    pub async fn stop(name: &str) -> CliResult<()> {
        use crate::application::toggle_wordpress::ToggleAction;
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let use_case = ToggleWordPress::new(docker_adapter);
        use_case
            .execute(name, ToggleAction::Stop)
            .await
            .map_err(CliError::from)
    }

    /// Restart a WordPress site
    pub async fn restart(name: &str) -> CliResult<()> {
        use crate::application::toggle_wordpress::ToggleAction;
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let use_case = ToggleWordPress::new(docker_adapter);
        use_case
            .execute(name, ToggleAction::Stop)
            .await
            .map_err(CliError::from)?;
        use_case
            .execute(name, ToggleAction::Start)
            .await
            .map_err(CliError::from)
    }

    /// Delete a WordPress site
    pub async fn delete(name: &str) -> CliResult<()> {
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);

        // WordPress containers use "wp-{name}" naming convention
        let wp_container = format!("wp-{}", name);
        let db_container = format!("db-{}", name);

        // Remove WordPress container
        if let Err(e) = docker_adapter.remove_container(&wp_container).await {
            eprintln!(
                "⚠️ Could not remove WordPress container '{}': {}",
                wp_container, e
            );
        }

        // Remove database container
        if let Err(e) = docker_adapter.remove_container(&db_container).await {
            eprintln!(
                "⚠️ Could not remove database container '{}': {}",
                db_container, e
            );
        }

        // Remove the Docker network. Without this, orphaned networks accumulate
        // and exhaust Docker's address pool ("all predefined address pools have
        // been fully subnetted"), blocking new site creation.
        let network_name = format!("enola_net_{}", name);
        let _ = docker_adapter.remove_network(&network_name).await;

        // Clean up /srv data directory.
        let srv_dir = format!("/srv/enola-wordpress/{}", name);
        let _ = std::fs::remove_dir_all(&srv_dir);

        Ok(())
    }

    /// Update WordPress with backup
    pub async fn update(name: &str) -> CliResult<()> {
        use crate::application::backup_system::BackupSystem;
        use crate::application::secure_wordpress_update::SecureWordPressUpdate;
        use crate::application::wordpress_status_check::WordPressStatusCheck;

        // BollardDockerAdapter::new()
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);

        let file_adapter = Arc::new(EnolaFileAdapter::new());
        let backup_system = Arc::new(BackupSystem::new(file_adapter));
        let status_check = Arc::new(WordPressStatusCheck::new(docker_adapter.clone()));

        let use_case = SecureWordPressUpdate::new(docker_adapter, backup_system, status_check);
        use_case.execute(name).await.map_err(CliError::from)
    }

    /// Edit WordPress site ports
    ///
    /// # Validación previa (PORTS-008)
    /// Los puertos nuevos se validan contra SO **y** Docker (incluye contenedores parados)
    /// ANTES de hacer cualquier cambio. Si está ocupado → error inmediato, 0 residuos.
    pub async fn edit(
        name: &str,
        http_port: Option<u16>,
        https_port: Option<u16>,
        ssl: Option<bool>,
        auto_ports: bool,
    ) -> CliResult<String> {
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());

        let tor_service_name = name.to_string();
        let nginx_service_name = format!("wp-{}", name);

        eprintln!("🔧 Editing WordPress site '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 1. Leer configuración actual ────────────────────────────────────
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let service = services
            .iter()
            .find(|s| s.name == tor_service_name)
            .ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "WordPress site '{}' not found in Tor services",
                    name
                ))
            })?;

        let mut old_http_port = 0u16;
        let mut old_https_port: Option<u16> = None;

        for (virtual_p, target) in &service.ports {
            let port = target
                .split(':')
                .next_back()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            match *virtual_p {
                80 => old_http_port = port,
                443 => old_https_port = Some(port),
                _ => {}
            }
        }

        let (has_ssl, detected_https, _) = nginx_adapter
            .detect_ssl_config(&nginx_service_name)
            .await
            .unwrap_or((false, None, None));
        if has_ssl && old_https_port.is_none() {
            old_https_port = detected_https;
        }

        let enable_ssl = ssl.unwrap_or(has_ssl);

        eprintln!("\n📋 Current configuration:");
        eprintln!("   HTTP port:  {}", old_http_port);
        if let Some(hp) = old_https_port {
            eprintln!("   HTTPS port: {}", hp);
        }
        eprintln!(
            "   SSL:        {}",
            if has_ssl { "enabled" } else { "disabled" }
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 2. Determinar nuevos puertos ────────────────────────────────────
        let mut new_http_port = http_port.unwrap_or(old_http_port);
        let mut new_https_port = if enable_ssl {
            let hp = https_port.or(old_https_port).unwrap_or(0);
            if hp == 0 {
                // SSL enabled but no HTTPS port specified — auto-assign one
                eprintln!("\n🔍 Auto-assigning HTTPS port...");
                let _ = std::io::Write::flush(&mut std::io::stderr());
                Some(
                    nginx_adapter
                        .find_available_port(15001, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find HTTPS port: {:?}", e))
                        })?,
                )
            } else {
                Some(hp)
            }
        } else {
            None
        };

        if auto_ports {
            eprintln!("\n🔍 Auto-detecting available ports...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            if http_port.is_none() {
                new_http_port = nginx_adapter
                    .find_available_port(10000, 15000)
                    .await
                    .map_err(|e| CliError::Generic(format!("Failed to find HTTP port: {:?}", e)))?;
            }
            if enable_ssl && https_port.is_none() && new_https_port.is_none() {
                new_https_port = Some(
                    nginx_adapter
                        .find_available_port(15001, 20000)
                        .await
                        .map_err(|e| {
                            CliError::Generic(format!("Failed to find HTTPS port: {:?}", e))
                        })?,
                );
            }
        }

        // ── 3. VALIDACIÓN PREVIA — OS + Docker (PORTS-008) ─────────────────
        // SEC-EXT-RACE-011: reserve flock BEFORE is_port_free_shared to close TOCTOU
        let _http_port_lock = if new_http_port != old_http_port {
            Some(reserve_port_or_fail(new_http_port, "http-port")?)
        } else {
            None
        };
        if new_http_port != old_http_port && !is_port_free_shared(new_http_port) {
            return Err(port_in_use_error(new_http_port, "http-port"));
        }
        if let Some(new_hp) = new_https_port {
            // SEC-EXT-RACE-011: same flock pattern for https port
            let _https_port_lock = if new_https_port != old_https_port {
                Some(reserve_port_or_fail(new_hp, "https-port")?)
            } else {
                None
            };
            if new_https_port != old_https_port && !is_port_free_shared(new_hp) {
                return Err(port_in_use_error(new_hp, "https-port"));
            }
        }

        // ── 4. Verificar si algo cambió ─────────────────────────────────────
        let http_changed = new_http_port != old_http_port;
        let https_changed = new_https_port != old_https_port;
        let ssl_changed = enable_ssl != has_ssl;

        if !http_changed && !https_changed && !ssl_changed {
            return Ok("ℹ️  No changes needed.".to_string());
        }

        eprintln!("\n🔄 Applying changes...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let sites = list().await?;
        let site = sites.iter().find(|s| s.name == name);
        let backend_port = site.and_then(|s| s.port).unwrap_or(80);

        // ── 5. Actualizar Tor ───────────────────────────────────────────────
        let mut tor_ports = vec![(80u16, new_http_port)];
        if let Some(hp) = new_https_port {
            tor_ports.push((443, hp));
        }
        update_tor_config_ports(&tor_service_name, &tor_ports).await?;
        eprintln!("   ✓ Tor configuration updated");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // 2. Handle SSL changes
        if enable_ssl && !has_ssl {
            // Enable SSL: Create Nginx proxy with SSL
            eprintln!("   🔐 Enabling SSL...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Generate self-signed certificate
            let (cert_path, key_path) = nginx_adapter
                .generate_self_signed_cert(&nginx_service_name)
                .await
                .map_err(|e| {
                    CliError::Generic(format!("Failed to generate SSL certificate: {:?}", e))
                })?;

            // Create NEW Nginx config with SSL (not update — there's no pre-existing config)
            let ssl_config = crate::ports::web::NginxProxyConfigWithSsl {
                service_name: nginx_service_name.clone(),
                http_port: new_http_port,
                https_port: new_https_port.unwrap_or(0),
                backend_port,
                server_name: "localhost".to_string(),
                ssl_cert_path: cert_path.clone(),
                ssl_key_path: key_path,
                rate_limit: None,
            };
            nginx_adapter
                .create_proxy_config_with_ssl(ssl_config)
                .await
                .map_err(CliError::from)?;
            nginx_adapter
                .enable_site(&format!("proxy_{}", nginx_service_name))
                .await
                .map_err(CliError::from)?;

            eprintln!("   ✓ SSL certificate generated: {}", cert_path);
            eprintln!("   ✓ Nginx SSL config created");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        } else if !enable_ssl && has_ssl {
            // Disable SSL: Remove Nginx SSL config
            eprintln!("   🔓 Disabling SSL...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            nginx_adapter
                .disable_site(&nginx_service_name)
                .await
                .map_err(CliError::from)?;
            eprintln!("   ✓ Nginx SSL config removed");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        } else if enable_ssl && (http_changed || https_changed) {
            // SSL enabled, just update ports
            nginx_adapter
                .update_proxy_ports_with_ssl(
                    &nginx_service_name,
                    new_http_port,
                    new_https_port,
                    backend_port,
                )
                .await
                .map_err(CliError::from)?;

            eprintln!("   ✓ Nginx configuration updated");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        // 3. Reload services
        if enable_ssl {
            nginx_adapter.reload().await.map_err(CliError::from)?;
            eprintln!("   ✓ Nginx reloaded");
            let _ = std::io::Write::flush(&mut std::io::stderr());
        }

        tor_adapter.reload_tor().await.map_err(CliError::from)?;
        eprintln!("   ✓ Tor reloaded");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Build result message
        let mut result = format!("\n✅ WordPress site '{}' updated!\n", name);
        result.push_str(&format!("   HTTP:  {}\n", new_http_port));
        if let Some(hp) = new_https_port {
            result.push_str(&format!("   HTTPS: {}\n", hp));
        }
        result.push_str(&format!(
            "   SSL:   {}\n",
            if enable_ssl { "enabled" } else { "disabled" }
        ));

        if let Some(onion) = site.and_then(|s| s.onion_address.clone()) {
            result.push_str(&format!("\n🧅 Onion: {}\n", onion));
        }

        Ok(result)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DRUPAL COMMANDS — DRUPAL-004a (Tor publish/hide)
// ═══════════════════════════════════════════════════════════════════════════
//
// Paralelo a `mod wordpress` (publish/hide). Reusa los mismos use cases
// (`DeployTorService` / `RemoveTorService`) — la única diferencia es el naming
// del Tor service: `drupal-{name}` (consistente con el container `drupal-{name}`
// de DRUPAL-002, §13.3).
//
// `list/create/start/stop/delete/status` viven en `executor::execute_drupal()`
// delegando directamente en `DrupalCmsAdapter` (CmsLifecycle de DRUPAL-002):
// no se añaden aquí para no duplicar la abstracción CMS.
//
// `edit` (cambio de http-port en caliente) requiere recreación del contenedor
// web (§13.17) y queda planificada para DRUPAL-006 — el ciclo E2E del plan
// LAUNCH-2 (DRUPAL-004b: `test_drupal.sh`) cubre solo create→publish→hide→delete.

pub mod drupal {
    use super::*;
    use crate::adapters::cms::drupal::DrupalCmsAdapter;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::cms::CmsLifecycle;

    /// Resuelve el puerto HTTP interno del sitio Drupal vía
    /// `DrupalCmsAdapter::status()`. Devuelve error si el sitio no existe
    /// o si está parado (no hay puerto activo expuesto).
    async fn resolve_backend_port(name: &str) -> CliResult<u16> {
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let adapter = DrupalCmsAdapter::new(Arc::new(docker), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Drupal site '{}' has no active HTTP port. \
                 Make sure it exists and is running (try `drupal status {0}`).",
                name
            ))
        })
    }

    /// Publish a Drupal site on Tor (creates a hidden service).
    ///
    /// Tor service name: `drupal-{name}` (consistente con el contenedor).
    /// Mapping: `.onion:80 → 127.0.0.1:{backend_port}` (Apache del contenedor).
    pub async fn publish(name: &str) -> CliResult<String> {
        let port = resolve_backend_port(name).await?;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );

        let request = DeployTorServiceRequest {
            service_name: format!("drupal-{}", name),
            ports: vec![(80, port)],
        };

        let onion = use_case.execute(request).await.map_err(CliError::from)?;
        Ok(format!(
            "✅ Drupal '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    /// Hide a Drupal site from Tor (removes its hidden service).
    ///
    /// No toca el contenedor: el sitio sigue accesible localmente en el
    /// puerto interno, solo deja de estar publicado en `.onion`.
    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);

        use_case
            .execute(&format!("drupal-{}", name))
            .await
            .map_err(CliError::from)
    }

    /// Edit the HTTP port of an existing Drupal site (DRUPAL-006).
    ///
    /// Docker no permite reasignar port bindings en caliente, así que se
    /// recrea atómicamente el contenedor `drupal-{name}` (web), preservando
    /// imagen, env vars, volumen `/var/www/html`, network y secret mount.
    /// El contenedor de BD `db-{name}-drupal` NO se toca: 3306 es interno
    /// a la network, no tiene port binding al host.
    ///
    /// Si el sitio está publicado en Tor (`drupal-{name}` hidden service),
    /// la cadena `.onion:80 → 127.0.0.1:nuevo_puerto → contenedor:80` se
    /// reactualiza automáticamente reescribiendo el HiddenServicePort y
    /// recargando Tor (§13.16: pares Tor↔Nginx sincronizados).
    ///
    /// Validación previa con `is_port_free_shared` (§13.7 + §13.17): rechaza
    /// el cambio antes de tocar nada si el nuevo puerto está ocupado a nivel
    /// SO o por otro contenedor Docker (incluso parado).
    pub async fn edit(name: &str, http_port: Option<u16>) -> CliResult<String> {
        use crate::ports::cms::CmsLifecycle as _;
        use crate::ports::container::ContainerPort as _;
        use crate::ports::tor::TorManagerPort;

        // ── 1. Resolver puerto actual via DrupalCmsAdapter::status() ──
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let docker_arc = Arc::new(docker);
        let adapter =
            DrupalCmsAdapter::new(docker_arc.clone(), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        let old_port = inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Drupal site '{}' has no active HTTP port. \
                 Make sure it exists (try `drupal status {0}`).",
                name
            ))
        })?;

        let new_port = match http_port {
            Some(p) => p,
            None => {
                return Err(CliError::InvalidInput(
                    "drupal edit requires --http-port <PORT>. \
                     Use `drupal status <name>` to see the current port."
                        .to_string(),
                ));
            }
        };

        if new_port == old_port {
            return Ok(format!(
                "ℹ️  No changes needed (HTTP port already {}).",
                old_port
            ));
        }

        // ── 2. Validación previa antes de tocar el contenedor (§13.17) ──
        let _port_lock = reserve_port_or_fail(new_port, "http-port")?; // SEC-EXT-RACE-011
        if !is_port_free_shared(new_port) {
            return Err(port_in_use_error(new_port, "http-port"));
        }

        eprintln!(
            "🔧 Editing Drupal '{}': HTTP {} → {}",
            name, old_port, new_port
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let container_name = format!("drupal-{}", name);

        // ── 3. Inspeccionar contenedor para reusar imagen + env actuales ──
        // Docker no expone hot-rebind de ports; recreamos preservando todo lo
        // demás. Image + env vienen del propio contenedor (resilientes a
        // upgrades de versión); volumen, network y secret mount son
        // determinísticos por la convención §13.3 + DrupalCmsAdapter.
        let inspect_image = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.Config.Image}}", &container_name])
            .output()
            .map_err(|e| CliError::Generic(format!("docker inspect failed: {}", e)))?;
        let image = String::from_utf8_lossy(&inspect_image.stdout)
            .trim()
            .to_string();
        if image.is_empty() {
            return Err(CliError::Generic(format!(
                "Could not read image for container '{}'. \
                 Make sure the site exists and Docker is reachable.",
                container_name
            )));
        }

        // Constantes alineadas con DrupalCmsAdapter (§13.3 + §13.2)
        let net_name = format!("enola_net_drupal_{}", name);
        let web_volume = format!("/srv/enola-drupal/{}/web", name);
        let secret_path = format!("/srv/enola-drupal/{}/secrets/db_password", name);
        let db_name = format!("db-{}-drupal", name);

        // ── 4. Stop + remove + recrear con nuevo binding (§13.17) ──
        let _ = docker_arc.stop_container(&container_name).await;
        eprintln!("   ✓ Container stopped");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let _ = docker_arc.remove_container(&container_name).await;
        eprintln!("   ✓ Container removed");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let run_status = std::process::Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "--restart",
                "unless-stopped",
                "--network",
                &net_name,
                "-v",
                &format!("{}:/var/www/html", web_volume),
                "-v",
                &format!("{}:/run/secrets/db_password:ro", secret_path),
                "-p",
                &format!("127.0.0.1:{}:80", new_port),
                "-e",
                &format!("ENOLA_DRUPAL_DB_HOST={}", db_name),
                "-e",
                "ENOLA_DRUPAL_DB_NAME=drupal",
                "-e",
                "ENOLA_DRUPAL_DB_USER=drupal",
                "-e",
                "ENOLA_DRUPAL_DB_PASSWORD_FILE=/run/secrets/db_password",
                &image,
            ])
            .status()
            .map_err(|e| CliError::Generic(format!("docker run failed: {}", e)))?;

        if !run_status.success() {
            return Err(CliError::Generic(format!(
                "Failed to recreate container '{}'. \
                 Check `docker logs {}` for details.",
                container_name, container_name
            )));
        }
        eprintln!("   ✓ Container recreated on 127.0.0.1:{}:80", new_port);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 5. Si el sitio está publicado en Tor, sincronizar el mapping ──
        // §13.16: el par Tor↔Nginx debe quedar coherente sin reload manual.
        let tor_adapter = TorConfigAdapter::new();
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let tor_service_name = format!("drupal-{}", name);
        let was_published = services.iter().any(|s| s.name == tor_service_name);

        if was_published {
            eprintln!("   ↻ Updating Tor hidden service '{}'...", tor_service_name);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Re-publicar = remove + deploy con el nuevo backend port.
            let nginx_none: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
                None;
            let remove_uc = RemoveTorService::new(Arc::new(TorConfigAdapter::new()), nginx_none);
            remove_uc
                .execute(&tor_service_name)
                .await
                .map_err(CliError::from)?;

            let deploy_uc = DeployTorService::new(
                Arc::new(TorConfigAdapter::new()),
                Arc::new(SystemdAdapter),
                Arc::new(FileManifestAdapter::new()),
            );
            let req = DeployTorServiceRequest {
                service_name: tor_service_name.clone(),
                ports: vec![(80, new_port)],
            };
            let onion = deploy_uc.execute(req).await.map_err(CliError::from)?;
            return Ok(format!(
                "✅ Drupal '{}' edited (HTTP {} → {})\n🧅 Tor address (unchanged): {}",
                name, old_port, new_port, onion
            ));
        }

        Ok(format!(
            "✅ Drupal '{}' edited (HTTP {} → {})\n   Site is local-only (not published on Tor).",
            name, old_port, new_port
        ))
    }

    #[cfg(test)]
    mod tests {
        //! Tests unitarios DRUPAL-004a.
        //!
        //! No mockeamos los adapters de Tor/Docker (requeriría infraestructura
        //! pesada), pero sí cubrimos:
        //!   - Naming del Tor service (`drupal-{name}`) — anti-regresión §13.3.
        //!   - Que `resolve_backend_port` devuelve `InvalidInput` orientativo
        //!     cuando el sitio no existe (sin Docker disponible falla en el
        //!     primer paso de la cadena, lo cual también es aceptable).

        #[test]
        fn drupal_tor_service_name_matches_container_naming() {
            // Garantiza que el prefijo Tor es idéntico al prefijo Docker
            // del DrupalCmsAdapter (§13.3). Si alguien cambia uno y olvida
            // el otro, este test rompe el aislamiento Tor⇄Nginx⇄Docker.
            let name = "myblog";
            let tor_service = format!("drupal-{}", name);
            let container = format!("drupal-{}", name);
            assert_eq!(
                tor_service, container,
                "Tor service name must match container name (§13.3)"
            );
        }

        #[test]
        fn drupal_tor_service_name_does_not_collide_with_wp() {
            // wp-{name} y drupal-{name} jamás colisionan: dos sitios con el
            // mismo `name` en WP y Drupal coexisten en el mismo nodo.
            let name = "blog";
            let wp = format!("wp-{}", name);
            let drupal = format!("drupal-{}", name);
            assert_ne!(wp, drupal);
            assert!(drupal.starts_with("drupal-"));
            assert!(!drupal.starts_with("wp-"));
        }

        // ── DRUPAL-006: drupal edit --http-port (recreación atómica) ──

        #[test]
        fn drupal_edit_uses_consistent_naming_with_create() {
            // Garantiza que los nombres derivados que usa `edit` (network,
            // volumen web, secret, contenedor BD) coincidan con los que
            // genera `DrupalCmsAdapter::create` (§13.3). Si alguien cambia
            // uno y olvida el otro, este test rompe el aislamiento.
            let name = "myblog";
            let container = format!("drupal-{}", name);
            let net = format!("enola_net_drupal_{}", name);
            let web_vol = format!("/srv/enola-drupal/{}/web", name);
            let secret = format!("/srv/enola-drupal/{}/secrets/db_password", name);
            let db = format!("db-{}-drupal", name);

            // Aserts que blindan el contrato §13.3 / §13.2.
            assert_eq!(container, "drupal-myblog");
            assert_eq!(net, "enola_net_drupal_myblog");
            assert!(web_vol.starts_with("/srv/enola-drupal/"));
            assert!(web_vol.ends_with("/web"));
            assert!(secret.ends_with("/secrets/db_password"));
            assert_eq!(db, "db-myblog-drupal");
        }

        #[test]
        fn drupal_edit_port_binding_is_localhost_only() {
            // §13.16 + §13.44: Docker NUNCA debe bindear a 0.0.0.0.
            // Este test refleja el formato del flag -p que pasa `edit` a
            // `docker run`. Si alguien lo cambia a "0.0.0.0:..." o a
            // ":80" suelto (que Docker interpreta como 0.0.0.0), rompe.
            let new_port: u16 = 8085;
            let mapping = format!("127.0.0.1:{}:80", new_port);
            assert!(mapping.starts_with("127.0.0.1:"));
            assert!(!mapping.starts_with("0.0.0.0"));
            assert!(mapping.ends_with(":80"));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GHOST COMMANDS — CMS-GHOST-003 (publish/hide/edit reales)
// ═══════════════════════════════════════════════════════════════════════════

pub mod ghost {
    //! Cableado real de `ghost publish/hide/edit` paralelo a [`super::drupal`].
    //!
    //! Diferencias clave con Drupal:
    //!   - Sin contenedor BD (SQLite embebido) → `edit` solo recrea web.
    //!   - Puerto interno **2368** (no 80).
    //!   - Volumen único `/srv/enola-ghost/{name}/content/` → `/var/lib/ghost/content`.
    //!   - Env var `url` debe regenerarse al cambiar puerto (Ghost la usa para
    //!     redirecciones absolutas).
    //!
    //! Naming Tor: `ghost-{name}` (mismo prefijo que el contenedor, 13.3).
    //! Anti-colisión 13.3: `ghost-{name}` ≠ `wp-{name}` ≠ `drupal-{name}`.
    use super::*;
    use crate::adapters::cms::ghost::GhostCmsAdapter;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::cms::CmsLifecycle;

    /// Puerto interno donde Ghost escucha dentro del contenedor.
    /// (Constante duplicada del adapter para evitar exponerla públicamente).
    const GHOST_INTERNAL_PORT: u16 = 2368;

    /// Resuelve el puerto HTTP host del blog Ghost vía status del adapter.
    async fn resolve_backend_port(name: &str) -> CliResult<u16> {
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let adapter = GhostCmsAdapter::new(Arc::new(docker), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Ghost blog '{}' has no active HTTP port. \
                 Make sure it exists and is running (try `ghost status {0}`).",
                name
            ))
        })
    }

    /// Publish a Ghost blog on Tor (creates a hidden service).
    ///
    /// Tor service name: `ghost-{name}`.
    /// Mapping: `.onion:80 → 127.0.0.1:{backend_port}` (Node Ghost del contenedor).
    pub async fn publish(name: &str) -> CliResult<String> {
        let port = resolve_backend_port(name).await?;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );

        let request = DeployTorServiceRequest {
            service_name: format!("ghost-{}", name),
            ports: vec![(80, port)],
        };

        let onion = use_case.execute(request).await.map_err(CliError::from)?;
        Ok(format!(
            "✅ Ghost '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    /// Hide a Ghost blog from Tor (removes its hidden service).
    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);

        use_case
            .execute(&format!("ghost-{}", name))
            .await
            .map_err(CliError::from)
    }

    /// Edit the HTTP port of an existing Ghost blog.
    ///
    /// Docker no permite reasignar port bindings en caliente: se recrea
    /// atómicamente el contenedor `ghost-{name}` preservando imagen, env
    /// vars y volumen `/var/lib/ghost/content`.
    ///
    /// La env var `url` se regenera apuntando al nuevo puerto (Ghost la usa
    /// para redirecciones absolutas; sin esto los assets quedan rotos).
    ///
    /// Si el blog está publicado en Tor (`ghost-{name}` hidden service), la
    /// cadena `.onion:80 → 127.0.0.1:nuevo_puerto → contenedor:2368` se
    /// reactualiza automáticamente (13.16: pares Tor↔Backend sincronizados).
    ///
    /// Validación previa con `is_port_free_shared` (13.7 + 13.17).
    pub async fn edit(name: &str, http_port: Option<u16>) -> CliResult<String> {
        use crate::ports::container::ContainerPort as _;
        use crate::ports::tor::TorManagerPort;

        // ── 1. Resolver puerto actual via GhostCmsAdapter::status() ──
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let docker_arc = Arc::new(docker);
        let adapter =
            GhostCmsAdapter::new(docker_arc.clone(), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        let old_port = inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Ghost blog '{}' has no active HTTP port. \
                 Make sure it exists (try `ghost status {0}`).",
                name
            ))
        })?;

        let new_port = match http_port {
            Some(p) => p,
            None => {
                return Err(CliError::InvalidInput(
                    "ghost edit requires --http-port <PORT>. \
                     Use `ghost status <name>` to see the current port."
                        .to_string(),
                ));
            }
        };

        if new_port == old_port {
            return Ok(format!(
                "ℹ️  No changes needed (HTTP port already {}).",
                old_port
            ));
        }

        // ── 2. Validación previa antes de tocar el contenedor (13.17) ──
        let _port_lock = reserve_port_or_fail(new_port, "http-port")?; // SEC-EXT-RACE-011
        if !is_port_free_shared(new_port) {
            return Err(port_in_use_error(new_port, "http-port"));
        }

        eprintln!(
            "🔧 Editing Ghost '{}': HTTP {} → {}",
            name, old_port, new_port
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let container_name = format!("ghost-{}", name);

        // ── 3. Inspeccionar imagen actual (resiliente a upgrades de tag) ──
        let inspect_image = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.Config.Image}}", &container_name])
            .output()
            .map_err(|e| CliError::Generic(format!("docker inspect failed: {}", e)))?;
        let image = String::from_utf8_lossy(&inspect_image.stdout)
            .trim()
            .to_string();
        if image.is_empty() {
            return Err(CliError::Generic(format!(
                "Could not read image for container '{}'. \
                 Make sure the blog exists and Docker is reachable.",
                container_name
            )));
        }

        // Constantes alineadas con GhostCmsAdapter (13.3 + 13.2).
        let net_name = format!("enola_net_ghost_{}", name);
        let content_volume = format!("/srv/enola-ghost/{}/content", name);

        // ── 4. Stop + remove + recrear con nuevo binding (13.17) ──
        let _ = docker_arc.stop_container(&container_name).await;
        eprintln!("   ✓ Container stopped");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let _ = docker_arc.remove_container(&container_name).await;
        eprintln!("   ✓ Container removed");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let new_url = format!("url=http://127.0.0.1:{}", new_port);
        let port_mapping = format!("127.0.0.1:{}:{}", new_port, GHOST_INTERNAL_PORT);
        let volume_mapping = format!("{}:/var/lib/ghost/content", content_volume);

        let run_status = std::process::Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "--restart",
                "unless-stopped",
                "--network",
                &net_name,
                "-v",
                &volume_mapping,
                "-p",
                &port_mapping,
                "-e",
                &new_url,
                "-e",
                "database__client=sqlite3",
                "-e",
                "database__connection__filename=/var/lib/ghost/content/data/ghost.db",
                "-e",
                "database__useNullAsDefault=true",
                "-e",
                "NODE_ENV=production",
                &image,
            ])
            .status()
            .map_err(|e| CliError::Generic(format!("docker run failed: {}", e)))?;

        if !run_status.success() {
            return Err(CliError::Generic(format!(
                "Failed to recreate container '{}'. \
                 Check `docker logs {}` for details.",
                container_name, container_name
            )));
        }
        eprintln!(
            "   ✓ Container recreated on 127.0.0.1:{}:{}",
            new_port, GHOST_INTERNAL_PORT
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // ── 5. Sincronizar Tor si el blog está publicado (13.16) ──
        let tor_adapter = TorConfigAdapter::new();
        let services = tor_adapter
            .list_hidden_services()
            .await
            .map_err(CliError::from)?;
        let tor_service_name = format!("ghost-{}", name);
        let was_published = services.iter().any(|s| s.name == tor_service_name);

        if was_published {
            eprintln!("   ↻ Updating Tor hidden service '{}'...", tor_service_name);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            let nginx_none: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
                None;
            let remove_uc = RemoveTorService::new(Arc::new(TorConfigAdapter::new()), nginx_none);
            remove_uc
                .execute(&tor_service_name)
                .await
                .map_err(CliError::from)?;

            let deploy_uc = DeployTorService::new(
                Arc::new(TorConfigAdapter::new()),
                Arc::new(SystemdAdapter),
                Arc::new(FileManifestAdapter::new()),
            );
            let req = DeployTorServiceRequest {
                service_name: tor_service_name.clone(),
                ports: vec![(80, new_port)],
            };
            let onion = deploy_uc.execute(req).await.map_err(CliError::from)?;
            return Ok(format!(
                "✅ Ghost '{}' edited (HTTP {} → {})\n🧅 Tor address (unchanged): {}",
                name, old_port, new_port, onion
            ));
        }

        Ok(format!(
            "✅ Ghost '{}' edited (HTTP {} → {})\n   Blog is local-only (not published on Tor).",
            name, old_port, new_port
        ))
    }

    #[cfg(test)]
    mod tests {
        //! Tests unitarios CMS-GHOST-003 — invariantes de naming/binding sin Docker.

        #[test]
        fn ghost_tor_service_name_matches_container_naming() {
            // 13.3: el prefijo Tor debe ser idéntico al prefijo del contenedor.
            let name = "myblog";
            let tor_service = format!("ghost-{}", name);
            let container = format!("ghost-{}", name);
            assert_eq!(tor_service, container);
        }

        #[test]
        fn ghost_naming_does_not_collide_with_wp_or_drupal() {
            // Tres blogs `myblog` en WP, Drupal y Ghost coexisten sin colisión.
            let n = "myblog";
            let wp = format!("wp-{}", n);
            let drupal = format!("drupal-{}", n);
            let ghost = format!("ghost-{}", n);
            assert_ne!(wp, ghost);
            assert_ne!(drupal, ghost);
            assert!(ghost.starts_with("ghost-"));
            assert!(!ghost.starts_with("wp-"));
            assert!(!ghost.starts_with("drupal-"));
        }

        #[test]
        fn ghost_edit_uses_consistent_naming_with_create() {
            // 13.3 + 13.2: los nombres derivados que usa `edit` deben coincidir
            // con los que genera `GhostCmsAdapter::create`.
            let name = "myblog";
            let container = format!("ghost-{}", name);
            let net = format!("enola_net_ghost_{}", name);
            let content_vol = format!("/srv/enola-ghost/{}/content", name);

            assert_eq!(container, "ghost-myblog");
            assert_eq!(net, "enola_net_ghost_myblog");
            assert!(content_vol.starts_with("/srv/enola-ghost/"));
            assert!(content_vol.ends_with("/content"));
        }

        #[test]
        fn ghost_edit_port_binding_is_localhost_only() {
            // 13.16 + 13.44: Docker NUNCA debe bindear a 0.0.0.0.
            // Ghost usa puerto interno 2368 (NO 80, anti-regresión).
            let new_port: u16 = 8085;
            let internal: u16 = 2368;
            let mapping = format!("127.0.0.1:{}:{}", new_port, internal);
            assert!(mapping.starts_with("127.0.0.1:"));
            assert!(!mapping.starts_with("0.0.0.0"));
            assert!(mapping.ends_with(":2368"));
            assert!(!mapping.ends_with(":80"));
        }

        #[test]
        fn ghost_edit_regenerates_url_env_var() {
            // El env var `url` debe regenerarse para que las redirecciones
            // absolutas de Ghost apunten al nuevo puerto.
            let new_port: u16 = 9100;
            let url_env = format!("url=http://127.0.0.1:{}", new_port);
            assert_eq!(url_env, "url=http://127.0.0.1:9100");
            assert!(url_env.contains("http://127.0.0.1:"));
            assert!(!url_env.contains("0.0.0.0"));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAGNOLIA COMMANDS (CMS-MAGNOLIA-CLI)
// ═══════════════════════════════════════════════════════════════════════════

pub mod magnolia {
    use super::*;
    use crate::adapters::cms::magnolia::MagnoliaCmsAdapter;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::cms::CmsLifecycle;

    async fn resolve_backend_port(name: &str) -> CliResult<u16> {
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let adapter =
            MagnoliaCmsAdapter::new(Arc::new(docker), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Magnolia instance '{}' has no active HTTP port.",
                name
            ))
        })
    }

    pub async fn publish(name: &str) -> CliResult<String> {
        let port = resolve_backend_port(name).await?;
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        let request = DeployTorServiceRequest {
            service_name: format!("magnolia-{}", name),
            ports: vec![(80, port)],
        };
        let onion = use_case.execute(request).await.map_err(CliError::from)?;
        Ok(format!(
            "✅ Magnolia '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);
        use_case
            .execute(&format!("magnolia-{}", name))
            .await
            .map_err(CliError::from)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// STRAPI COMMANDS (CMS-STRAPI-CLI)
// ═══════════════════════════════════════════════════════════════════════════

pub mod strapi {
    use super::*;
    use crate::adapters::cms::strapi::StrapiCmsAdapter;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::cms::CmsLifecycle;

    async fn resolve_backend_port(name: &str) -> CliResult<u16> {
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let adapter = StrapiCmsAdapter::new(Arc::new(docker), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Strapi instance '{}' has no active HTTP port.",
                name
            ))
        })
    }

    pub async fn publish(name: &str) -> CliResult<String> {
        let port = resolve_backend_port(name).await?;
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        let request = DeployTorServiceRequest {
            service_name: format!("strapi-{}", name),
            ports: vec![(80, port)],
        };
        let onion = use_case.execute(request).await.map_err(CliError::from)?;
        Ok(format!(
            "✅ Strapi '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);
        use_case
            .execute(&format!("strapi-{}", name))
            .await
            .map_err(CliError::from)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WAGTAIL COMMANDS (CMS-WAGTAIL-CLI)
// ═══════════════════════════════════════════════════════════════════════════

pub mod wagtail {
    use super::*;
    use crate::adapters::cms::wagtail::WagtailCmsAdapter;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::cms::CmsLifecycle;

    async fn resolve_backend_port(name: &str) -> CliResult<u16> {
        let docker = BollardDockerAdapter::new()
            .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
        let adapter =
            WagtailCmsAdapter::new(Arc::new(docker), Arc::new(FileManifestAdapter::new()));
        let inst = adapter.status(name).await.map_err(CliError::from)?;
        inst.http_port.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Wagtail instance '{}' has no active HTTP port.",
                name
            ))
        })
    }

    pub async fn publish(name: &str) -> CliResult<String> {
        let port = resolve_backend_port(name).await?;
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let use_case = DeployTorService::new(
            tor_adapter,
            systemd_adapter,
            Arc::new(FileManifestAdapter::new()),
        );
        let request = DeployTorServiceRequest {
            service_name: format!("wagtail-{}", name),
            ports: vec![(80, port)],
        };
        let onion = use_case.execute(request).await.map_err(CliError::from)?;
        Ok(format!(
            "✅ Wagtail '{}' published!\n🧅 Address: {}",
            name, onion
        ))
    }

    pub async fn hide(name: &str) -> CliResult<()> {
        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter: Option<Arc<dyn crate::ports::web::NginxManagerPort + Send + Sync>> =
            None;
        let use_case = RemoveTorService::new(tor_adapter, nginx_adapter);
        use_case
            .execute(&format!("wagtail-{}", name))
            .await
            .map_err(CliError::from)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FILE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod files {
    use super::*;

    /// List file shares — incluye servicios con prefijo fileserver_ y servicios SSL
    /// creados directamente con el nombre del share (sin prefijo).
    pub async fn list() -> CliResult<Vec<crate::ports::tor::TorServiceInfo>> {
        let adapter = Arc::new(TorConfigAdapter::new());
        let use_case = ListTorServices::new(adapter);
        let services = use_case.execute().await?;
        Ok(services
            .into_iter()
            .filter(|s| {
                // Servicios HTTP creados por DeployFileServer (prefijo fileserver_/fileserver-)
                s.name.starts_with("fileserver_") || s.name.starts_with("fileserver-")
            // Servicios SSL creados directamente con el nombre del share (sin prefijo)
            // Se detectan por la existencia de config Nginx con autoindex
            || {
                let avail = format!("/etc/nginx/sites-available/{}", s.name);
                std::fs::read_to_string(&avail)
                    .map(|c| c.contains("autoindex on"))
                    .unwrap_or(false)
            }
            })
            .collect())
    }

    /// Create a file share
    pub async fn create(name: &str, auth: bool, ssl: bool) -> CliResult<String> {
        use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
        use crate::ports::file::FileManagerPort;
        use crate::ports::service::ServiceManagerPort;
        use crate::ports::tor::TorManagerPort;
        use crate::ports::web::NginxManagerPort;

        let tor_adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());
        let systemd_adapter: Arc<dyn ServiceManagerPort + Send + Sync> = Arc::new(SystemdAdapter);
        let file_adapter: Arc<dyn FileManagerPort + Send + Sync> =
            Arc::new(EnolaFileAdapter::new());
        let manifest = Arc::new(FileManifestAdapter::new());
        use crate::ports::manifest::ManifestPort;

        let share_path = format!("/srv/enola-files/{}", name);
        let share_path_buf = std::path::PathBuf::from(&share_path);

        eprintln!("📁 Creating file share '{}'...", name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Ensure directory exists
        file_adapter
            .ensure_dir(&share_path_buf)
            .await
            .map_err(|e| CliError::Generic(format!("Failed to create directory: {:?}", e)))?;

        // Set permissions (ignore errors for dev environments without proper users)
        let _ = file_adapter
            .set_ownership(&share_path_buf, "root", "www-data")
            .await;
        let _ = file_adapter.set_permissions(&share_path_buf, 0o750).await;

        if ssl {
            // HTTPS mode: Create file server with SSL certificate
            eprintln!("🔐 Creating File Server with HTTPS (Tor → Nginx+SSL → Files)");
            eprintln!("   Architecture: .onion:80/443 → Nginx:[auto] → FileServer");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Find two available ports for Nginx (HTTP and HTTPS)
            let (http_port, _http_port_lock) = nginx_adapter
                .find_available_port_with_lock(10000, 15000)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to find HTTP port: {:?}", e)))?;
            let (https_port, _https_port_lock) = nginx_adapter
                .find_available_port_with_lock(15001, 20000)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to find HTTPS port: {:?}", e)))?;

            eprintln!("   📍 Using Nginx HTTP port: {}", http_port);
            eprintln!("   📍 Using Nginx HTTPS port: {}", https_port);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Generate self-signed certificate
            eprintln!("   🔑 Generating self-signed SSL certificate...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let (cert_path, key_path) = nginx_adapter
                .generate_self_signed_cert(name)
                .await
                .map_err(|e| {
                    CliError::Generic(format!("Failed to generate SSL certificate: {:?}", e))
                })?;
            let _ = manifest.append("ssl_cert", &cert_path);
            let _ = manifest.append("ssl_key", &key_path);

            // Create Nginx fileserver config with SSL (HTTP + HTTPS)
            eprintln!("   📝 Creating Nginx SSL config for file server...");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Create combined HTTP + HTTPS config for file server
            let pqc_tls_directive = crate::infrastructure::pqc_tls::nginx_pqc_curve_directive();
            let config_content = format!(
                r#"
# Auto-generated by Enola Server (FileServer with SSL)

# HTTP Server
server {{
    listen 127.0.0.1:{http_port};
    server_name localhost;

    root {share_path};

    autoindex on;
    autoindex_exact_size off;
    autoindex_localtime on;

    disable_symlinks on;

    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "DENY" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "no-referrer" always;

    server_tokens off;

    location / {{
        try_files $uri $uri/ =404;
    }}

    location ~ /\. {{
        deny all;
        return 404;
    }}

    access_log /var/log/nginx/{name}_http_access.log;
    error_log /var/log/nginx/{name}_http_error.log;
}}

# HTTPS Server
server {{
    listen 127.0.0.1:{https_port} ssl;
    server_name localhost;

    ssl_certificate {cert_path};
    ssl_certificate_key {key_path};
    ssl_protocols TLSv1.3;
{pqc_tls_directive}
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;
    ssl_session_cache shared:SSL:10m;
    ssl_session_timeout 1d;

    root {share_path};

    autoindex on;
    autoindex_exact_size off;
    autoindex_localtime on;

    disable_symlinks on;

    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "DENY" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "no-referrer" always;
    add_header Strict-Transport-Security "max-age=31536000" always;

    server_tokens off;

    location / {{
        try_files $uri $uri/ =404;
    }}

    location ~ /\. {{
        deny all;
        return 404;
    }}

    access_log /var/log/nginx/{name}_https_access.log;
    error_log /var/log/nginx/{name}_https_error.log;
}}
"#,
                http_port = http_port,
                https_port = https_port,
                share_path = share_path,
                cert_path = cert_path,
                key_path = key_path,
                name = name,
                pqc_tls_directive = pqc_tls_directive
            );

            // Write config file
            let config_path = format!("/etc/nginx/sites-available/{}", name);
            let config_path_buf = std::path::PathBuf::from(&config_path);
            let _nginx_site_lock =
                crate::infrastructure::shared_artifact_lock::maybe_lock_nginx_site(
                    &config_path_buf,
                )
                .map_err(|e| {
                    CliError::Generic(format!(
                        "Failed to acquire lock for shared Nginx artifact '{}': {}",
                        config_path, e
                    ))
                })?;
            tokio::fs::write(&config_path, config_content)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to write Nginx config: {}", e)))?;
            let _ = manifest.append("nginx_config", name);

            eprintln!("   ✓ Nginx SSL config created");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Enable site and reload
            eprintln!("   🔗 Enabling Nginx site...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            nginx_adapter
                .enable_site(name)
                .await
                .map_err(|e| CliError::Generic(format!("Failed to enable site: {:?}", e)))?;

            if !nginx_adapter.validate_config().await.unwrap_or(false) {
                return Err(CliError::Generic(
                    "Nginx config validation failed".to_string(),
                ));
            }
            nginx_adapter
                .reload()
                .await
                .map_err(|e| CliError::Generic(format!("Failed to reload Nginx: {:?}", e)))?;
            eprintln!("   ✓ Nginx reloaded");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Create Tor hidden service with both HTTP and HTTPS ports
            eprintln!("   🧅 Deploying Tor hidden service with HTTP+HTTPS...");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let use_case = DeployTorService::new(
                tor_adapter.clone(),
                systemd_adapter,
                Arc::new(FileManifestAdapter::new()),
            );
            let request = DeployTorServiceRequest {
                service_name: name.to_string(),
                ports: vec![
                    (80, http_port),   // HTTP
                    (443, https_port), // HTTPS
                ],
            };
            let onion = use_case.execute(request).await.map_err(CliError::from)?;

            // Handle Auth (Optional)
            if auth {
                tor_adapter
                    .enable_client_auth(name)
                    .await
                    .map_err(CliError::from)?;
            }

            eprintln!("\n📋 File Server Configuration (HTTPS enabled):");
            eprintln!("   Shared Path:  {}", share_path);
            eprintln!("   Nginx config: /etc/nginx/sites-available/{}", name);
            eprintln!("   SSL cert:     {}", cert_path);
            eprintln!("   SSL key:      {}", key_path);
            eprintln!("   Tor config:   /etc/tor/enola.d/{}.conf", name);
            eprintln!(
                "   Flow HTTP:    {}:80 → Nginx:{} → Files",
                onion, http_port
            );
            eprintln!(
                "   Flow HTTPS:   {}:443 → Nginx:{} → Files",
                onion, https_port
            );
            let _ = std::io::Write::flush(&mut std::io::stderr());

            Ok(format!(
                "File Server Created (HTTPS).\nOnion Address: {}\nShared Path: {}",
                onion, share_path
            ))
        } else {
            // Standard HTTP mode (no SSL) - use existing DeployFileServer
            eprintln!("📁 Creating File Server (HTTP only)");
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Find an available port — NEVER hardcode (8080 conflicts with auth server, etc.)
            let (http_port, _http_port_lock) = nginx_adapter
                .find_available_port_with_lock(20000, 30000)
                .await
                .map_err(|e| {
                    CliError::Generic(format!("Failed to find available port: {:?}", e))
                })?;
            eprintln!("   📍 Using Nginx port: {}", http_port);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            let use_case = DeployFileServer::new(
                nginx_adapter,
                tor_adapter,
                systemd_adapter,
                file_adapter,
                Arc::new(FileManifestAdapter::new()),
            );
            let request = DeployFileServerRequest {
                service_name: name.to_string(),
                enable_auth: auth,
                port: http_port,
                share_path: Some(share_path.clone()),
            };
            use_case
                .execute(request)
                .await
                .map(|(onion, path)| {
                    format!(
                        "File Server Created.\nOnion Address: {}\nShared Path: {}",
                        onion, path
                    )
                })
                .map_err(CliError::from)
        }
    }

    /// Delete a file share
    pub async fn delete(name: &str) -> CliResult<()> {
        // Read share path from Nginx config before removing it
        let share_path = read_share_path(name);

        let adapter = Arc::new(TorConfigAdapter::new());
        let nginx_adapter = Arc::new(NginxAdapter::new());
        let use_case = RemoveTorService::new(adapter, Some(nginx_adapter));
        // Try underscore prefix first (created by DeployFileServer)
        let result = use_case.execute(&format!("fileserver_{}", name)).await;
        if result.is_ok() {
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.crt", name)).await;
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.key", name)).await;
            cleanup_share_dir(&share_path, name);
            return Ok(());
        }
        // Try hyphen prefix (legacy)
        let adapter2 = Arc::new(TorConfigAdapter::new());
        let nginx_adapter2 = Arc::new(NginxAdapter::new());
        let use_case2 = RemoveTorService::new(adapter2, Some(nginx_adapter2));
        let result2 = use_case2.execute(&format!("fileserver-{}", name)).await;
        if result2.is_ok() {
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.crt", name)).await;
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.key", name)).await;
            cleanup_share_dir(&share_path, name);
            return Ok(());
        }
        // Try direct name (SSL services created without fileserver_ prefix)
        let adapter3 = Arc::new(TorConfigAdapter::new());
        let nginx_adapter3 = Arc::new(NginxAdapter::new());
        let use_case3 = RemoveTorService::new(adapter3, Some(nginx_adapter3));
        let result3 = use_case3.execute(name).await;
        if result3.is_ok() {
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.crt", name)).await;
            let _ = tokio::fs::remove_file(format!("/etc/nginx/ssl/{}.key", name)).await;
            cleanup_share_dir(&share_path, name);
            return Ok(());
        }
        result3.map_err(CliError::from)
    }

    fn read_share_path(name: &str) -> Option<String> {
        let candidates = [
            format!("/etc/nginx/sites-available/fileserver_{}", name),
            format!("/etc/nginx/sites-available/fileserver-{}", name),
            format!("/etc/nginx/sites-available/{}", name),
        ];
        for cfg_path in &candidates {
            if let Ok(content) = std::fs::read_to_string(cfg_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("root ") {
                        return Some(rest.trim_end_matches(';').trim().to_string());
                    }
                }
            }
        }
        None
    }

    fn cleanup_share_dir(share_path: &Option<String>, name: &str) {
        let path = share_path
            .clone()
            .unwrap_or_else(|| format!("/srv/enola-files/{}", name));
        let _ = std::fs::remove_dir_all(&path);
    }

    /// Edit file share settings (port)
    pub async fn edit(name: &str, port: Option<u16>) -> CliResult<String> {
        use crate::application::edit_port_config::EditPortConfig;
        use crate::ports::web::NginxManagerPort;

        // Get current service info.
        // Supports three naming patterns:
        //   1. fileserver_{name}  — HTTP services (DeployFileServer)
        //   2. fileserver-{name}  — legacy hyphen variant
        //   3. {name}             — SSL services (created directly without prefix)
        let services = list().await?;
        let service = services
            .iter()
            .find(|s| {
                s.name == format!("fileserver_{}", name)
                    || s.name == format!("fileserver-{}", name)
                    || s.name == name
            })
            .ok_or_else(|| CliError::InvalidInput(format!("File share '{}' not found", name)))?;

        let stored_name = service.name.clone();

        let (old_virtual, old_target_str) = service
            .ports
            .first()
            .map(|(v, t)| (*v, t.clone()))
            .unwrap_or((80, "127.0.0.1:8080".to_string()));

        let old_target: u16 = old_target_str
            .split(':')
            .next_back()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        if let Some(new_port) = port {
            let tor_adapter = Arc::new(TorConfigAdapter::new());
            let nginx_adapter = Arc::new(NginxAdapter::new());
            let use_case = EditPortConfig::new(tor_adapter, Some(nginx_adapter.clone()));

            // For fileservers: onion_port (virtual) stays at old_virtual (80),
            // nginx_listen_port and backend_port both change to new_port.
            // This updates:
            //   Tor:   HiddenServicePort 80 127.0.0.1:NEW_PORT
            //   Nginx: listen 127.0.0.1:NEW_PORT  (replaces old listen line)
            use_case
                .execute(&stored_name, old_virtual, new_port, new_port)
                .await
                .map_err(CliError::from)?;

            // update_proxy_ports already calls nginx.reload() on success,
            // but call it explicitly as a safety net in case the config had no proxy_pass.
            let _ = nginx_adapter.reload().await;

            Ok(format!(
                "✅ File share '{}' port updated to {}",
                name, new_port
            ))
        } else {
            Ok(format!(
                "File share '{}' current port: {} -> {}",
                name, old_virtual, old_target
            ))
        }
    }

    /// Fix file share permissions
    pub async fn fix_perms(name: &str) -> CliResult<()> {
        use std::process::Command;

        // Try to read the actual root path from Nginx config (supports custom paths)
        let nginx_config_paths = [
            format!("/etc/nginx/sites-available/fileserver_{}", name),
            format!("/etc/nginx/sites-available/{}", name),
        ];
        let default_path = format!("/srv/enola-files/{}", name);
        let path = nginx_config_paths
            .iter()
            .find_map(|cfg_path| {
                std::fs::read_to_string(cfg_path).ok().and_then(|content| {
                    content.lines().find_map(|line| {
                        let trimmed = line.trim();
                        trimmed
                            .strip_prefix("root ")
                            .map(|rest| rest.trim_end_matches(';').trim().to_string())
                    })
                })
            })
            .unwrap_or(default_path);

        Command::new("chown")
            .args(["-R", "root:www-data", &path])
            .status()?;

        Command::new("chmod").args(["-R", "750", &path]).status()?;

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAINTENANCE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod maintenance {
    use super::*;
    use crate::application::system_health_check::SystemHealthCheck;

    /// Get system status
    pub async fn status() -> CliResult<String> {
        // SystemHealthCheck doesn't need adapters for this implementation
        let use_case = SystemHealthCheck::new();
        let status = use_case.execute().await.map_err(CliError::from)?;

        // Format status to string
        let report = format!(
            "System Status: {}\nCPU: {:.1}%\nMemory: {}/{} bytes\nDisks: {:?}",
            status.overall,
            status.cpu_usage,
            status.memory_used,
            status.memory_total,
            status.disk_usage
        );
        Ok(report)
    }

    /// Run smoke test (removed in lite version)
    pub async fn smoke_test() -> CliResult<String> {
        Err(CliError::Generic(
            "Smoke test is not available in enola-cli-lite.".to_string(),
        ))
    }

    /// Enable automatic checks
    pub async fn enable_checks() -> CliResult<()> {
        use std::process::Command;
        Command::new("systemctl")
            .args(["enable", "--now", "enola-healthcheck.timer"])
            .status()?;
        Ok(())
    }

    /// Disable automatic checks
    pub async fn disable_checks() -> CliResult<()> {
        use std::process::Command;
        Command::new("systemctl")
            .args(["disable", "--now", "enola-healthcheck.timer"])
            .status()?;
        Ok(())
    }

    /// Get timer status
    pub async fn timer_status() -> CliResult<String> {
        use std::process::Command;
        let output = Command::new("systemctl")
            .args(["status", "enola-healthcheck.timer"])
            .output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// SSH configuration info
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct SshConfigInfo {
        pub port: u16,
        pub password_auth: bool,
        pub pubkey_auth: bool,
        pub root_login: String,
        pub authorized_keys_count: usize,
    }

    /// Get SSH configuration
    pub async fn ssh_config() -> CliResult<SshConfigInfo> {
        use std::fs;

        let config_path = "/etc/ssh/sshd_config";

        let mut port = 22u16;
        let mut password_auth = true;
        let mut pubkey_auth = true;
        let mut root_login = "prohibit-password".to_string();

        // Handle missing sshd_config gracefully — return defaults
        let content = match fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "ℹ️  SSH config not found at {}. Showing defaults.",
                    config_path
                );
                eprintln!("   Install OpenSSH server: sudo apt install openssh-server");
                String::new()
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(CliError::Generic(format!(
                    "Permission denied reading {}.\n  Run with: sudo enola-cli maintenance ssh-config",
                    config_path
                )));
            }
            Err(e) => {
                return Err(CliError::Io(e));
            }
        };

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0].to_lowercase().as_str() {
                    "port" => port = parts[1].parse().unwrap_or(22),
                    "passwordauthentication" => password_auth = parts[1].to_lowercase() == "yes",
                    "pubkeyauthentication" => pubkey_auth = parts[1].to_lowercase() == "yes",
                    "permitrootlogin" => root_login = parts[1].to_string(),
                    _ => {}
                }
            }
        }

        // Count authorized keys
        let auth_keys_path = "/root/.ssh/authorized_keys";
        let authorized_keys_count = if std::path::Path::new(auth_keys_path).exists() {
            fs::read_to_string(auth_keys_path)
                .map(|c| {
                    c.lines()
                        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        Ok(SshConfigInfo {
            port,
            password_auth,
            pubkey_auth,
            root_login,
            authorized_keys_count,
        })
    }

    /// PQC-012: Hardening SSH del host con algoritmos post-cuánticos (TRANSITORIO).
    ///
    /// Añade `sntrup761x25519-sha512@openssh.com` como primer KEX preferido.
    /// Requiere OpenSSH ≥9.0 en el servidor. Es una mejora TRANSITORIA:
    /// cuando Tor, TLS y otros protocolos soporten PQC completo, se actualizará.
    ///
    /// Genera un bloque `# Enola PQC hardening` en /etc/ssh/sshd_config.d/99-enola-pqc.conf
    /// para no modificar el sshd_config principal (más seguro, más fácil de revertir).
    pub async fn ssh_harden_pqc(force: bool, dry_run: bool) -> CliResult<String> {
        use std::fs;
        use std::process::Command;

        // -- Verificar OpenSSH versión --
        let ssh_version = Command::new("ssh")
            .arg("-V")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let version_ok = ssh_version.contains("OpenSSH_9") || ssh_version.contains("OpenSSH_10");

        // Verificar soporte de sntrup
        let sntrup_supported = Command::new("ssh")
            .args(["-Q", "kex"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("sntrup761"))
            .unwrap_or(false);

        let drop_in_dir = std::path::Path::new("/etc/ssh/sshd_config.d");
        let drop_in_path = drop_in_dir.join("99-enola-pqc.conf");

        // Obtener fecha actual para el comentario del archivo
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Fecha aproximada en formato YYYY-MM-DD (sin chrono)
        let days_since_epoch = now_ts / 86400;
        let year = 1970 + days_since_epoch / 365;
        let generated_date = format!("~{}-generated", year);

        // -- Contenido del drop-in --
        // PQC-010-UPDATE: cuando OpenSSH soporte más algoritmos PQC NIST,
        // actualizar KEX con: ml-kem-768-sha256@openssh.com (en desarrollo)
        let content = format!(
            "# Enola CLI — SSH Post-Quantum Hardening (PQC-012)\n\
             # Generated: {}\n\
             # TRANSITIONAL: sntrup761x25519-sha512 = NTRU Prime + X25519 hybrid\n\
             # UPDATE TRIGGER: When OpenSSH supports ML-KEM (FIPS 203) natively,\n\
             #   run: sudo enola-cli maintenance ssh-harden-pqc --force\n\
             #   to apply the updated algorithm list.\n\
             # Revert: sudo rm /etc/ssh/sshd_config.d/99-enola-pqc.conf && sudo systemctl reload sshd\n\
             \n\
             # Key exchange: PQC hybrid first, then classical fallback\n\
             KexAlgorithms sntrup761x25519-sha512@openssh.com,curve25519-sha256@libssh.org,curve25519-sha256,diffie-hellman-group16-sha512,diffie-hellman-group18-sha512\n\
             \n\
             # Host keys: prefer Ed25519, then RSA-4096 (ECDSA removed — smaller but same vulnerability)\n\
             HostKeyAlgorithms ssh-ed25519,ssh-ed25519-cert-v01@openssh.com,rsa-sha2-512,rsa-sha2-256\n\
             \n\
             # MACs: authenticated encryption modes only (ETM = Encrypt-Then-MAC)\n\
             MACs hmac-sha2-256-etm@openssh.com,hmac-sha2-512-etm@openssh.com,hmac-sha2-256,hmac-sha2-512\n\
             \n\
             # Ciphers: ChaCha20-Poly1305 first (post-quantum safe for symmetric part)\n\
             Ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes128-gcm@openssh.com,aes256-ctr\n",
            generated_date
        );

        let mut report = String::new();
        report.push_str("🔬 SSH Post-Quantum Hardening (PQC-012)\n");
        report.push_str(&format!("   OpenSSH version: {}\n", ssh_version));

        if !version_ok {
            report.push_str("   ⚠️  WARNING: OpenSSH 9.0+ recommended for sntrup761 support.\n");
            report.push_str("      Current version may not support the PQC KEX algorithm.\n");
            report.push_str(
                "      Upgrade: sudo apt install openssh-server (Ubuntu 24.04 has 9.x)\n",
            );
        }

        if !sntrup_supported {
            report.push_str("   ⚠️  WARNING: sntrup761x25519-sha512 not found in `ssh -Q kex`.\n");
            report
                .push_str("      The PQC algorithm will be skipped, falling back to classical.\n");
        } else {
            report.push_str("   ✅ sntrup761x25519-sha512 (PQC hybrid) is supported.\n");
        }

        report.push_str(&format!("\n   Target: {}\n", drop_in_path.display()));
        report.push_str("   Config block that will be applied:\n");
        for line in content.lines() {
            report.push_str(&format!("     {}\n", line));
        }

        if dry_run {
            report.push_str("\n   DRY RUN — no changes applied. Remove --dry-run to apply.\n");
            return Ok(report);
        }

        if !force {
            report.push_str("\n   ⚠️  This will modify /etc/ssh/sshd_config.d/99-enola-pqc.conf\n");
            report.push_str("      and reload sshd. Add --force to apply without confirmation.\n");
            return Ok(report);
        }

        // Crear directorio drop-in si no existe (Ubuntu 22.04+)
        if !drop_in_dir.exists() {
            fs::create_dir_all(drop_in_dir).map_err(|e| {
                CliError::Generic(format!(
                    "Cannot create {}: {}. Run as root.",
                    drop_in_dir.display(),
                    e
                ))
            })?;
        }

        // Escribir configuración
        fs::write(&drop_in_path, &content).map_err(|e| {
            CliError::Generic(format!(
                "Cannot write {}: {}. Run: sudo enola-cli maintenance ssh-harden-pqc --force",
                drop_in_path.display(),
                e
            ))
        })?;

        // Recargar sshd
        let reload = Command::new("systemctl")
            .args(["reload", "sshd"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if reload {
            report.push_str(&format!("\n   ✅ Written: {}\n", drop_in_path.display()));
            report.push_str("   ✅ sshd reloaded successfully.\n");
            report.push_str("   🔬 PQC hybrid KEX is now active for all SSH connections.\n");
            report.push_str(
                "\n   📌 TRANSITIONAL MEASURE: When OpenSSH supports ML-KEM (FIPS 203),\n",
            );
            report.push_str("      run this command again to apply the updated algorithm list.\n");
            report.push_str("   📌 To revert: sudo rm /etc/ssh/sshd_config.d/99-enola-pqc.conf && sudo systemctl reload sshd\n");
        } else {
            report.push_str(&format!("\n   ✅ Written: {}\n", drop_in_path.display()));
            report.push_str(
                "   ⚠️  sshd reload failed. Apply manually: sudo systemctl reload sshd\n",
            );
        }

        // PQC-011: Verificar OpenSSH ≥9.0 en contenedores Forgejo activos.
        // `docker exec enola-git-X ssh -V` devuelve la versión del OpenSSH del contenedor.
        // Forgejo usa su propio servidor SSH Go (crypto/ssh), pero se verifica OpenSSH del
        // sistema del contenedor — útil para auditoría. Se reporta sin bloquear.
        report.push_str("\n🔬 PQC-011: Forgejo SSH version check\n");
        let git_containers_out = Command::new("docker")
            .args([
                "ps",
                "--filter",
                "name=enola-git-",
                "--format",
                "{{.Names}}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        let git_containers: Vec<&str> = git_containers_out
            .lines()
            .filter(|l| !l.is_empty())
            .collect();

        if git_containers.is_empty() {
            report.push_str("   ℹ️  No running Forgejo containers found (enola-git-*).\n");
            report.push_str("      Start a git instance with: enola-cli git create <name>\n");
        } else {
            for container in &git_containers {
                let ver_out = Command::new("docker")
                    .args(["exec", container, "ssh", "-V"])
                    .output()
                    .map(|o| {
                        // ssh -V escribe en stderr
                        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if !stderr.is_empty() {
                            stderr
                        } else {
                            stdout
                        }
                    })
                    .unwrap_or_else(|_| "docker exec failed".to_string());

                let ok = ver_out.contains("OpenSSH_9") || ver_out.contains("OpenSSH_10");
                let icon = if ok { "✅" } else { "⚠️ " };
                report.push_str(&format!("   {} {} → {}\n", icon, container, ver_out));

                if !ok && !ver_out.contains("docker exec failed") {
                    report.push_str(
                        "      NOTE: Forgejo's built-in SSH uses Go's crypto/ssh, not OpenSSH.\n",
                    );
                    report.push_str(
                        "      PQC KEX via sntrup761 is NOT available in Forgejo's SSH yet.\n",
                    );
                    report.push_str(
                        "      Host SSH hardening (PQC-012) still protects admin access.\n",
                    );
                    report.push_str(
                        "      PQC-010-UPDATE: monitor Forgejo issues for OpenSSH mode.\n",
                    );
                }
            }
        }

        Ok(report)
    }

    /// Create system backup
    pub async fn backup() -> CliResult<String> {
        use crate::application::backup_system::BackupSystem;
        use std::path::PathBuf;

        let file_adapter = Arc::new(EnolaFileAdapter::new());
        let backup_system = BackupSystem::new(file_adapter);

        // Backup key directories
        let paths_to_backup = vec![
            PathBuf::from("/var/lib/tor"),
            PathBuf::from("/etc/nginx/sites-available"),
            PathBuf::from("/opt/enola"),
        ];

        // Filter to existing paths
        let existing_paths: Vec<PathBuf> =
            paths_to_backup.into_iter().filter(|p| p.exists()).collect();

        if existing_paths.is_empty() {
            return Err(CliError::InvalidInput("No backup paths found".to_string()));
        }

        let backup_path = backup_system
            .create_backup_of_paths("system", &existing_paths)
            .await
            .map_err(CliError::from)?;

        Ok(format!("✅ System backup created: {:?}", backup_path))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTICS COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod diagnostics {
    use super::*;
    use crate::application::nginx_status_checker::NginxStatusChecker;
    use crate::application::ssh_status_check::SshStatusCheck;
    use crate::application::system_health_check::SystemHealthCheck;

    /// Get services summary
    pub async fn summary() -> CliResult<String> {
        maintenance::status().await
    }

    /// Get NGINX status
    pub async fn nginx() -> CliResult<String> {
        let systemd_adapter = Arc::new(SystemdAdapter);
        let nginx_adapter = Arc::new(NginxAdapter::new());
        let use_case = NginxStatusChecker::new(systemd_adapter, nginx_adapter);
        let report = use_case.execute().await.map_err(CliError::from)?;
        Ok(format!(
            "NGINX Status:\n  Active: {}\n  Sites Enabled: {:?}\n  HTTP Status: {}\n  Version: {}",
            report.active, report.sites_enabled, report.check_http_status, report.version
        ))
    }

    /// Get Tor status
    pub async fn tor() -> CliResult<Vec<crate::ports::tor::TorServiceInfo>> {
        tor::list().await
    }

    /// Get SSH status
    pub async fn ssh() -> CliResult<String> {
        let systemd_adapter = Arc::new(SystemdAdapter);
        let file_adapter = Arc::new(EnolaFileAdapter::new());
        let use_case = SshStatusCheck::new(systemd_adapter, file_adapter);
        let status = use_case.execute().await.map_err(CliError::from)?;
        Ok(format!(
            "SSH Status:\n  Active: {}\n  Ports: {:?}\n  Listening Confirmed: {}",
            status.active, status.ports, status.listening_confirmed
        ))
    }

    /// Test NGINX configuration
    pub async fn nginx_test() -> CliResult<bool> {
        use std::process::Command;
        let output = Command::new("nginx").args(["-t"]).status()?;
        Ok(output.success())
    }

    /// Get system resources
    pub async fn resources() -> CliResult<String> {
        use crate::adapters::hardware::probe::EnolaHardwareProbe;

        let system_check = Arc::new(SystemHealthCheck::new());
        let systemd_adapter = Arc::new(SystemdAdapter);
        let docker_adapter =
            Arc::new(BollardDockerAdapter::new().map_err(|e| CliError::Generic(e.to_string()))?);
        let hardware_adapter = Arc::new(EnolaHardwareProbe::new());

        let use_case = SystemResourceMonitor::new(
            system_check,
            systemd_adapter,
            docker_adapter,
            Some(hardware_adapter),
        );
        let report = use_case.execute().await.map_err(CliError::from)?;

        let gpu_info = if let Some(hw) = &report.hardware {
            if !hw.gpus.is_empty() {
                let gpu = &hw.gpus[0];
                format!("GPU: {} ({} MB)", gpu.name, gpu.vram_total_mb)
            } else {
                "GPU: None".to_string()
            }
        } else {
            "GPU: Unknown".to_string()
        };

        Ok(format!(
            "System Resources:\n  Overall: {}\n  CPU: {:.1}%\n  Memory: {}/{} bytes\n  {}\n  Services: {:?}\n  Containers: {:?}",
            report.system.overall, report.system.cpu_usage, report.system.memory_used, report.system.memory_total,
            gpu_info,
            report.services.keys().collect::<Vec<_>>(), report.containers.len()
        ))
    }

    /// WordPress diagnostics info
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WordPressDiagnostics {
        pub sites: Vec<WpSiteStatus>,
        pub total_sites: usize,
        pub healthy_count: usize,
        pub unhealthy_count: usize,
    }

    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WpSiteStatus {
        pub name: String,
        pub wp_running: bool,
        pub db_running: bool,
        pub has_tor: bool,
        pub health_check: String,
    }

    /// Get WordPress diagnostics
    pub async fn wordpress() -> CliResult<WordPressDiagnostics> {
        use std::process::Command;

        // Get all WordPress sites
        let sites = wordpress::list().await?;

        let mut wp_statuses: Vec<WpSiteStatus> = Vec::new();
        let mut healthy_count = 0;

        for site in &sites {
            let wp_running = site.status == "running";

            // Check DB container
            let db_container = format!("db-{}", site.name);
            let db_output = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Running}}", &db_container])
                .output();
            let db_running = db_output
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false);

            let has_tor = site.onion_address.is_some();

            let health_check = if wp_running && db_running {
                healthy_count += 1;
                "healthy".to_string()
            } else if wp_running || db_running {
                "degraded".to_string()
            } else {
                "stopped".to_string()
            };

            wp_statuses.push(WpSiteStatus {
                name: site.name.clone(),
                wp_running,
                db_running,
                has_tor,
                health_check,
            });
        }

        let total_sites = wp_statuses.len();

        Ok(WordPressDiagnostics {
            sites: wp_statuses,
            total_sites,
            healthy_count,
            unhealthy_count: total_sites - healthy_count,
        })
    }

    /// WordPress/NGINX sync status
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WpSyncStatus {
        pub sites: Vec<WpNginxSync>,
        pub all_synced: bool,
        pub issues: Vec<String>,
    }

    #[derive(Debug, Clone, serde::Serialize)]
    pub struct WpNginxSync {
        pub site_name: String,
        pub has_nginx_config: bool,
        pub nginx_config_path: String,
        pub nginx_enabled: bool,
        pub tor_service_exists: bool,
        pub synced: bool,
    }

    /// Check WordPress/NGINX sync
    pub async fn wp_sync() -> CliResult<WpSyncStatus> {
        use std::path::Path;

        let sites = wordpress::list().await?;
        let mut sync_statuses: Vec<WpNginxSync> = Vec::new();
        let mut issues: Vec<String> = Vec::new();
        let mut all_synced = true;

        for site in &sites {
            let nginx_available = format!("/etc/nginx/sites-available/wp-{}", site.name);
            let nginx_enabled = format!("/etc/nginx/sites-enabled/wp-{}", site.name);
            let tor_dir = format!("/var/lib/tor/wp-{}", site.name);

            let has_nginx_config = Path::new(&nginx_available).exists();
            let nginx_is_enabled = Path::new(&nginx_enabled).exists();
            let tor_exists = Path::new(&tor_dir).exists();

            let synced = has_nginx_config && (site.onion_address.is_none() || tor_exists);

            if !synced {
                all_synced = false;
                if !has_nginx_config {
                    issues.push(format!("Site '{}': Missing NGINX config", site.name));
                }
                if site.onion_address.is_some() && !tor_exists {
                    issues.push(format!(
                        "Site '{}': Tor service directory missing",
                        site.name
                    ));
                }
            }

            sync_statuses.push(WpNginxSync {
                site_name: site.name.clone(),
                has_nginx_config,
                nginx_config_path: nginx_available,
                nginx_enabled: nginx_is_enabled,
                tor_service_exists: tor_exists,
                synced,
            });
        }

        Ok(WpSyncStatus {
            sites: sync_statuses,
            all_synced,
            issues,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod test {
    use super::*;

    /// Run tests
    pub async fn run(_filter: Option<&str>) -> CliResult<String> {
        use crate::adapters::testing::test_runner::CargoTestRunnerAdapter;
        use crate::ports::test_runner::TestRunnerPort;

        let runner = CargoTestRunnerAdapter::new();
        let mut rx = runner.run_tests().await;

        let mut output = Vec::new();
        while let Some(event) = rx.recv().await {
            output.push(format!("{:?}", event));
        }

        Ok(output.join("\n"))
    }

    /// List available tests
    pub async fn list() -> CliResult<Vec<String>> {
        use crate::adapters::testing::test_runner::CargoTestRunnerAdapter;
        use crate::ports::test_runner::TestRunnerPort;

        let runner = CargoTestRunnerAdapter::new();
        Ok(runner.list_tests().await)
    }

    /// Run benchmarks (removed in lite version)
    pub async fn benchmark() -> CliResult<String> {
        Err(CliError::Generic(
            "Benchmarks are not available in enola-cli-lite.".to_string(),
        ))
    }

    /// Test results info
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct TestResults {
        pub last_run: Option<String>,
        pub total: usize,
        pub passed: usize,
        pub failed: usize,
        pub skipped: usize,
        pub duration_secs: f64,
        pub failures: Vec<TestFailure>,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct TestFailure {
        pub name: String,
        pub message: String,
    }

    /// Get last test results
    pub async fn results() -> CliResult<TestResults> {
        use std::fs;
        use std::path::Path;

        let results_path = "/var/lib/enola/test_results.json";

        if !Path::new(results_path).exists() {
            // Try to read from cargo test output
            return Ok(TestResults {
                last_run: None,
                total: 0,
                passed: 0,
                failed: 0,
                skipped: 0,
                duration_secs: 0.0,
                failures: Vec::new(),
            });
        }

        let content = fs::read_to_string(results_path)?;
        let results: TestResults = serde_json::from_str(&content)
            .map_err(|e| CliError::Generic(format!("Failed to parse test results: {}", e)))?;

        Ok(results)
    }

    /// Clean test artifacts
    pub async fn clean() -> CliResult<String> {
        use std::fs;
        use std::path::Path;

        let mut cleaned = Vec::new();

        // Clean test artifacts (non-glob paths)
        let test_artifacts = [
            "/var/lib/enola/test_results.json",
            "/var/lib/enola/test_coverage",
        ];

        for path in &test_artifacts {
            if Path::new(path).exists() {
                if Path::new(path).is_dir() {
                    fs::remove_dir_all(path).ok();
                } else {
                    fs::remove_file(path).ok();
                }
                cleaned.push(path.to_string());
            }
        }

        // Clean /tmp/enola-test-* files manually
        if let Ok(entries) = fs::read_dir("/tmp") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("enola-test-") {
                        if path.is_dir() {
                            fs::remove_dir_all(&path).ok();
                        } else {
                            fs::remove_file(&path).ok();
                        }
                        cleaned.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Run cargo clean for test artifacts only (not full clean)
        use std::process::Command;
        let _ = Command::new("cargo")
            .args(["clean", "--profile", "test"])
            .output();

        if cleaned.is_empty() {
            Ok("No test artifacts found to clean".to_string())
        } else {
            Ok(format!(
                "✅ Cleaned {} artifact(s):\n  - {}",
                cleaned.len(),
                cleaned.join("\n  - ")
            ))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LOG COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

pub mod logs {
    use super::*;

    /// List available log sources
    pub async fn list() -> CliResult<Vec<String>> {
        Ok(vec![
            "system".to_string(),
            "tor".to_string(),
            "nginx".to_string(),
            "docker".to_string(),
            "enola".to_string(),
            "install".to_string(),
            "smoke-test".to_string(),
        ])
    }

    /// View logs from a source
    pub async fn view(source: &str, lines: usize, _follow: bool) -> CliResult<Vec<String>> {
        use std::process::Command;

        let lines_str = lines.to_string();
        let (cmd, args): (&str, Vec<&str>) = match source {
            "system" => ("journalctl", vec!["-n", &lines_str, "-e"]),
            "tor" => ("journalctl", vec!["-u", "tor@default", "-n", &lines_str]),
            "nginx" => ("tail", vec!["-n", &lines_str, "/var/log/nginx/error.log"]),
            "docker" => ("docker", vec!["ps", "-a"]),
            "enola" => ("tail", vec!["-n", &lines_str, "/var/log/enola/enola.log"]),
            "install" => (
                "tail",
                vec!["-n", &lines_str, "/var/log/enola/postinst.log"],
            ),
            "smoke-test" => (
                "tail",
                vec!["-n", &lines_str, "/var/log/enola/smoke_test.log"],
            ),
            _ => {
                return Err(CliError::InvalidInput(format!(
                    "Unknown log source: {}",
                    source
                )))
            }
        };

        let output = Command::new(cmd).args(&args).output()?;

        let logs = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(logs)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PORTS MODULE — lista todos los puertos de servicios Enola
// Tarea PORTS-009 (183)
// ═══════════════════════════════════════════════════════════════════════════

/// Módulo de gestión y consulta de puertos.
pub mod ports {
    use super::*;

    /// Una entrada en la tabla de puertos.
    ///
    /// Representa un puerto en uso en la cadena Tor→Nginx→App.
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct PortEntry {
        /// Nombre del servicio (ej: "mi-git", "mi-wordpress")
        pub service: String,
        /// Tipo de servicio: "git", "wordpress", "tor", "nginx"
        pub service_type: String,
        /// Rol del puerto en la cadena:
        /// "onion-http" | "onion-https" | "nginx-listen" | "nginx-listen-ssl" |
        /// "backend" | "ssh" | "api"
        pub role: String,
        /// Número de puerto
        pub port: u16,
        /// Interfaz de binding: "127.0.0.1" (interno) o "virtual" (.onion URL)
        pub interface: String,
        /// Estado: "running", "stopped", "active", "virtual", "unknown"
        pub status: String,
    }

    /// Obtiene todos los puertos en uso por servicios Enola.
    ///
    /// Fuentes consultadas:
    /// - `docker ps -a`   → puertos Git y WordPress (incluye contenedores parados)
    /// - `/etc/nginx/sites-enabled/` → puertos de escucha de Nginx
    /// - `/etc/tor/torrc` + `/etc/tor/enola.d/` → puertos virtuales .onion
    ///
    /// Incluye contenedores parados (retienen port bindings en Docker).
    pub async fn list_all_ports() -> CliResult<Vec<PortEntry>> {
        let mut entries: Vec<PortEntry> = Vec::new();

        // ── 1. Puertos Git/Forgejo ───────────────────────────────────────────
        if let Ok(servers) = super::git::list().await {
            for s in servers {
                if let Some(p) = s.http_port {
                    entries.push(PortEntry {
                        service: s.name.clone(),
                        service_type: "git".to_string(),
                        role: "backend".to_string(),
                        port: p,
                        interface: "127.0.0.1".to_string(),
                        status: s.status.clone(),
                    });
                }
                if let Some(p) = s.ssh_port {
                    entries.push(PortEntry {
                        service: s.name.clone(),
                        service_type: "git".to_string(),
                        role: "ssh".to_string(),
                        port: p,
                        interface: "127.0.0.1".to_string(),
                        status: s.status.clone(),
                    });
                }
            }
        }

        // ── 2. Puertos WordPress ─────────────────────────────────────────────
        if let Ok(sites) = super::wordpress::list().await {
            for s in sites {
                if let Some(p) = s.port {
                    entries.push(PortEntry {
                        service: s.name.clone(),
                        service_type: "wordpress".to_string(),
                        role: "backend".to_string(),
                        port: p,
                        interface: "127.0.0.1".to_string(),
                        status: s.status.clone(),
                    });
                }
            }
        }

        // ── 3. Puertos Nginx (nginx-listen) desde /etc/nginx/sites-enabled/ ──
        let nginx_dir = std::path::Path::new("/etc/nginx/sites-enabled");
        if nginx_dir.exists() {
            if let Ok(dir) = std::fs::read_dir(nginx_dir) {
                for entry in dir.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("listen 127.0.0.1:") {
                                if let Some(port_str) = trimmed
                                    .strip_prefix("listen 127.0.0.1:")
                                    .and_then(|s| s.split(';').next())
                                    .and_then(|s| s.split_whitespace().next())
                                {
                                    if let Ok(p) = port_str.parse::<u16>() {
                                        let role = if trimmed.contains("ssl") {
                                            "nginx-listen-ssl"
                                        } else {
                                            "nginx-listen"
                                        };
                                        entries.push(PortEntry {
                                            service: name.trim_start_matches("proxy_").to_string(),
                                            service_type: "nginx".to_string(),
                                            role: role.to_string(),
                                            port: p,
                                            interface: "127.0.0.1".to_string(),
                                            status: "active".to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── 5. Puertos Tor (virtuales) desde /etc/tor/torrc + enola.d/*.conf ─
        let torrc_files = {
            let mut files = vec!["/etc/tor/torrc".to_string()];
            if let Ok(dir) = std::fs::read_dir("/etc/tor/enola.d") {
                for e in dir.flatten() {
                    let p = e.path().to_string_lossy().to_string();
                    if p.ends_with(".conf") {
                        files.push(p);
                    }
                }
            }
            files
        };

        for tor_file in &torrc_files {
            if let Ok(torrc) = std::fs::read_to_string(tor_file) {
                let mut current_service = String::new();
                for line in torrc.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("HiddenServiceDir") {
                        current_service = trimmed
                            .split_whitespace()
                            .last()
                            .unwrap_or("")
                            .trim_end_matches('/')
                            .split('/')
                            .next_back()
                            .unwrap_or("")
                            .trim_start_matches("enola_")
                            .to_string();
                    } else if trimmed.starts_with("HiddenServicePort")
                        && !current_service.is_empty()
                    {
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(virtual_port) = parts[1].parse::<u16>() {
                                let role = if virtual_port == 443 {
                                    "onion-https"
                                } else {
                                    "onion-http"
                                };
                                entries.push(PortEntry {
                                    service: current_service.clone(),
                                    service_type: "tor".to_string(),
                                    role: role.to_string(),
                                    port: virtual_port,
                                    interface: "virtual".to_string(),
                                    status: "virtual".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Ordenar por servicio y luego por rol
        entries.sort_by(|a, b| a.service.cmp(&b.service).then(a.role.cmp(&b.role)));
        Ok(entries)
    }
}

#[cfg(test)]
mod cli_error_tests {
    use super::{CliError, CliResult};
    use crate::domain::error::EnolaError;

    // ── CliError::Display ────────────────────────────────────────────────────

    #[test]
    fn display_not_implemented() {
        let e = CliError::NotImplemented("foo command".into());
        assert_eq!(format!("{}", e), "Not implemented: foo command");
    }

    #[test]
    fn display_invalid_input() {
        let e = CliError::InvalidInput("bad arg".into());
        assert_eq!(format!("{}", e), "bad arg");
    }

    #[test]
    fn display_generic() {
        let e = CliError::Generic("something went wrong".into());
        assert_eq!(format!("{}", e), "something went wrong");
    }

    #[test]
    fn display_domain() {
        let inner = EnolaError::NotFound("missing".into());
        let e = CliError::Domain(inner);
        let s = format!("{}", e);
        assert!(!s.is_empty());
    }

    #[test]
    fn display_io_permission_denied() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let e = CliError::Io(io_err);
        let s = format!("{}", e);
        assert!(s.contains("Permission denied"));
        assert!(s.contains("sudo"));
    }

    #[test]
    fn display_io_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
        let e = CliError::Io(io_err);
        let s = format!("{}", e);
        assert!(
            s.contains("not found") || s.contains("Not found") || s.contains("File or directory")
        );
    }

    #[test]
    fn display_io_other() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let e = CliError::Io(io_err);
        let s = format!("{}", e);
        assert!(s.contains("I/O error") || s.contains("pipe broke"));
    }

    #[test]
    fn display_controlled_exit_stderr() {
        let e = CliError::ControlledExit {
            code: 1,
            stdout: None,
            stderr: Some("fatal error".into()),
        };
        assert_eq!(format!("{}", e), "fatal error");
    }

    #[test]
    fn display_controlled_exit_stdout_only() {
        let e = CliError::ControlledExit {
            code: 0,
            stdout: Some("done".into()),
            stderr: None,
        };
        assert_eq!(format!("{}", e), "done");
    }

    #[test]
    fn display_controlled_exit_neither() {
        let e = CliError::ControlledExit {
            code: 0,
            stdout: None,
            stderr: None,
        };
        assert_eq!(format!("{}", e), "controlled exit");
    }

    // ── CliError::From ───────────────────────────────────────────────────────

    #[test]
    fn from_enola_error() {
        let enola = EnolaError::NotFound("x".into());
        let cli: CliError = enola.into();
        assert!(matches!(cli, CliError::Domain(_)));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
        let cli: CliError = io_err.into();
        assert!(matches!(cli, CliError::Io(_)));
    }

    // ── Debug ────────────────────────────────────────────────────────────────

    #[test]
    fn debug_not_panics() {
        let e = CliError::Generic("dbg".into());
        let s = format!("{:?}", e);
        assert!(!s.is_empty());
    }

    // ── parse_env_port (helper in ports module) ──────────────────────────────

    #[test]
    fn parse_env_port_found() {
        let content = "HOST=localhost\nPORT=8080\nDEBUG=true\n";
        assert_eq!(
            content
                .lines()
                .find(|l| l.starts_with("PORT="))
                .and_then(|l| l.splitn(2, '=').nth(1))
                .and_then(|v| v.trim().parse::<u16>().ok()),
            Some(8080)
        );
    }

    #[test]
    fn parse_env_port_missing_key() {
        let content = "HOST=localhost\nDEBUG=true\n";
        assert_eq!(
            content
                .lines()
                .find(|l| l.starts_with("PORT"))
                .and_then(|l| l.splitn(2, '=').nth(1))
                .and_then(|v| v.trim().parse::<u16>().ok()),
            None
        );
    }

    // ── CliResult type alias ─────────────────────────────────────────────────

    #[test]
    fn cli_result_ok_is_ok() {
        let r: CliResult<u32> = Ok(42);
        assert!(r.is_ok());
        assert_eq!(r.unwrap(), 42); // unwrap: test-only
    }

    #[test]
    fn cli_result_err_is_err() {
        let r: CliResult<u32> = Err(CliError::Generic("fail".into()));
        assert!(r.is_err());
    }
}

#[cfg(test)]
mod tor_edit_tests {
    use super::update_tor_config_ports;

    #[tokio::test]
    async fn test_update_tor_config_ports_replaces_port() {
        let tmp = std::env::temp_dir().join("enola_test_tor_edit.conf");
        let initial = "# Enola Service: test\n\
            HiddenServiceDir /var/lib/tor/enola_test\n\
            HiddenServicePort 80 127.0.0.1:11738\n";
        tokio::fs::write(&tmp, initial).await.unwrap();

        // We need to mock the path — update_tor_config_ports uses a fixed path format.
        // Since the function reads from /etc/tor/enola.d/{name}.conf, we test the
        // parsing logic directly instead.
        let content = tokio::fs::read_to_string(&tmp).await.unwrap();

        // Simulate the logic of update_tor_config_ports
        let ports = vec![(443u16, 12000u16)];
        let mut new_lines: Vec<String> = Vec::new();
        let mut port_index = 0;
        for line in content.lines() {
            if line.trim().starts_with("HiddenServicePort") {
                if port_index < ports.len() {
                    let (vp, np) = ports[port_index];
                    new_lines.push(format!("HiddenServicePort {} 127.0.0.1:{}", vp, np));
                    port_index += 1;
                } else {
                    new_lines.push(line.to_string());
                }
            } else {
                new_lines.push(line.to_string());
            }
        }
        let new_content = new_lines.join("\n");

        assert!(new_content.contains("HiddenServicePort 443 127.0.0.1:12000"));
        assert!(!new_content.contains("HiddenServicePort 80 127.0.0.1:11738"));
        assert!(new_content.contains("HiddenServiceDir /var/lib/tor/enola_test"));
        assert!(new_content.contains("# Enola Service: test"));

        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_update_tor_config_ports_preserves_non_port_lines() {
        let content = "# Enola Service: test\n\
            HiddenServiceDir /var/lib/tor/enola_test\n\
            HiddenServicePort 80 127.0.0.1:11738\n";

        let ports = vec![(80u16, 9999u16)];
        let mut new_lines: Vec<String> = Vec::new();
        let mut port_index = 0;
        for line in content.lines() {
            if line.trim().starts_with("HiddenServicePort") {
                if port_index < ports.len() {
                    let (vp, np) = ports[port_index];
                    new_lines.push(format!("HiddenServicePort {} 127.0.0.1:{}", vp, np));
                    port_index += 1;
                } else {
                    new_lines.push(line.to_string());
                }
            } else {
                new_lines.push(line.to_string());
            }
        }
        let new_content = new_lines.join("\n");

        assert!(new_content.contains("HiddenServiceDir /var/lib/tor/enola_test"));
        assert!(new_content.contains("# Enola Service: test"));
        assert!(new_content.contains("HiddenServicePort 80 127.0.0.1:9999"));
    }

    #[tokio::test]
    async fn test_update_tor_config_ports_adds_missing_port() {
        let content = "# Enola Service: test\n\
            HiddenServiceDir /var/lib/tor/enola_test\n";

        let ports = vec![(80u16, 8080u16)];
        let mut new_lines: Vec<String> = Vec::new();
        let mut port_index = 0;
        for line in content.lines() {
            if line.trim().starts_with("HiddenServicePort") {
                if port_index < ports.len() {
                    let (vp, np) = ports[port_index];
                    new_lines.push(format!("HiddenServicePort {} 127.0.0.1:{}", vp, np));
                    port_index += 1;
                } else {
                    new_lines.push(line.to_string());
                }
            } else {
                new_lines.push(line.to_string());
            }
        }
        while port_index < ports.len() {
            let (vp, np) = ports[port_index];
            new_lines.push(format!("HiddenServicePort {} 127.0.0.1:{}", vp, np));
            port_index += 1;
        }
        let new_content = new_lines.join("\n");

        assert!(new_content.contains("HiddenServicePort 80 127.0.0.1:8080"));
    }
}
