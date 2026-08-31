// CLI Executor Module
// Connects clap commands with the command implementations

use crate::cli::commands::{self, CliError, CliResult};
use crate::cli::{
    AppArmorCommands, Cli, Commands, DiagnosticsCommands, DocsCommands, DrupalCommands,
    FileCommands, FirewallCommands, GhostCommands, GitCommands, GitUserCommands, LogCommands,
    MagnoliaCommands, MaintenanceCommands, PortsCommands, StrapiCommands, TestCommands,
    TorAuthCommands, TorCommands, VpnCommands, VpnPeerCommands, WagtailCommands, WordPressCommands,
};
use std::sync::Arc;

use crate::adapters::infra::dependencies::SystemDependencyAdapter;
use crate::adapters::infra::manifest::FileManifestAdapter;
use crate::application::dependency_manager::DependencyManager;
use crate::domain::dependencies::{SetupScope, ALL_DEPENDENCIES};
use crate::ports::dependencies::DependencyPort;
use crate::ports::manifest::ManifestPort;

/// Execute a CLI command and return the result
pub async fn execute(cli: Cli) -> CliResult<String> {
    // ── LIC-002: License Acceptance Guard ──
    // Must accept proprietary license before ANY command (except --help/--version).
    // Persists acceptance in ~/.enola/license_accepted.json.
    check_license_acceptance()?;

    // ── PRE-FLIGHT: instalar dependencias del sistema si faltan ───────────────
    maybe_run_preflight(&cli).await?;

    match cli.command {
        Commands::Tor(cmd) => execute_tor(cmd, cli.format.as_str()).await,
        Commands::Git(cmd) => execute_git(cmd, cli.format.as_str()).await,
        Commands::Wp(cmd) => execute_wordpress(cmd, cli.format.as_str()).await,
        Commands::Drupal(cmd) => execute_drupal(cmd, cli.format.as_str()).await,
        Commands::Ghost(cmd) => execute_ghost(cmd, cli.format.as_str()).await,
        Commands::Magnolia(cmd) => execute_magnolia(cmd, cli.format.as_str()).await,
        Commands::Strapi(cmd) => execute_strapi(cmd, cli.format.as_str()).await,
        Commands::Wagtail(cmd) => execute_wagtail(cmd, cli.format.as_str()).await,
        Commands::Files(cmd) => execute_files(cmd, cli.format.as_str()).await,
        Commands::Maintenance(cmd) => execute_maintenance(cmd, cli.format.as_str()).await,
        Commands::Diag(cmd) => execute_diagnostics(cmd, cli.format.as_str()).await,
        Commands::Test(cmd) => execute_test(cmd, cli.format.as_str()).await,
        Commands::Logs(cmd) => execute_logs(cmd, cli.format.as_str()).await,
        Commands::Ports(cmd) => execute_ports(cmd, cli.format.as_str()).await,
        Commands::Firewall(cmd) => execute_firewall(cmd).await,
        Commands::Apparmor(cmd) => execute_apparmor(cmd).await,
        Commands::Vpn(cmd) => execute_vpn(cmd).await,
        Commands::Setup {
            all,
            vpn,
            security,
            pqc_tls,
        } => execute_setup(all, vpn, security, pqc_tls).await,
        Commands::Doctor { security } => execute_doctor(security).await,
        Commands::Quickref => execute_quickref().await,
        Commands::License => execute_license().await,
        Commands::Uninstall {
            yes,
            keep_data,
            only,
            force,
            remove_deps,
        } => execute_uninstall(yes, keep_data, only, force, remove_deps).await,
        Commands::ConfigShow { json } => {
            crate::application::config_inspector::show(json)
                .map_err(|e| crate::cli::commands::CliError::Generic(format!("{}", e)))?;
            Ok(String::new())
        }
        Commands::ConfigValidate { reachable, json } => {
            crate::application::config_inspector::validate(reachable, json)
                .map_err(|e| crate::cli::commands::CliError::Generic(format!("{}", e)))?;
            Ok(String::new())
        }
        Commands::Docs(cmd) => execute_docs(cmd).await,
        Commands::Update(cmd) => execute_update(cmd).await,
        Commands::Verify {
            file,
            pqsig,
            pubkey,
            json,
        } => execute_verify(file, pqsig, pubkey, json).await,
        Commands::Web { port } => {
            crate::application::web_server::start_server(port)
                .await
                .map_err(|e| CliError::Generic(format!("web server error: {}", e)))?;
            Ok(String::new())
        }
    }
}

/// RELEASE-VERIFY (PQC-030): verifica la autenticidad de un release descargado
/// usando la clave pública ML-DSA-65 embebida. Exit code 21 si falla.
async fn execute_verify(
    file: String,
    pqsig: Option<String>,
    pubkey: Option<String>,
    json: bool,
) -> CliResult<String> {
    let report =
        crate::application::release_verify::run(&file, pqsig.as_deref(), pubkey.as_deref());
    let rendered = if json {
        serde_json::to_string_pretty(&report.json_value())
            .map_err(|e| CliError::Generic(format!("failed to render verify JSON: {}", e)))?
    } else {
        report.human_summary()
    };
    if report.success() {
        Ok(rendered)
    } else {
        Err(CliError::ControlledExit {
            code: 21,
            stdout: None,
            stderr: Some(rendered),
        })
    }
}

/// Determina si el comando seleccionado requiere dependencias de sistema
fn command_requires_system_dependencies(cmd: &Commands) -> bool {
    match cmd {
        // Exentos: no requieren dependencias instaladas previamente
        Commands::Quickref
        | Commands::License
        | Commands::Doctor { .. }
        | Commands::Setup { .. }
        | Commands::Uninstall { .. }
        | Commands::ConfigShow { .. }
        | Commands::ConfigValidate { .. }
        | Commands::Docs(_)
        | Commands::Verify { .. }
        | Commands::Update(_) => false,

        // El resto de familias usan Docker/Nginx/Tor/UFW/AppArmor/WireGuard
        _ => true,
    }
}

/// Devuelve true si stdin es interactivo (TTY)
fn is_tty() -> bool {
    #[allow(unsafe_code)]
    unsafe {
        libc::isatty(libc::STDIN_FILENO) != 0
    }
}

/// Devuelve true si el proceso corre como root
fn is_root() -> bool {
    #[allow(unsafe_code)]
    unsafe {
        libc::geteuid() == 0
    }
}

/// Preflight: si faltan dependencias y el comando las requiere,
/// ofrece ejecutar `setup --all` automáticamente.
async fn maybe_run_preflight(cli: &Cli) -> CliResult<()> {
    if !command_requires_system_dependencies(&cli.command) {
        return Ok(());
    }

    let dep = Arc::new(SystemDependencyAdapter::new());
    let mgr = DependencyManager::new(dep.clone());
    let statuses = dep.check_all(&ALL_DEPENDENCIES.iter().collect::<Vec<_>>());
    let missing_cnt = statuses.iter().filter(|s| !s.installed).count();
    if missing_cnt == 0 {
        return Ok(());
    }

    // Mostrar reporte resumido
    let report = mgr.doctor();
    eprintln!("{}", report);

    if !is_root() {
        // No somos root: no intentamos instalar automáticamente
        return Err(CliError::Generic(
            "Faltan dependencias del sistema. Ejecute:\n  sudo enola-cli setup --all\n".to_string(),
        ));
    }

    if !is_tty() {
        // No interactivo: intentar instalar directamente sin prompt
        let result = mgr
            .setup(SetupScope::All)
            .map_err(|e| CliError::Generic(format!("Fallo instalando dependencias: {}", e)))?;
        eprintln!("{}", mgr.format_setup_result(&result));
        return Ok(());
    }

    // Interactivo: pedir confirmación
    eprint!(
        "\nSe detectaron dependencias faltantes. ¿Desea instalarlas ahora con 'setup --all'? [Y/n]: "
    );
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        let ans = input.trim().to_lowercase();
        if ans.is_empty() || ans == "y" || ans == "yes" || ans == "s" || ans == "si" {
            let result = mgr
                .setup(SetupScope::All)
                .map_err(|e| CliError::Generic(format!("Fallo instalando dependencias: {}", e)))?;
            eprintln!("{}", mgr.format_setup_result(&result));
        } else {
            return Err(CliError::Generic(
                "Operación cancelada. Instale dependencias con:\n  sudo enola-cli setup --all\n"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// LIC-002: LICENSE ACCEPTANCE (first-run gate)
// ═══════════════════════════════════════════════════════════════════════════

/// Verifica que el usuario ha aceptado la licencia propietaria.
/// Si no existe `~/.enola/license_accepted.json` o la versión/hash cambió,
/// muestra el resumen de licencia y pide aceptación interactiva.
/// `--version` y `--help` ya se manejan por clap antes de llegar aquí.
fn check_license_acceptance() -> CliResult<()> {
    use crate::domain::license_acceptance::{
        LicenseAcceptance, LICENSE_HASH, LICENSE_SUMMARY_ES, LICENSE_VERSION,
    };

    // ── Bypass para tests E2E ──
    #[cfg(feature = "testing")]
    {
        if crate::infrastructure::test_token::verify_test_token_from_env() {
            return Ok(());
        }
    }

    let acceptance_path = match dirs::home_dir() {
        Some(home) => home.join(".enola").join("license_accepted.json"),
        None => {
            return Err(CliError::Generic(
                "Cannot determine home directory to check license acceptance.".to_string(),
            ));
        }
    };

    // If already accepted for current license version → OK
    if acceptance_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(&acceptance_path) {
            if let Ok(acceptance) = serde_json::from_str::<LicenseAcceptance>(&contents) {
                if acceptance.is_valid_for_current() {
                    return Ok(());
                }
                // License version changed → re-acceptance needed
                eprintln!(
                    "\x1b[1;33m⚠️  La licencia ha sido actualizada (v{} → v{}). Se requiere nueva aceptación.\x1b[0m\n",
                    acceptance.license_version, LICENSE_VERSION
                );
            }
        }
    }

    // Show license summary
    eprintln!("{}", LICENSE_SUMMARY_ES);
    eprintln!();

    // Ask for acceptance interactively
    eprintln!("  ¿Acepta la licencia y la política de uso?");

    // ISSUE-001 fix: allow non-interactive acceptance via env var.
    if let Ok(env_accept) = std::env::var("ENOLA_ACCEPT_LICENSE") {
        let env_accept = env_accept.trim().to_lowercase();
        if env_accept == "acepto" || env_accept == "i accept" {
            eprintln!("  ✅ Licencia aceptada via ENOLA_ACCEPT_LICENSE.");
            let acceptance = LicenseAcceptance {
                accepted: true,
                timestamp: chrono::Utc::now().to_rfc3339(),
                license_version: LICENSE_VERSION.to_string(),
                license_hash: LICENSE_HASH.to_string(),
            };
            if let Some(parent) = acceptance_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let json = serde_json::to_string_pretty(&acceptance).unwrap_or_default();
            // LOW-04: Use atomic write with 0600 permissions from the start.
            let _ = crate::infrastructure::atomic_secret_file::write_secret_atomically(
                &acceptance_path,
                json.as_bytes(),
            );
            return Ok(());
        }
    }

    eprint!("  Escriba '\x1b[1;32macepto\x1b[0m' / '\x1b[1;32mI accept\x1b[0m': ");

    // Check if stdin is a terminal (interactive)
    let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } != 0;
    if !is_tty {
        return Err(CliError::Generic(
            "License acceptance requires an interactive terminal.\n\
             Run the CLI interactively to accept the license on first use,\n\
             or set ENOLA_ACCEPT_LICENSE=acepto for non-interactive use."
                .to_string(),
        ));
    }

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| CliError::Generic(format!("Failed to read input: {}", e)))?;

    let input = input.trim().to_lowercase();
    if input != "acepto" && input != "i accept" {
        return Err(CliError::Generic(
            "Licencia no aceptada. El CLI no puede ejecutarse sin aceptar la licencia.\n\
             Ejecute de nuevo y escriba 'acepto' o 'I accept'."
                .to_string(),
        ));
    }

    // Persist acceptance
    let acceptance = LicenseAcceptance {
        accepted: true,
        timestamp: chrono::Utc::now().to_rfc3339(),
        license_version: LICENSE_VERSION.to_string(),
        license_hash: LICENSE_HASH.to_string(),
    };

    // Ensure ~/.enola/ exists
    if let Some(parent) = acceptance_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "\x1b[1;33m⚠️  No se pudo crear {}: {}\x1b[0m",
                parent.display(),
                e
            );
        }
    }

    match serde_json::to_string_pretty(&acceptance) {
        Ok(json) => {
            // LOW-04: Use atomic write with 0600 permissions from the start.
            if let Err(e) = crate::infrastructure::atomic_secret_file::write_secret_atomically(
                &acceptance_path,
                json.as_bytes(),
            ) {
                eprintln!(
                    "\x1b[1;33m⚠️  No se pudo guardar la aceptación en {}: {}\x1b[0m",
                    acceptance_path.display(),
                    e
                );
            } else {
                eprintln!(
                    "\n  \x1b[1;32m✅ Licencia aceptada.\x1b[0m Guardada en {}\n",
                    acceptance_path.display()
                );
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;33m⚠️  Error al serializar aceptación: {}\x1b[0m", e);
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// TOR EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_tor(cmd: TorCommands, format: &str) -> CliResult<String> {
    match cmd {
        TorCommands::List => {
            let services = commands::tor::list().await?;
            if format == "json" {
                return format_output(&services, format);
            }
            if services.is_empty() {
                return Ok("No Tor hidden services found.".to_string());
            }
            let mut out = format!("🧅 Tor Hidden Services ({})\n", services.len());
            out.push_str("═══════════════════════════════════════════════════\n");
            for svc in &services {
                out.push_str(&format!("{}\n", svc));
                out.push_str("───────────────────────────────────────────────────\n");
            }
            Ok(out)
        }
        TorCommands::Create {
            name,
            service_type,
            virtual_port,
            target_port,
            ssl,
        } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let hostname =
                commands::tor::create(&name, &service_type, virtual_port, target_port, ssl).await?;
            // FW-001: Registrar target_port en UFW (virtual_port es .onion, no socket real)
            if let Some(tp) = target_port {
                sync_firewall_ports(&[tp]);
            }
            Ok(format!(
                "✅ Service '{}' created successfully!\n🧅 Hostname: {}",
                name, hostname
            ))
        }
        TorCommands::Start { name } => {
            commands::tor::start(&name).await?;
            Ok(format!("✅ Service '{}' started", name))
        }
        TorCommands::Stop { name } => {
            commands::tor::stop(&name).await?;
            Ok(format!("✅ Service '{}' stopped", name))
        }
        TorCommands::Remove { name, force } => {
            if !force {
                eprintln!(
                    "⚠️  This will permanently delete the service '{}' and its .onion address.",
                    name
                );
                eprintln!("Use --force to skip this confirmation.");
                return Err(CliError::InvalidInput(
                    "Confirmation required. Use --force to skip.".to_string(),
                ));
            }
            // FW-003: Extraer puertos ANTES de eliminar para poder limpiar UFW
            let ports = extract_service_ports(&name);
            commands::tor::remove(&name).await?;
            if !ports.is_empty() {
                sync_firewall_cleanup(&ports);
            }
            Ok(format!("✅ Service '{}' removed", name))
        }
        TorCommands::Edit {
            name,
            virtual_port,
            nginx_port,
            target_port,
            auto_ports,
        } => {
            let result =
                commands::tor::edit(&name, virtual_port, nginx_port, target_port, auto_ports)
                    .await?;
            // FW-002: Registrar nuevos puertos internos en UFW si se editaron
            let mut ports_to_sync = Vec::new();
            if let Some(p) = nginx_port {
                ports_to_sync.push(p);
            }
            if let Some(p) = target_port {
                ports_to_sync.push(p);
            }
            if !ports_to_sync.is_empty() {
                sync_firewall_ports(&ports_to_sync);
            }
            Ok(result)
        }
        TorCommands::Rotate { name } => {
            let new_hostname = commands::tor::rotate(&name).await?;
            Ok(format!(
                "✅ Identity rotated for '{}'\n🧅 New hostname: {}",
                name, new_hostname
            ))
        }
        TorCommands::Auth(auth_cmd) => execute_tor_auth(auth_cmd, format).await,
    }
}

async fn execute_tor_auth(cmd: TorAuthCommands, format: &str) -> CliResult<String> {
    match cmd {
        TorAuthCommands::List { service } => {
            let clients = commands::tor::auth::list(&service).await?;
            if clients.is_empty() {
                Ok(format!("No authorized clients for service '{}'", service))
            } else {
                format_output(&clients, format)
            }
        }
        TorAuthCommands::Enable { service } => {
            commands::tor::auth::enable(&service).await?;
            Ok(format!("✅ Client authorization enabled for '{}'", service))
        }
        TorAuthCommands::Disable { service } => {
            commands::tor::auth::disable(&service).await?;
            Ok(format!(
                "✅ Client authorization disabled for '{}'",
                service
            ))
        }
        TorAuthCommands::Add {
            service,
            client,
            pubkey,
        } => {
            commands::tor::auth::add(&service, &client, &pubkey).await?;
            Ok(format!(
                "✅ Client '{}' added to service '{}'",
                client, service
            ))
        }
        TorAuthCommands::Revoke { service, client } => {
            commands::tor::auth::revoke(&service, &client).await?;
            Ok(format!(
                "✅ Client '{}' revoked from service '{}'",
                client, service
            ))
        }
        TorAuthCommands::Generate { client } => {
            // Modelo GitHub/GitLab: el CLIENTE genera su propio par de claves.
            // La privada NUNCA sale de su equipo. Solo envía la pública al operador.
            let (pubkey, privkey) = commands::tor::auth::generate(&client).await?;
            // PQC-014: Advertencia post-cuántica — X25519 no es resistente a Shor
            Ok(format!(
                "🔐 Generated keypair for client '{}'\n\n\
                 ── HOW THIS WORKS (like SSH keys for GitHub/GitLab) ──\n\
                 You generated this keypair on YOUR machine.\n\
                 1. Keep the PRIVATE key — import it in your Tor Browser.\n\
                 2. Send the PUBLIC key to the service operator (via Signal, PGP, etc.)\n\
                 3. The operator adds you with: enola-cli tor auth add <service> --client {} --pubkey <key>\n\
                 4. Once added, you can access the .onion service in Tor Browser.\n\n\
                 📤 PUBLIC KEY (send this to the service operator):\n{}\n\n\
                 📥 PRIVATE KEY (import in Tor Browser → Onion Services → Client auth):\n{}\n\n\
                 ⚠️  NEVER share your private key! Save it now — it won't be shown again.\n\
                 ⚠️  The private key never leaves your machine. Only the public key is shared.\n\n\
                 🔬 QUANTUM SECURITY NOTE:\n\
                    These keys use X25519 (Curve25519), which is NOT resistant to quantum\n\
                    computers (Shor's algorithm). Mitigations:\n\
                    • Rotate keys periodically: enola-cli tor auth rotate <service-name> --client {}\n\
                    • The Tor Project is working on post-quantum auth (ML-KEM). Update when available.\n\
                    • See PQC documentation",
                client, client, pubkey, privkey, client
            ))
        }
        // PQC-013: Rotar claves X25519 — el cliente genera nuevas claves
        // El operador actualiza solo la pública en el servidor
        // Mitiga HNDL reduciendo la vida útil de cada par de claves
        TorAuthCommands::Rotate { service, client } => {
            let (pubkey, privkey) = commands::tor::auth::generate(&client).await?;
            // Revocar la clave antigua y añadir la nueva en una operación atómica
            commands::tor::auth::revoke(&service, &client).await?;
            commands::tor::auth::add(&service, &client, &pubkey).await?;
            Ok(format!(
                "🔄 Keypair rotated for client '{}' on service '{}'\n\n\
                 ── ROTATION FLOW ──\n\
                 New keys were generated. The server has been updated with the new public key.\n\
                 The client MUST import the new private key in their Tor Browser.\n\n\
                 📤 NEW PUBLIC KEY (already updated on server):\n{}\n\n\
                 📥 NEW PRIVATE KEY (send to client — they import in Tor Browser):\n{}\n\n\
                 ⚠️  The old private key is now invalid.\n\
                 ⚠️  Send the private key to the client via a secure channel (Signal, PGP, etc.)\n\n\
                 🔬 QUANTUM SECURITY: Regular rotation reduces the HNDL attack window.\n\
                    Recommended rotation frequency: every 90 days.\n\
                    Future: will migrate to ML-KEM when Tor supports post-quantum auth (PQC-043).",
                client, service, pubkey, privkey
            ))
        }
    }
}

// ─── UFW-010: Aviso si firewall no está activo al crear servicios ────────────
/// Imprime un warning (no bloqueante) si UFW está instalado pero no activo.
fn print_ufw_warning() {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    use std::sync::Arc;
    let mgr = FirewallManager::new(Arc::new(UfwAdapter::new()));
    if let Some(warn) = mgr.inactive_warning() {
        eprintln!("{}", warn);
    }
}

// ─── AA-011: Aviso si AppArmor no está activo al crear servicios ────────────
/// Imprime un warning (no bloqueante) si AppArmor no está disponible.
fn print_apparmor_warning() {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use std::sync::Arc;
    let mgr = AppArmorManager::new(Arc::new(AppArmorAdapter::new()));
    if let Some(warn) = mgr.inactive_warning() {
        eprintln!("{}", warn);
    }
}

// ─── Sincronización UFW ↔ servicios (FW-001/FW-002/FW-003) ────────────────
// Cuando se crea, edita o elimina un servicio, los puertos internos
// (127.0.0.1) deben registrarse/actualizarse/limpiarse en UFW para que
// la cadena Tor→Nginx→Docker no sea bloqueada por `default deny incoming`.

/// Registra puertos internos en UFW (solo si UFW está activo).
/// Se llama después de crear un servicio exitosamente.
fn sync_firewall_ports(ports: &[u16]) {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::ports::firewall::FirewallPort;
    use crate::ports::manifest::ManifestPort;
    let ufw = UfwAdapter::new();
    if !ufw.is_active().unwrap_or(false) {
        return;
    }
    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    for &port in ports {
        if port == 0 {
            continue;
        }
        match ufw.allow_loopback_port(port) {
            Ok(()) => {
                eprintln!("🛡 Firewall: puerto {}/tcp permitido (loopback)", port);
                let _ = manifest.append("ufw_rule", &port.to_string());
            }
            Err(e) => eprintln!("⚠  Firewall: no se pudo permitir puerto {}: {}", port, e),
        }
    }
}

/// Actualiza un puerto en UFW: elimina el antiguo, permite el nuevo.
/// Se llama después de editar un servicio que cambia de puerto.
#[allow(dead_code)]
fn sync_firewall_port_change(old_port: u16, new_port: u16) {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::ports::firewall::FirewallPort;
    use crate::ports::manifest::ManifestPort;
    let ufw = UfwAdapter::new();
    if !ufw.is_active().unwrap_or(false) {
        return;
    }
    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    if old_port != 0 {
        let _ = ufw.remove_loopback_rule(old_port);
        let _ = manifest.remove("ufw_rule", &old_port.to_string());
    }
    if new_port != 0 {
        match ufw.allow_loopback_port(new_port) {
            Ok(()) => {
                eprintln!("🛡 Firewall: puerto {}/tcp actualizado (loopback)", new_port);
                let _ = manifest.append("ufw_rule", &new_port.to_string());
            }
            Err(e) => eprintln!(
                "⚠  Firewall: no se pudo permitir puerto {}: {}",
                new_port, e
            ),
        }
    }
}

/// Limpia reglas UFW de puertos que ya no se usan.
/// Se llama ANTES de eliminar un servicio (para obtener los puertos desde Nginx config).
fn sync_firewall_cleanup(ports: &[u16]) {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::ports::firewall::FirewallPort;
    use crate::ports::manifest::ManifestPort;
    let ufw = UfwAdapter::new();
    if !ufw.is_active().unwrap_or(false) {
        return;
    }
    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    for &port in ports {
        if port == 0 {
            continue;
        }
        let _ = ufw.remove_loopback_rule(port);
        let _ = manifest.remove("ufw_rule", &port.to_string());
        eprintln!("🛡 Firewall: regla para puerto {}/tcp eliminada", port);
    }
}

/// VPN-001: Permite tráfico UDP en un puerto VPN (regla pública, no solo loopback).
/// A diferencia de los servicios internos (Tor→Nginx→Docker) que usan loopback,
/// WireGuard necesita recibir tráfico UDP desde Internet.
fn sync_vpn_firewall_allow(port: u16) {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::domain::firewall::FirewallProtocol;
    use crate::ports::firewall::FirewallPort;
    use crate::ports::manifest::ManifestPort;
    let ufw = UfwAdapter::new();
    if !ufw.is_active().unwrap_or(false) {
        return;
    }
    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    match ufw.allow_port(port, FirewallProtocol::Udp, None) {
        Ok(()) => {
            eprintln!("🛡 Firewall: puerto {}/udp permitido (VPN)", port);
            let _ = manifest.append("ufw_rule", &port.to_string());
        }
        Err(e) => eprintln!(
            "⚠  Firewall: no se pudo permitir puerto {}/udp: {}",
            port, e
        ),
    }
}

/// VPN-001: Elimina regla UFW para un puerto UDP de VPN.
fn sync_vpn_firewall_remove(port: u16) {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::domain::firewall::FirewallProtocol;
    use crate::ports::firewall::FirewallPort;
    use crate::ports::manifest::ManifestPort;
    let ufw = UfwAdapter::new();
    if !ufw.is_active().unwrap_or(false) {
        return;
    }
    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    match ufw.deny_port(port, FirewallProtocol::Udp) {
        Ok(()) => {
            eprintln!("🛡 Firewall: regla {}/udp eliminada (VPN)", port);
            let _ = manifest.remove("ufw_rule", &port.to_string());
        }
        Err(e) => eprintln!("⚠  Firewall: no se pudo eliminar regla {}/udp: {}", port, e),
    }
}

/// Extrae puertos (listen + proxy_pass) de las configs Nginx de un servicio.
/// Se llama ANTES de eliminar el servicio, para poder limpiar UFW después.
fn extract_service_ports(service_name: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    let candidates = [
        format!("/etc/nginx/sites-available/proxy_{}", service_name),
        format!("/etc/nginx/sites-available/{}", service_name),
        format!("/etc/nginx/sites-available/{}.conf", service_name),
        format!("/etc/nginx/sites-available/proxy_{}.conf", service_name),
        format!("/etc/nginx/sites-available/fileserver_{}", service_name),
    ];
    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                // Extract listen port: "listen 127.0.0.1:PORT;" or "listen 127.0.0.1:PORT ssl;"
                if trimmed.starts_with("listen") && trimmed.contains("127.0.0.1:") {
                    if let Some(port_str) = trimmed.split("127.0.0.1:").nth(1) {
                        if let Some(p) = port_str.split(|c: char| !c.is_ascii_digit()).next() {
                            if let Ok(port) = p.parse::<u16>() {
                                if !ports.contains(&port) {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
                // Extract backend port: "proxy_pass http://127.0.0.1:PORT;"
                if trimmed.starts_with("proxy_pass") && trimmed.contains("127.0.0.1:") {
                    if let Some(port_str) = trimmed.split("127.0.0.1:").nth(1) {
                        if let Some(p) = port_str.split(|c: char| !c.is_ascii_digit()).next() {
                            if let Ok(port) = p.parse::<u16>() {
                                if !ports.contains(&port) {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    ports
}

/// FW-004: Escanea puertos de servicios Enola existentes en configs Nginx.
/// Busca archivos proxy_* y fileserver_* en /etc/nginx/sites-available/
/// y extrae listen + proxy_pass ports bound to 127.0.0.1.
fn scan_existing_enola_ports() -> Vec<u16> {
    let sites_dir = std::path::Path::new("/etc/nginx/sites-available");
    let mut ports = Vec::new();

    let entries = match std::fs::read_dir(sites_dir) {
        Ok(e) => e,
        Err(_) => return ports, // Nginx no instalado o sin permisos
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Solo configs generadas por Enola
        if !name_str.starts_with("proxy_") && !name_str.starts_with("fileserver_") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for line in content.lines() {
                let trimmed = line.trim();
                // listen 127.0.0.1:PORT
                if trimmed.starts_with("listen") && trimmed.contains("127.0.0.1:") {
                    if let Some(port_str) = trimmed.split("127.0.0.1:").nth(1) {
                        if let Some(p) = port_str.split(|c: char| !c.is_ascii_digit()).next() {
                            if let Ok(port) = p.parse::<u16>() {
                                if !ports.contains(&port) {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
                // proxy_pass http://127.0.0.1:PORT
                if trimmed.starts_with("proxy_pass") && trimmed.contains("127.0.0.1:") {
                    if let Some(port_str) = trimmed.split("127.0.0.1:").nth(1) {
                        if let Some(p) = port_str.split(|c: char| !c.is_ascii_digit()).next() {
                            if let Ok(port) = p.parse::<u16>() {
                                if !ports.contains(&port) {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !ports.is_empty() {
        eprintln!(
            "🔍 FW-004: Found {} ports from existing Enola services",
            ports.len()
        );
    }

    ports
}

// ─── AA-012: Auto-crear perfil AppArmor al crear servicios ──────────────────
/// Crea y carga un perfil AppArmor para un servicio recién creado.
/// Non-blocking: si AppArmor no está disponible, solo imprime info.
fn apparmor_apply_profile(
    service_type: &crate::domain::apparmor::AppArmorServiceType,
    instance_name: &str,
) {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use crate::domain::apparmor::AppArmorMode;
    use crate::ports::manifest::ManifestPort;
    use std::sync::Arc;
    let mgr = AppArmorManager::new(Arc::new(AppArmorAdapter::new()));
    match mgr.apply_to_service(service_type, instance_name, AppArmorMode::Complain) {
        Ok(msg) if !msg.is_empty() => {
            eprintln!("{}", msg);
            let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
            let _ = manifest.append(
                "apparmor_profile",
                &service_type.profile_name(instance_name),
            );
        }
        Ok(_) => {} // AppArmor not available — skip silently
        Err(e) => eprintln!("⚠️  AppArmor profile creation failed: {} (non-blocking)", e),
    }
}

// ─── AA-013: Auto-eliminar perfil AppArmor al eliminar servicios ────────────
/// Elimina el perfil AppArmor de un servicio eliminado.
/// Non-blocking: nunca falla la eliminación del servicio.
fn apparmor_remove_profile(
    service_type: &crate::domain::apparmor::AppArmorServiceType,
    instance_name: &str,
) {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use crate::ports::manifest::ManifestPort;
    use std::sync::Arc;
    let mgr = AppArmorManager::new(Arc::new(AppArmorAdapter::new()));
    match mgr.remove_from_service(service_type, instance_name) {
        Ok(()) => {
            let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
            let _ = manifest.remove(
                "apparmor_profile",
                &service_type.profile_name(instance_name),
            );
        }
        Err(e) => eprintln!("⚠️  AppArmor profile removal failed: {} (non-blocking)", e),
    }
}
// ═══════════════════════════════════════════════════════════════════════════
// GIT EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_git(cmd: GitCommands, format: &str) -> CliResult<String> {
    match cmd {
        GitCommands::List => {
            let servers = commands::git::list().await?;
            format_output(&servers, format)
        }
        GitCommands::Create {
            name,
            ssl,
            http_port,
            ssh_port,
            admin_user,
            admin_password,
        } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            print_ufw_warning();
            print_apparmor_warning();
            // Validar: si se da uno de los dos, hay que dar ambos
            match (&admin_user, &admin_password) {
                (Some(_), None) => {
                    return Err(CliError::InvalidInput(
                        "Si especificas --admin-user también debes especificar --admin-password"
                            .to_string(),
                    ))
                }
                (None, Some(_)) => {
                    return Err(CliError::InvalidInput(
                        "Si especificas --admin-password también debes especificar --admin-user"
                            .to_string(),
                    ))
                }
                _ => {}
            }
            // PORTS-004: Validar puertos ANTES de crear (fail-fast, 0 residuos)
            {
                use crate::adapters::infra::port_checker::PortCheckerAdapter;
                use crate::application::port_validator::{PortRanges, PortValidator};
                use std::sync::Arc;
                let validator = PortValidator::new(Arc::new(PortCheckerAdapter::new()));
                let resolved_http = validator
                    .resolve_port(http_port, PortRanges::GIT_HTTP, "http-port")
                    .map_err(|e| CliError::InvalidInput(e.to_string()))?;
                let resolved_ssh = validator
                    .resolve_port(ssh_port, PortRanges::GIT_SSH, "ssh-port")
                    .map_err(|e| CliError::InvalidInput(e.to_string()))?;
                let result = commands::git::create(
                    &name,
                    ssl,
                    admin_user.as_deref(),
                    admin_password.as_deref(),
                    resolved_http,
                    resolved_ssh,
                )
                .await?;
                // FW-001: Registrar puertos en UFW
                sync_firewall_ports(&[resolved_http, resolved_ssh]);
                // AA-012: Crear perfil AppArmor para el servidor Git
                apparmor_apply_profile(&crate::domain::apparmor::AppArmorServiceType::Git, &name);
                Ok(format!("✅ Git server '{}' created!\n{}", name, result))
            }
        }
        GitCommands::Start { name } => {
            commands::git::start(&name).await?;
            Ok(format!("✅ Git server '{}' started", name))
        }
        GitCommands::Stop { name } => {
            commands::git::stop(&name).await?;
            Ok(format!("✅ Git server '{}' stopped", name))
        }
        GitCommands::Status { name } => {
            let info = commands::git::status(&name).await?;
            format_output(&info, format)
        }
        GitCommands::Delete { name, force } => {
            if !force {
                return Err(CliError::InvalidInput(
                    "Confirmation required. Use --force to skip.".to_string(),
                ));
            }
            // FW-003: Extraer puertos ANTES de eliminar para poder limpiar UFW
            let ports = extract_service_ports(&name);
            commands::git::delete(&name).await?;
            if !ports.is_empty() {
                sync_firewall_cleanup(&ports);
            }
            // Remove Tor hidden service if it exists.
            let tor_adapter = Arc::new(crate::adapters::tor::TorConfigAdapter::new());
            let _ = crate::ports::tor::TorManagerPort::remove_hidden_service(
                tor_adapter.as_ref(),
                &name,
            )
            .await;
            // AA-013: Eliminar perfil AppArmor del servidor eliminado
            apparmor_remove_profile(&crate::domain::apparmor::AppArmorServiceType::Git, &name);
            Ok(format!("✅ Git server '{}' deleted", name))
        }
        GitCommands::Registration {
            name,
            enable,
            disable,
            status,
        } => {
            if status {
                // --status: mostrar estado actual sin modificar
                let enabled = commands::git::registration_status(&name).await?;
                let state = if enabled {
                    "habilitado ✅"
                } else {
                    "deshabilitado ❌"
                };
                Ok(format!("📋 Registro de usuarios en '{}': {}", name, state))
            } else {
                // --disable tiene precedencia; --enable activa; ninguno = deshabilitar por defecto
                let should_enable = if disable { false } else { enable };
                commands::git::registration(&name, should_enable).await?;
                let state = if should_enable {
                    "habilitado ✅"
                } else {
                    "deshabilitado ❌"
                };
                Ok(format!("✅ Registro de usuarios en '{}': {}", name, state))
            }
        }
        GitCommands::Edit {
            name,
            http_port,
            https_port,
            ssh_port,
            auto_ports,
        } => {
            let result =
                commands::git::edit(&name, http_port, https_port, ssh_port, auto_ports).await?;
            // FW-002: Registrar nuevos puertos en UFW si se editaron
            let mut ports_to_sync = Vec::new();
            if let Some(p) = http_port {
                ports_to_sync.push(p);
            }
            if let Some(p) = https_port {
                ports_to_sync.push(p);
            }
            if let Some(p) = ssh_port {
                ports_to_sync.push(p);
            }
            if !ports_to_sync.is_empty() {
                sync_firewall_ports(&ports_to_sync);
            }
            Ok(result)
        }
        GitCommands::Publish { name, ssl } => {
            let result = commands::git::publish(&name, ssl).await?;
            Ok(result)
        }
        GitCommands::Hide { name } => {
            commands::git::hide(&name).await?;
            Ok(format!("✅ Git server '{}' hidden from Tor", name))
        }
        GitCommands::User(user_cmd) => execute_git_user(user_cmd, format).await,
        GitCommands::Watcher => {
            println!("👀 Starting Git Pipeline Watcher (Foreground)...");
            println!("Waiting for signals in /var/lib/enola/triggers/*.sig");
            // This is blocking
            commands::git::watcher().await?;
            Ok("Watcher stopped".to_string())
        }
    }
}

async fn execute_git_user(cmd: GitUserCommands, format: &str) -> CliResult<String> {
    match cmd {
        GitUserCommands::List {
            server,
            admin_user,
            admin_pass,
        } => {
            let users =
                commands::git::user::list(&server, admin_user.as_deref(), admin_pass.as_deref())
                    .await?;
            if users.is_empty() {
                Ok(format!("No users found on server '{}'", server))
            } else {
                format_output(&users, format)
            }
        }
        GitUserCommands::Create {
            server,
            username,
            email,
            password,
            admin,
            admin_user,
            admin_pass,
        } => {
            commands::git::user::create(
                &server,
                &username,
                &email,
                &password,
                admin,
                admin_user.as_deref(),
                admin_pass.as_deref(),
            )
            .await?;
            Ok(format!(
                "✅ User '{}' created on server '{}'",
                username, server
            ))
        }
        GitUserCommands::Delete {
            server,
            username,
            admin_user,
            admin_pass,
        } => {
            commands::git::user::delete(
                &server,
                &username,
                admin_user.as_deref(),
                admin_pass.as_deref(),
            )
            .await?;
            Ok(format!(
                "✅ User '{}' deleted from server '{}'",
                username, server
            ))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WORDPRESS EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_wordpress(cmd: WordPressCommands, format: &str) -> CliResult<String> {
    match cmd {
        WordPressCommands::List => {
            let sites = commands::wordpress::list().await?;
            format_output(&sites, format)
        }
        WordPressCommands::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            print_ufw_warning();
            print_apparmor_warning();
            // PORTS-005: Validar puerto ANTES de crear (fail-fast, 0 residuos)
            use crate::adapters::infra::port_checker::PortCheckerAdapter;
            use crate::application::port_validator::{PortRanges, PortValidator};
            use std::sync::Arc;
            let validator = PortValidator::new(Arc::new(PortCheckerAdapter::new()));
            let resolved_http = validator
                .resolve_port(http_port, PortRanges::WORDPRESS_BACKEND, "http-port")
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            println!("📝 Creating WordPress site '{}'...", name);
            let _site = commands::wordpress::create(&name, Some(resolved_http)).await?;
            // FW-001: Registrar puerto en UFW
            sync_firewall_ports(&[resolved_http]);
            // AA-012: Crear perfil AppArmor para WordPress
            apparmor_apply_profile(
                &crate::domain::apparmor::AppArmorServiceType::WordPress,
                &name,
            );
            Ok(format!(
                "✅ WordPress site '{}' created!\n\
                 🌐 Setup: http://127.0.0.1:{}/\n\
                 ⚠️  Complete the setup wizard FIRST, then publish on Tor:\n\
                    enola-cli wp publish {}",
                name, resolved_http, name
            ))
        }
        WordPressCommands::Start { name } => {
            commands::wordpress::start(&name).await?;
            Ok(format!("✅ WordPress site '{}' started", name))
        }
        WordPressCommands::Stop { name } => {
            commands::wordpress::stop(&name).await?;
            Ok(format!("✅ WordPress site '{}' stopped", name))
        }
        WordPressCommands::Restart { name } => {
            commands::wordpress::restart(&name).await?;
            Ok(format!("✅ WordPress site '{}' restarted", name))
        }
        WordPressCommands::Delete { name, force } => {
            if !force {
                return Err(CliError::InvalidInput(
                    "Confirmation required. Use --force to skip.".to_string(),
                ));
            }
            // FW-003: Extraer puertos ANTES de eliminar para poder limpiar UFW
            // WP usa naming wp-{name} para la config Nginx
            let mut ports = extract_service_ports(&name);
            ports.extend(extract_service_ports(&format!("wp-{}", name)));
            commands::wordpress::delete(&name).await?;
            // Remove the Tor hidden service (name without prefix, consistent
            // with wp create/publish/hide/edit). Without this, deleted sites
            // left orphaned .onion configs in /etc/tor/enola.d/.
            if let Err(e) = commands::wordpress::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            if !ports.is_empty() {
                sync_firewall_cleanup(&ports);
            }
            // AA-013: Eliminar perfil AppArmor del sitio eliminado
            apparmor_remove_profile(
                &crate::domain::apparmor::AppArmorServiceType::WordPress,
                &name,
            );
            // SEC-005: Cleanup secrets directory
            let secrets_dir = format!("/srv/enola-wordpress/{}_secrets", name);
            if std::path::Path::new(&secrets_dir).exists() {
                let _ = std::fs::remove_dir_all(&secrets_dir);
            }
            // Cleanup data volumes (bind mounts) — stale MariaDB data causes
            // "Access denied" on recreate because old password doesn't match
            let db_dir = format!("/srv/enola-wordpress/{}_db", name);
            if std::path::Path::new(&db_dir).exists() {
                let _ = std::fs::remove_dir_all(&db_dir);
            }
            let wp_dir = format!("/srv/enola-wordpress/{}_wp", name);
            if std::path::Path::new(&wp_dir).exists() {
                let _ = std::fs::remove_dir_all(&wp_dir);
            }
            Ok(format!("✅ WordPress site '{}' deleted", name))
        }
        WordPressCommands::Update { name } => {
            println!("🔄 Updating WordPress site '{}' (with backup)...", name);
            commands::wordpress::update(&name).await?;
            Ok(format!("✅ WordPress site '{}' updated successfully", name))
        }
        WordPressCommands::Config { name } => {
            let result = commands::wordpress::config(&name).await?;
            Ok(result)
        }
        WordPressCommands::Status { name } => {
            let status = commands::wordpress::status(&name).await?;
            format_output(&status, format)
        }
        WordPressCommands::Publish { name } => {
            let result = commands::wordpress::publish(&name).await?;
            Ok(result)
        }
        WordPressCommands::Hide { name } => {
            commands::wordpress::hide(&name).await?;
            Ok(format!("✅ WordPress site '{}' hidden from Tor", name))
        }
        WordPressCommands::Edit {
            name,
            http_port,
            https_port,
            ssl,
            auto_ports,
        } => {
            let result =
                commands::wordpress::edit(&name, http_port, https_port, ssl, auto_ports).await?;
            // FW-002: Registrar nuevos puertos en UFW si se editaron
            let mut ports_to_sync = Vec::new();
            if let Some(p) = http_port {
                ports_to_sync.push(p);
            }
            if let Some(p) = https_port {
                ports_to_sync.push(p);
            }
            if !ports_to_sync.is_empty() {
                sync_firewall_ports(&ports_to_sync);
            }
            Ok(result)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DRUPAL EXECUTOR — DRUPAL-003 (cableado del CmsLifecycle de DRUPAL-002)
// ═══════════════════════════════════════════════════════════════════════════

/// Construye un `DrupalCmsAdapter` listo para usar (ContainerPort = Bollard).
fn build_manifest() -> Arc<dyn ManifestPort + Send + Sync> {
    Arc::new(FileManifestAdapter::new())
}

fn build_drupal_adapter() -> CliResult<crate::adapters::cms::drupal::DrupalCmsAdapter> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    let docker = BollardDockerAdapter::new()
        .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
    Ok(crate::adapters::cms::drupal::DrupalCmsAdapter::new(
        Arc::new(docker),
        build_manifest(),
    ))
}

async fn execute_drupal(cmd: DrupalCommands, format: &str) -> CliResult<String> {
    use crate::cli::DrupalCommands as D;
    use crate::domain::cms::{CmsCreateRequest, CmsStatus};
    use crate::ports::cms::{CmsAdapter, CmsLifecycle};

    match cmd {
        D::List => {
            // DRUPAL-003: lista por convención de naming `drupal-<name>` (§13.3).
            // Parser ligero sobre `docker ps -a` (igual patrón que `wp::list`).
            let output = std::process::Command::new("docker")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    "name=drupal-",
                    "--format",
                    "{{.Names}}\t{{.Status}}\t{{.Ports}}",
                ])
                .output()
                .map_err(|e| CliError::Generic(format!("docker ps failed: {}", e)))?;
            let raw = String::from_utf8_lossy(&output.stdout);
            let mut sites = Vec::new();
            for line in raw.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 2 {
                    continue;
                }
                let cname = parts[0];
                if !cname.starts_with("drupal-") {
                    continue;
                }
                let name = cname.strip_prefix("drupal-").unwrap_or(cname).to_string();
                let status = if parts[1].contains("Up") {
                    "running"
                } else {
                    "stopped"
                };
                sites.push(serde_json::json!({
                    "name": name,
                    "container": cname,
                    "status": status,
                    "ports": parts.get(2).copied().unwrap_or(""),
                }));
            }
            if format == "json" {
                return Ok(serde_json::to_string_pretty(&sites).unwrap_or_default());
            }
            if sites.is_empty() {
                return Ok("No Drupal sites found.".to_string());
            }
            let mut out = format!("🌐 Drupal sites ({})\n", sites.len());
            out.push_str("═══════════════════════════════════════════════════\n");
            for s in &sites {
                out.push_str(&format!(
                    "• {}  [{}]  ports={}\n",
                    s["name"].as_str().unwrap_or(""),
                    s["status"].as_str().unwrap_or(""),
                    s["ports"].as_str().unwrap_or(""),
                ));
            }
            Ok(out)
        }
        D::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let adapter = build_drupal_adapter()?;
            let req = CmsCreateRequest {
                name: name.clone(),
                http_port: Some(http_port),
                db_password: None,
            };
            let inst = adapter.create(req).await.map_err(CliError::from)?;
            // FW-001: registrar puerto interno en UFW (defensa en profundidad).
            sync_firewall_ports(&[http_port]);
            Ok(format!(
                "✅ Drupal '{}' created (status: {}). \
                 Open http://127.0.0.1:{}/ to complete the setup wizard.\n   \
                 Stack: {} + {}.\n\
                 ⚠️  Complete the setup FIRST, then publish on Tor:\n\
                    enola-cli drupal publish {}",
                name,
                inst.status,
                http_port,
                adapter.descriptor().default_image,
                adapter
                    .descriptor()
                    .db_stack
                    .default_image()
                    .unwrap_or("none"),
                name,
            ))
        }
        D::Start { name } => {
            let adapter = build_drupal_adapter()?;
            adapter.start(&name).await.map_err(CliError::from)?;
            Ok(format!("✅ Drupal '{}' started", name))
        }
        D::Stop { name } => {
            let adapter = build_drupal_adapter()?;
            adapter.stop(&name).await.map_err(CliError::from)?;
            Ok(format!("✅ Drupal '{}' stopped", name))
        }
        D::Delete { name, force } => {
            let adapter = build_drupal_adapter()?;
            adapter.delete(&name, force).await.map_err(CliError::from)?;
            // Remove the Tor hidden service (drupal-{name}). Without this,
            // deleted sites left orphaned .onion configs in /etc/tor/enola.d/.
            if let Err(e) = commands::drupal::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            // Cleanup data volumes (bind mounts) — stale MariaDB data causes
            // "Access denied" on recreate because old password doesn't match
            let drupal_dir = format!("/srv/enola-drupal/{}", name);
            if std::path::Path::new(&drupal_dir).exists() {
                let _ = std::fs::remove_dir_all(&drupal_dir);
            }
            Ok(format!("✅ Drupal '{}' deleted", name))
        }
        D::Status { name } => {
            let adapter = build_drupal_adapter()?;
            let inst = adapter.status(&name).await.map_err(CliError::from)?;
            if inst.status == CmsStatus::NotFound {
                return Err(CliError::InvalidInput(format!(
                    "Drupal site '{}' not found",
                    name
                )));
            }
            if format == "json" {
                let json = serde_json::json!({
                    "name": inst.name,
                    "kind": inst.kind.slug(),
                    "status": inst.status.to_string(),
                    "http_port": inst.http_port,
                    "onion": inst.onion_address,
                });
                return Ok(serde_json::to_string_pretty(&json).unwrap_or_default());
            }
            Ok(format!(
                "🌐 Drupal '{}'\n  status:    {}\n  http_port: {}\n  onion:     {}",
                inst.name,
                inst.status,
                inst.http_port
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                inst.onion_address.unwrap_or_else(|| "-".to_string()),
            ))
        }
        // — DRUPAL-004a: publish/hide cableados sobre Tor —
        D::Publish { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let result = commands::drupal::publish(&name).await?;
            Ok(result)
        }
        D::Hide { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::drupal::hide(&name).await?;
            Ok(format!("✅ Drupal '{}' hidden from Tor", name))
        }
        // — Edit (port hot-swap con recreación de contenedor) — DRUPAL-006 ✅ —
        D::Edit { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::drupal::edit(&name, http_port).await
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GHOST EXECUTOR — CMS-GHOST-002
//
// Lifecycle delegado a `GhostCmsAdapter` (CMS-GHOST-001). Una sola variante de
// contenedor (SQLite embebido), parser docker ps por prefijo `ghost-` (§13.3).
// publish/hide/edit son stubs orientativos hasta CMS-GHOST-003+ (paralelo a
// cómo DRUPAL-003 stubeó publish/hide hasta DRUPAL-004a).
// ═══════════════════════════════════════════════════════════════════════════

fn build_ghost_adapter() -> CliResult<crate::adapters::cms::ghost::GhostCmsAdapter> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    let docker = BollardDockerAdapter::new()
        .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
    Ok(crate::adapters::cms::ghost::GhostCmsAdapter::new(
        Arc::new(docker),
        build_manifest(),
    ))
}

async fn execute_ghost(cmd: GhostCommands, format: &str) -> CliResult<String> {
    use crate::cli::GhostCommands as G;
    use crate::domain::cms::{CmsCreateRequest, CmsStatus};
    use crate::ports::cms::{CmsAdapter, CmsLifecycle};

    match cmd {
        G::List => {
            // Misma estrategia que `drupal list`: parser ligero sobre docker ps.
            // Ghost containers tienen prefijo `ghost-` (§13.3, anti-colisión con wp/drupal).
            let output = std::process::Command::new("docker")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    "name=ghost-",
                    "--format",
                    "{{.Names}}\t{{.Status}}\t{{.Ports}}",
                ])
                .output()
                .map_err(|e| CliError::Generic(format!("docker ps failed: {}", e)))?;
            let raw = String::from_utf8_lossy(&output.stdout);
            let mut sites = Vec::new();
            for line in raw.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 2 {
                    continue;
                }
                let cname = parts[0];
                if !cname.starts_with("ghost-") {
                    continue;
                }
                let name = cname.strip_prefix("ghost-").unwrap_or(cname).to_string();
                let status = if parts[1].contains("Up") {
                    "running"
                } else {
                    "stopped"
                };
                sites.push(serde_json::json!({
                    "name": name,
                    "container": cname,
                    "status": status,
                    "ports": parts.get(2).copied().unwrap_or(""),
                }));
            }
            if format == "json" {
                return Ok(serde_json::to_string_pretty(&sites).unwrap_or_default());
            }
            if sites.is_empty() {
                return Ok("No Ghost blogs found.".to_string());
            }
            let mut out = format!("✍️  Ghost blogs ({})\n", sites.len());
            out.push_str("═══════════════════════════════════════════════════\n");
            for s in &sites {
                out.push_str(&format!(
                    "• {}  [{}]  ports={}\n",
                    s["name"].as_str().unwrap_or(""),
                    s["status"].as_str().unwrap_or(""),
                    s["ports"].as_str().unwrap_or(""),
                ));
            }
            Ok(out)
        }
        G::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let adapter = build_ghost_adapter()?;
            let req = CmsCreateRequest {
                name: name.clone(),
                http_port: Some(http_port),
                db_password: None, // Ghost SQLite no usa password
            };
            let inst = adapter.create(req).await.map_err(CliError::from)?;
            // FW-001: registrar puerto interno en UFW (defensa en profundidad).
            sync_firewall_ports(&[http_port]);
            Ok(format!(
                "✅ Ghost '{}' created (status: {}). \
                 Open http://127.0.0.1:{}/ to complete the setup wizard.\n   \
                 Stack: {} (SQLite embedded — no DB container).\n\
                 ⚠️  Complete the setup FIRST, then publish on Tor:\n\
                    enola-cli ghost publish {}",
                name,
                inst.status,
                http_port,
                adapter.descriptor().default_image,
                name,
            ))
        }
        G::Start { name } => {
            let adapter = build_ghost_adapter()?;
            adapter.start(&name).await.map_err(CliError::from)?;
            Ok(format!("✅ Ghost '{}' started", name))
        }
        G::Stop { name } => {
            let adapter = build_ghost_adapter()?;
            adapter.stop(&name).await.map_err(CliError::from)?;
            Ok(format!("✅ Ghost '{}' stopped", name))
        }
        G::Delete { name, force } => {
            use crate::adapters::infra::docker::BollardDockerAdapter;
            use crate::ports::container::ContainerPort as _;
            let adapter = build_ghost_adapter()?;
            adapter.delete(&name, force).await.map_err(CliError::from)?;
            // Remove Tor hidden service (ghost-{name}).
            if let Err(e) = commands::ghost::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            // Remove Docker network to prevent pool exhaustion.
            let docker = BollardDockerAdapter::new()
                .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
            let _ = docker
                .remove_network(&format!("enola_net_ghost_{}", name))
                .await;
            // Cleanup data volumes.
            let ghost_dir = format!("/srv/enola-ghost/{}", name);
            if std::path::Path::new(&ghost_dir).exists() {
                let _ = std::fs::remove_dir_all(&ghost_dir);
            }
            Ok(format!("✅ Ghost '{}' deleted", name))
        }
        G::Status { name } => {
            let adapter = build_ghost_adapter()?;
            let inst = adapter.status(&name).await.map_err(CliError::from)?;
            if inst.status == CmsStatus::NotFound {
                return Err(CliError::InvalidInput(format!(
                    "Ghost blog '{}' not found",
                    name
                )));
            }
            if format == "json" {
                let json = serde_json::json!({
                    "name": inst.name,
                    "kind": inst.kind.slug(),
                    "status": inst.status.to_string(),
                    "http_port": inst.http_port,
                    "onion": inst.onion_address,
                });
                return Ok(serde_json::to_string_pretty(&json).unwrap_or_default());
            }
            Ok(format!(
                "✍️  Ghost '{}'\n  status:    {}\n  http_port: {}\n  onion:     {}",
                inst.name,
                inst.status,
                inst.http_port
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                inst.onion_address.unwrap_or_else(|| "-".to_string()),
            ))
        }
        // — CMS-GHOST-003: publish/hide/edit cableados (paralelo a DRUPAL-004a/006) —
        G::Publish { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::ghost::publish(&name).await
        }
        G::Hide { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::ghost::hide(&name).await?;
            Ok(format!("✅ Ghost '{}' hidden from Tor", name))
        }
        G::Edit { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::ghost::edit(&name, http_port).await
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAGNOLIA / STRAPI / WAGTAIL EXECUTORS (CMS-MAGNOLIA-CLI / CMS-STRAPI-CLI /
// CMS-WAGTAIL-CLI, 2026-05-09)
//
// Cableado siguiendo el patrón Ghost (§13.56 — CmsLifecycle): create,
// start, stop, delete, status, list, publish, hide. Strapi además incluye
// build-image para construir la imagen de producción localmente.
// Modelo free-only: sin protección por tier.
// ═══════════════════════════════════════════════════════════════════════════

fn build_magnolia_adapter() -> CliResult<crate::adapters::cms::magnolia::MagnoliaCmsAdapter> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    let docker = BollardDockerAdapter::new()
        .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
    Ok(crate::adapters::cms::magnolia::MagnoliaCmsAdapter::new(
        Arc::new(docker),
        build_manifest(),
    ))
}

fn build_strapi_adapter() -> CliResult<crate::adapters::cms::strapi::StrapiCmsAdapter> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    let docker = BollardDockerAdapter::new()
        .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
    Ok(crate::adapters::cms::strapi::StrapiCmsAdapter::new(
        Arc::new(docker),
        build_manifest(),
    ))
}

fn build_wagtail_adapter() -> CliResult<crate::adapters::cms::wagtail::WagtailCmsAdapter> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    let docker = BollardDockerAdapter::new()
        .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
    Ok(crate::adapters::cms::wagtail::WagtailCmsAdapter::new(
        Arc::new(docker),
        build_manifest(),
    ))
}

/// List CMS instances by container prefix (parser docker ps, mismo
/// patrón que `execute_ghost::List` y `execute_drupal::List`).
async fn list_cms_by_prefix(
    prefix: &str,
    icon: &str,
    label: &str,
    format: &str,
) -> CliResult<String> {
    let output = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={}", prefix),
            "--format",
            "{{.Names}}\t{{.Status}}\t{{.Ports}}",
        ])
        .output()
        .map_err(|e| CliError::Generic(format!("docker ps failed: {}", e)))?;
    let raw = String::from_utf8_lossy(&output.stdout);
    let mut sites = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let cname = parts[0];
        if !cname.starts_with(prefix) {
            continue;
        }
        // Para Strapi/Wagtail, descartar el contenedor BD (`db-<name>-<cms>`)
        // si hubiera; el prefijo ya nos da solo el contenedor web.
        let name = cname.strip_prefix(prefix).unwrap_or(cname).to_string();
        let status = if parts[1].contains("Up") {
            "running"
        } else {
            "stopped"
        };
        sites.push(serde_json::json!({
            "name": name,
            "container": cname,
            "status": status,
            "ports": parts.get(2).copied().unwrap_or(""),
        }));
    }
    if format == "json" {
        return Ok(serde_json::to_string_pretty(&sites).unwrap_or_default());
    }
    if sites.is_empty() {
        return Ok(format!("No {} instances found.", label));
    }
    let mut out = format!("{}  {} instances ({})\n", icon, label, sites.len());
    out.push_str("═══════════════════════════════════════════════════\n");
    for s in &sites {
        out.push_str(&format!(
            "• {}  [{}]  ports={}\n",
            s["name"].as_str().unwrap_or(""),
            s["status"].as_str().unwrap_or(""),
            s["ports"].as_str().unwrap_or(""),
        ));
    }
    Ok(out)
}

fn render_cms_status(
    inst: &crate::domain::cms::CmsInstance,
    icon: &str,
    label: &str,
    format: &str,
) -> CliResult<String> {
    if format == "json" {
        let json = serde_json::json!({
            "name": inst.name,
            "kind": inst.kind.slug(),
            "status": inst.status.to_string(),
            "http_port": inst.http_port,
            "onion": inst.onion_address,
        });
        return Ok(serde_json::to_string_pretty(&json).unwrap_or_default());
    }
    Ok(format!(
        "{}  {} '{}'\n  status:    {}\n  http_port: {}\n  onion:     {}",
        icon,
        label,
        inst.name,
        inst.status,
        inst.http_port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string()),
        inst.onion_address
            .clone()
            .unwrap_or_else(|| "-".to_string()),
    ))
}

async fn execute_magnolia(cmd: MagnoliaCommands, format: &str) -> CliResult<String> {
    use crate::cli::MagnoliaCommands as M;
    use crate::domain::cms::{CmsCreateRequest, CmsStatus};
    use crate::ports::cms::{CmsAdapter, CmsLifecycle};

    match cmd {
        M::List => list_cms_by_prefix("magnolia-", "🌳", "Magnolia", format).await,
        M::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let adapter = build_magnolia_adapter()?;
            let req = CmsCreateRequest {
                name: name.clone(),
                http_port: Some(http_port),
                db_password: None,
            };
            let inst = adapter.create(req).await.map_err(CliError::from)?;
            sync_firewall_ports(&[http_port]);
            Ok(format!(
                "✅ Magnolia '{}' created (status: {}). \
                 Open http://127.0.0.1:{}/ to complete the setup wizard.\n   \
                 Stack: {} (Tomcat — needs ≥4 GB RAM).\n\
                 ⚠️  Complete the setup FIRST, then publish on Tor:\n\
                    enola-cli magnolia publish {}",
                name,
                inst.status,
                http_port,
                adapter.descriptor().default_image,
                name,
            ))
        }
        M::Start { name } => {
            build_magnolia_adapter()?
                .start(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Magnolia '{}' started", name))
        }
        M::Stop { name } => {
            build_magnolia_adapter()?
                .stop(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Magnolia '{}' stopped", name))
        }
        M::Delete { name, force } => {
            use crate::adapters::infra::docker::BollardDockerAdapter;
            use crate::ports::container::ContainerPort as _;
            build_magnolia_adapter()?
                .delete(&name, force)
                .await
                .map_err(CliError::from)?;
            // Remove Tor hidden service (magnolia-{name}).
            if let Err(e) = commands::magnolia::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            // Remove Docker network to prevent pool exhaustion.
            let docker = BollardDockerAdapter::new()
                .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
            let _ = docker
                .remove_network(&format!("enola_net_magnolia_{}", name))
                .await;
            // Cleanup data volumes.
            let data_dir = format!("/srv/enola-magnolia/{}", name);
            if std::path::Path::new(&data_dir).exists() {
                let _ = std::fs::remove_dir_all(&data_dir);
            }
            Ok(format!("✅ Magnolia '{}' deleted", name))
        }
        M::Status { name } => {
            let adapter = build_magnolia_adapter()?;
            let inst = adapter.status(&name).await.map_err(CliError::from)?;
            if inst.status == CmsStatus::NotFound {
                return Err(CliError::InvalidInput(format!(
                    "Magnolia instance '{}' not found",
                    name
                )));
            }
            render_cms_status(&inst, "🌳", "Magnolia", format)
        }
        M::Publish { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::magnolia::publish(&name).await
        }
        M::Hide { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::magnolia::hide(&name).await?;
            Ok(format!("✅ Magnolia '{}' hidden from Tor", name))
        }
    }
}

async fn execute_strapi(cmd: StrapiCommands, format: &str) -> CliResult<String> {
    use crate::cli::StrapiCommands as S;
    use crate::domain::cms::{CmsCreateRequest, CmsStatus};
    use crate::ports::cms::{CmsAdapter, CmsLifecycle};

    match cmd {
        S::List => list_cms_by_prefix("strapi-", "🚀", "Strapi", format).await,
        S::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            // Verify the Strapi production image exists locally (must build first).
            {
                use crate::adapters::infra::docker::BollardDockerAdapter;
                use crate::ports::container::ContainerPort as _;
                let docker = BollardDockerAdapter::new()
                    .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
                let tag = crate::infrastructure::embedded_scripts::STRAPI_IMAGE_TAG;
                if !docker.image_exists(tag).await.map_err(CliError::from)? {
                    return Err(CliError::Generic(format!(
                        "Strapi image '{}' not found. Build it first with:\n  \
                         sudo enola-cli strapi build-image",
                        tag
                    )));
                }
            }
            let adapter = build_strapi_adapter()?;
            let req = CmsCreateRequest {
                name: name.clone(),
                http_port: Some(http_port),
                db_password: None,
            };
            let inst = adapter.create(req).await.map_err(CliError::from)?;
            sync_firewall_ports(&[http_port]);
            Ok(format!(
                "✅ Strapi '{}' created (status: {}). \
                 Admin panel: http://127.0.0.1:{}/admin (first run sets up admin).\n   \
                 Stack: {} (Postgres 16 + 5 secrets in /srv/enola-strapi/{}/secrets/).\n\
                 ⚠️  Complete the setup FIRST, then publish on Tor:\n\
                    enola-cli strapi publish {}",
                name,
                inst.status,
                http_port,
                adapter.descriptor().default_image,
                name,
                name,
            ))
        }
        S::Start { name } => {
            build_strapi_adapter()?
                .start(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Strapi '{}' started", name))
        }
        S::Stop { name } => {
            build_strapi_adapter()?
                .stop(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Strapi '{}' stopped", name))
        }
        S::Delete { name, force } => {
            use crate::adapters::infra::docker::BollardDockerAdapter;
            use crate::ports::container::ContainerPort as _;
            build_strapi_adapter()?
                .delete(&name, force)
                .await
                .map_err(CliError::from)?;
            // Remove Tor hidden service (strapi-{name}).
            if let Err(e) = commands::strapi::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            // Remove Docker network to prevent pool exhaustion.
            let docker = BollardDockerAdapter::new()
                .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
            let _ = docker
                .remove_network(&format!("enola_net_strapi_{}", name))
                .await;
            // Cleanup data volumes.
            let data_dir = format!("/srv/enola-strapi/{}", name);
            if std::path::Path::new(&data_dir).exists() {
                let _ = std::fs::remove_dir_all(&data_dir);
            }
            Ok(format!("✅ Strapi '{}' deleted", name))
        }
        S::Status { name } => {
            let adapter = build_strapi_adapter()?;
            let inst = adapter.status(&name).await.map_err(CliError::from)?;
            if inst.status == CmsStatus::NotFound {
                return Err(CliError::InvalidInput(format!(
                    "Strapi instance '{}' not found",
                    name
                )));
            }
            render_cms_status(&inst, "🚀", "Strapi", format)
        }
        S::Publish { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::strapi::publish(&name).await
        }
        S::Hide { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::strapi::hide(&name).await?;
            Ok(format!("✅ Strapi '{}' hidden from Tor", name))
        }
        S::BuildImage { force } => {
            use crate::adapters::infra::docker::BollardDockerAdapter;
            use crate::infrastructure::embedded_scripts;
            use crate::ports::container::ContainerPort as _;
            use crate::ports::container::ImageBuildConfig;

            let tag = embedded_scripts::STRAPI_IMAGE_TAG;
            let docker = BollardDockerAdapter::new()
                .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;

            if !force && docker.image_exists(tag).await.map_err(CliError::from)? {
                return Ok(format!(
                    "✅ Strapi image '{}' already exists. Use --force to rebuild.",
                    tag
                ));
            }

            let context_path = embedded_scripts::ensure_strapi_context().map_err(|e| {
                CliError::Generic(format!("Failed to prepare Strapi build context: {}", e))
            })?;
            let dockerfile_path = context_path.join(embedded_scripts::STRAPI_DOCKERFILE_NAME);

            println!(
                "🏗️  Building Strapi production image: {} (this takes ~5-10 minutes)",
                tag
            );
            let build_config = ImageBuildConfig {
                dockerfile_path,
                context_path,
                tag: tag.to_string(),
                build_args: std::collections::HashMap::new(),
            };
            docker
                .build_image(build_config)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Strapi image '{}' built successfully.", tag))
        }
    }
}

async fn execute_wagtail(cmd: WagtailCommands, format: &str) -> CliResult<String> {
    use crate::cli::WagtailCommands as W;
    use crate::domain::cms::{CmsCreateRequest, CmsStatus};
    use crate::ports::cms::{CmsAdapter, CmsLifecycle};

    match cmd {
        W::List => list_cms_by_prefix("wagtail-", "🦅", "Wagtail", format).await,
        W::Create { name, http_port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let adapter = build_wagtail_adapter()?;
            let req = CmsCreateRequest {
                name: name.clone(),
                http_port: Some(http_port),
                db_password: None,
            };
            let inst = adapter.create(req).await.map_err(CliError::from)?;
            sync_firewall_ports(&[http_port]);
            Ok(format!(
                "✅ Wagtail '{}' created (status: {}). \
                 Open http://127.0.0.1:{}/admin/ to complete the setup.\n   \
                 Stack: {} (Postgres 16).\n\
                 ⚠️  Complete the setup FIRST, then publish on Tor:\n\
                    enola-cli wagtail publish {}",
                name,
                inst.status,
                http_port,
                adapter.descriptor().default_image,
                name,
            ))
        }
        W::Start { name } => {
            build_wagtail_adapter()?
                .start(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Wagtail '{}' started", name))
        }
        W::Stop { name } => {
            build_wagtail_adapter()?
                .stop(&name)
                .await
                .map_err(CliError::from)?;
            Ok(format!("✅ Wagtail '{}' stopped", name))
        }
        W::Delete { name, force } => {
            use crate::adapters::infra::docker::BollardDockerAdapter;
            use crate::ports::container::ContainerPort as _;
            build_wagtail_adapter()?
                .delete(&name, force)
                .await
                .map_err(CliError::from)?;
            // Remove Tor hidden service (wagtail-{name}).
            if let Err(e) = commands::wagtail::hide(&name).await {
                eprintln!("⚠️ Could not remove Tor service for '{}': {}", name, e);
            }
            // Remove Docker network to prevent pool exhaustion.
            let docker = BollardDockerAdapter::new()
                .map_err(|e| CliError::Generic(format!("Docker unavailable: {}", e)))?;
            let _ = docker
                .remove_network(&format!("enola_net_wagtail_{}", name))
                .await;
            // Cleanup data volumes.
            let data_dir = format!("/srv/enola-wagtail/{}", name);
            if std::path::Path::new(&data_dir).exists() {
                let _ = std::fs::remove_dir_all(&data_dir);
            }
            Ok(format!("✅ Wagtail '{}' deleted", name))
        }
        W::Status { name } => {
            let adapter = build_wagtail_adapter()?;
            let inst = adapter.status(&name).await.map_err(CliError::from)?;
            if inst.status == CmsStatus::NotFound {
                return Err(CliError::InvalidInput(format!(
                    "Wagtail instance '{}' not found",
                    name
                )));
            }
            render_cms_status(&inst, "🦅", "Wagtail", format)
        }
        W::Publish { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::wagtail::publish(&name).await
        }
        W::Hide { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::wagtail::hide(&name).await?;
            Ok(format!("✅ Wagtail '{}' hidden from Tor", name))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FILES EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_files(cmd: FileCommands, format: &str) -> CliResult<String> {
    match cmd {
        FileCommands::List => {
            let shares = commands::files::list().await?;
            format_output(&shares, format)
        }
        FileCommands::Create { name, auth, ssl } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let hostname = commands::files::create(&name, auth, ssl).await?;
            // FW-001: Los puertos del fileserver son internos (Nginx listen),
            // la creación devuelve hostname, los puertos se gestionan internamente
            Ok(format!(
                "✅ File share '{}' created!\n🧅 Hostname: {}",
                name, hostname
            ))
        }
        FileCommands::Edit { name, port } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let result = commands::files::edit(&name, port).await?;
            // FW-002: Registrar nuevo puerto en UFW si se editó
            if let Some(p) = port {
                sync_firewall_ports(&[p]);
            }
            Ok(result)
        }
        FileCommands::Delete { name, force } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            if !force {
                return Err(CliError::InvalidInput(
                    "Confirmation required. Use --force to skip.".to_string(),
                ));
            }
            commands::files::delete(&name).await?;
            Ok(format!("✅ File share '{}' deleted", name))
        }
        FileCommands::FixPerms { name } => {
            crate::domain::naming::validate_service_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            commands::files::fix_perms(&name).await?;
            Ok(format!("✅ Permissions fixed for file share '{}'", name))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAINTENANCE EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_maintenance(cmd: MaintenanceCommands, format: &str) -> CliResult<String> {
    match cmd {
        MaintenanceCommands::Status => {
            let status = commands::maintenance::status().await?;
            format_output(&status, format)
        }
        MaintenanceCommands::SmokeTest => {
            println!("🩺 Running smoke test...");
            let result = commands::maintenance::smoke_test().await?;
            format_output(&result, format)
        }
        MaintenanceCommands::EnableChecks => {
            commands::maintenance::enable_checks().await?;
            Ok("✅ Automatic health checks enabled".to_string())
        }
        MaintenanceCommands::DisableChecks => {
            commands::maintenance::disable_checks().await?;
            Ok("✅ Automatic health checks disabled".to_string())
        }
        MaintenanceCommands::TimerStatus => {
            let status = commands::maintenance::timer_status().await?;
            Ok(status)
        }
        MaintenanceCommands::SshConfig => {
            let config = commands::maintenance::ssh_config().await?;
            format_output(&config, format)
        }
        // PQC-012: Hardening SSH host con KEX post-cuántico (transitorio)
        MaintenanceCommands::SshHardenPqc { force, dry_run } => {
            let result = commands::maintenance::ssh_harden_pqc(force, dry_run).await?;
            Ok(result)
        }
        MaintenanceCommands::Backup => {
            println!("💾 Creating system backup...");
            let result = commands::maintenance::backup().await?;
            Ok(result)
        }
        MaintenanceCommands::Cleanup {
            target,
            dry_run,
            force,
            keep_days,
        } => {
            use crate::application::cleanup_service::{format_bytes, CleanupService};

            let mode = if dry_run { "DRY-RUN" } else { "CLEANUP" };
            println!("╔════════════════════════════════════════════════════════════════╗");
            println!(
                "║  🧹 MAINTENANCE {} - Target: {}                              ",
                mode, target
            );
            println!("╚════════════════════════════════════════════════════════════════╝");
            println!();

            // Determine project root (where the CLI is running from)
            let project_root =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

            let service = if target == "docker" || target == "all" {
                use crate::adapters::infra::docker::BollardDockerAdapter;
                match BollardDockerAdapter::new() {
                    Ok(docker) => CleanupService::new(project_root, dry_run)
                        .with_container_manager(std::sync::Arc::new(docker)),
                    Err(_) => CleanupService::new(project_root, dry_run),
                }
            } else {
                CleanupService::new(project_root, dry_run)
            };
            let result = service
                .cleanup(&target, keep_days, force)
                .await
                .map_err(|e| CliError::Generic(e.to_string()))?;

            println!();
            println!("════════════════════════════════════════════════════════════════");
            if dry_run {
                println!("📋 DRY-RUN Summary:");
                println!("   Would delete {} files", result.files_deleted);
                println!("   Would free {}", format_bytes(result.bytes_freed));
            } else {
                println!("✅ Cleanup Summary:");
                println!("   Deleted {} files", result.files_deleted);
                println!("   Freed {}", format_bytes(result.bytes_freed));
            }

            if !result.errors.is_empty() {
                println!();
                println!("⚠️  Errors ({}):", result.errors.len());
                for err in &result.errors {
                    println!("   - {}", err);
                }
            }
            println!("════════════════════════════════════════════════════════════════");

            Ok(format!(
                "Cleanup completed: {} files, {} freed",
                result.files_deleted,
                format_bytes(result.bytes_freed)
            ))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTICS EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_diagnostics(cmd: DiagnosticsCommands, format: &str) -> CliResult<String> {
    match cmd {
        DiagnosticsCommands::Summary => {
            let status = commands::diagnostics::summary().await?;
            format_output(&status, format)
        }
        DiagnosticsCommands::Nginx => {
            let status = commands::diagnostics::nginx().await?;
            format_output(&status, format)
        }
        DiagnosticsCommands::Tor => {
            let services = commands::diagnostics::tor().await?;
            format_output(&services, format)
        }
        DiagnosticsCommands::Ssh => {
            let status = commands::diagnostics::ssh().await?;
            format_output(&status, format)
        }
        DiagnosticsCommands::WordPress => {
            let status = commands::diagnostics::wordpress().await?;
            format_output(&status, format)
        }
        DiagnosticsCommands::WpSync => {
            let status = commands::diagnostics::wp_sync().await?;
            format_output(&status, format)
        }
        DiagnosticsCommands::NginxTest => {
            let success = commands::diagnostics::nginx_test().await?;
            if success {
                Ok("✅ NGINX configuration is valid".to_string())
            } else {
                Ok("❌ NGINX configuration has errors".to_string())
            }
        }
        DiagnosticsCommands::Resources => {
            let resources = commands::diagnostics::resources().await?;
            format_output(&resources, format)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_test(cmd: TestCommands, format: &str) -> CliResult<String> {
    match cmd {
        TestCommands::Run { filter } => {
            println!("🧪 Running tests...");
            let result = commands::test::run(filter.as_deref()).await?;
            format_output(&result, format)
        }
        TestCommands::List => {
            let tests = commands::test::list().await?;
            for test in &tests {
                println!("  • {}", test);
            }
            Ok(format!("Total: {} tests", tests.len()))
        }
        TestCommands::Benchmark => {
            println!("📊 Running benchmarks...");
            let result = commands::test::benchmark().await?;
            format_output(&result, format)
        }
        TestCommands::Results => {
            let results = commands::test::results().await?;
            format_output(&results, format)
        }
        TestCommands::Clean => {
            let result = commands::test::clean().await?;
            Ok(result)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LOGS EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_logs(cmd: LogCommands, _format: &str) -> CliResult<String> {
    match cmd {
        LogCommands::List => {
            let sources = commands::logs::list().await?;
            println!("📝 Available log sources:");
            for source in &sources {
                println!("  • {}", source);
            }
            Ok(format!("Total: {} sources", sources.len()))
        }
        LogCommands::View {
            source,
            lines,
            follow,
        } => {
            let logs = commands::logs::view(&source, lines, follow).await?;
            Ok(logs.join("\n"))
        }
        LogCommands::Install => {
            let logs = commands::logs::view("install", 100, false).await?;
            Ok(logs.join("\n"))
        }
        LogCommands::SmokeTest => {
            let logs = commands::logs::view("smoke-test", 100, false).await?;
            Ok(logs.join("\n"))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// QUICKREF EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_quickref() -> CliResult<String> {
    let quickref = r#"
╔══════════════════════════════════════════════════════════════════════════════╗
║                    📖 ENOLA CLI — REFERENCIA RÁPIDA                          ║
║                       Docker ↔ Enola CLI Equivalencias                       ║
╚══════════════════════════════════════════════════════════════════════════════╝

┌──────────────────────────────────────────────────────────────────────────────┐
│ LIMPIEZA Y MANTENIMIENTO                                                     │
├──────────────────────────────────────────────────────────────────────────────┤
│ Acción                  │ Docker (manual)          │ Enola CLI               │
├─────────────────────────┼──────────────────────────┼─────────────────────────┤
│ Limpiar contenedores    │ docker container prune   │ maintenance cleanup     │
│ Limpiar imágenes        │ docker image prune       │   --target docker       │
│ Limpiar todo Docker     │ docker system prune -a   │ maintenance cleanup     │
│                         │                          │   --target all          │
│ Limpiar logs antiguos   │ find /var/log -mtime...  │ maintenance cleanup     │
│                         │                          │   --target logs         │
│ Ver qué se borrará      │ (manual)                 │ maintenance cleanup     │
│                         │                          │   --dry-run             │
└──────────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────┐
│ ESTADO DEL SISTEMA                                                           │
├──────────────────────────────────────────────────────────────────────────────┤
│ Ver recursos            │ docker stats + free -h   │ maintenance status      │
│ Diagnóstico completo    │ (varios comandos)        │ diag run                │
│ Smoke test              │ (manual)                 │ maintenance smoke-test  │
└──────────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────┐
│ EJEMPLOS COMUNES                                                             │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  # Detener y limpiar un servicio                                               │
│  sudo enola-cli wp stop mi-blog                                               │
│  sudo enola-cli wp delete mi-blog                                             │
│                                                                              │
│  # Liberar recursos Docker (equivale a docker system prune)                  │
│  sudo enola-cli maintenance cleanup --target docker                          │
│                                                                              │
│  # Limpieza completa del sistema                                             │
│  sudo enola-cli maintenance cleanup --target all --force                     │
│                                                                              │
│  # Ver qué se eliminaría sin eliminar (dry-run)                              │
│  sudo enola-cli maintenance cleanup --target all --dry-run                   │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

💡 TIP: Siempre usa Enola CLI en lugar de Docker directamente.
        Enola CLI gestiona contenedores + configs como una unidad.

📚 Documentación completa: docs/CLI_REFERENCE.md
"#;
    Ok(quickref.to_string())
}

/// INT-012: Show the full proprietary license text embedded in the binary.
async fn execute_license() -> CliResult<String> {
    const LICENSE_TEXT: &str = include_str!("../../LICENSE");
    Ok(LICENSE_TEXT.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// UTILITY FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

fn format_output<T: std::fmt::Debug + serde::Serialize>(
    data: &T,
    format: &str,
) -> CliResult<String> {
    match format {
        "json" => serde_json::to_string_pretty(data)
            .map_err(|e| CliError::InvalidInput(format!("JSON serialization error: {}", e))),
        _ => Ok(format!("{:#?}", data)),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FIREWALL (UFW)
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_firewall(cmd: FirewallCommands) -> CliResult<String> {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    use std::sync::Arc;

    let manager = FirewallManager::new(Arc::new(UfwAdapter::new()));

    match cmd {
        FirewallCommands::Setup { ssh_port, force } => {
            if !force {
                println!("🛡  UFW Firewall Setup");
                println!("The following rules will be applied:");
                println!("  • Default policy: DENY incoming, ALLOW outgoing");
                println!("  • Allow SSH on port {}/tcp", ssh_port);
                println!("  • Configure DOCKER-USER chain (prevents Docker bypassing UFW)");
                println!();
                print!("Continue? [y/N] ");
                use std::io::Write;
                std::io::stdout()
                    .flush()
                    .map_err(|e| CliError::Generic(e.to_string()))?;

                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| CliError::Generic(e.to_string()))?;
                if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                    return Ok("Cancelled.".to_string());
                }
            }

            // FW-004: Scan existing Enola Docker containers for port bindings
            let existing_ports = scan_existing_enola_ports();

            manager
                .setup_secure_defaults(ssh_port, &existing_ports)
                .map_err(|e| CliError::Generic(e.to_string()))
        }

        FirewallCommands::Status => {
            let status = manager
                .get_status()
                .map_err(|e| CliError::Generic(e.to_string()))?;
            let rules_table = if status.rules.is_empty() {
                "  (no rules)".to_string()
            } else {
                status
                    .rules
                    .iter()
                    .map(|r| {
                        format!(
                            "  {}/{} \t{}\t from {}",
                            r.port,
                            r.protocol,
                            r.action,
                            r.from.as_deref().unwrap_or("anywhere")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(format!(
                "🛡  Firewall Status\n\
                 ─────────────────────────────────────\n\
                 Status:          {}\n\
                 Default in:      {}\n\
                 Default out:     {}\n\
                 DOCKER-USER:     {}\n\
                 ─────────────────────────────────────\n\
                 Rules:\n{}\n\
                 ─────────────────────────────────────\n\
                 {}",
                if status.active {
                    "✅ active"
                } else {
                    "❌ inactive — run: sudo enola-cli firewall setup"
                },
                status.default_incoming,
                status.default_outgoing,
                if status.docker_user_configured {
                    "✅ configured"
                } else {
                    "❌ not configured — run: sudo enola-cli firewall setup"
                },
                rules_table,
                if status.is_secure() {
                    "✅ Secure configuration"
                } else {
                    "⚠️  Run 'sudo enola-cli firewall setup' to harden"
                }
            ))
        }

        FirewallCommands::Allow { port, proto, from } => {
            let protocol = proto
                .parse()
                .map_err(|e: String| CliError::InvalidInput(e))?;
            manager
                .add_rule(port, protocol, from)
                .map_err(|e| CliError::Generic(e.to_string()))
        }

        FirewallCommands::Deny { port, proto } => {
            let protocol = proto
                .parse()
                .map_err(|e: String| CliError::InvalidInput(e))?;
            manager
                .deny_rule(port, protocol)
                .map_err(|e| CliError::Generic(e.to_string()))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// APPARMOR (🛡️ Sandboxing) — Tareas AA-008..010 (208-210)
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_apparmor(cmd: AppArmorCommands) -> CliResult<String> {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use crate::domain::apparmor::AppArmorMode;
    use std::sync::Arc;

    let manager = AppArmorManager::new(Arc::new(AppArmorAdapter::new()));

    match cmd {
        AppArmorCommands::Setup { mode, force } => {
            let mode: AppArmorMode = mode
                .parse()
                .map_err(|e: String| CliError::InvalidInput(e))?;

            if !force {
                println!("🛡️  AppArmor Sandboxing Setup");
                println!(
                    "The following base profiles will be loaded (mode: {}):",
                    mode
                );
                println!("  • enola-nginx     → Nginx system service");
                println!("  • enola-tor       → Tor system service");
                println!("  • enola-docker-base → Base profile for Docker containers");
                println!();
                println!("Per-service profiles will be created automatically");
                println!("when you create services with git/wp create.");
                println!();
                print!("Continue? [y/N] ");
                use std::io::Write;
                std::io::stdout()
                    .flush()
                    .map_err(|e| CliError::Generic(e.to_string()))?;

                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| CliError::Generic(e.to_string()))?;
                if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                    return Ok("Cancelled.".to_string());
                }
            }
            manager
                .setup_base_profiles(mode)
                .map_err(|e| CliError::Generic(e.to_string()))
        }

        AppArmorCommands::Status => {
            let status = manager
                .get_status()
                .map_err(|e| CliError::Generic(e.to_string()))?;

            if !status.installed {
                return Ok("🛡️  AppArmor: not installed\n  Install with: sudo apt install apparmor apparmor-utils".to_string());
            }
            if !status.enabled {
                return Ok("🛡️  AppArmor: installed but kernel module not enabled\n  On WSL2: standard kernel does not include AppArmor\n  On native Linux: check /sys/module/apparmor/parameters/enabled".to_string());
            }

            let profiles_table = if status.profiles.is_empty() {
                "  (no Enola profiles loaded)".to_string()
            } else {
                status
                    .profiles
                    .iter()
                    .map(|p| format!("  {:<30} {:<10} {}", p.name, p.mode, p.service_type))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let violations_info = if status.recent_violations.is_empty() {
                "  None".to_string()
            } else {
                status
                    .recent_violations
                    .iter()
                    .take(10)
                    .map(|v| {
                        format!(
                            "  [{}] {} {} {}",
                            v.timestamp,
                            v.operation,
                            v.profile,
                            v.path.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            Ok(format!(
                "🛡️  AppArmor Status\n\
                 ─────────────────────────────────────\n\
                 Installed:       ✅\n\
                 Kernel module:   ✅ enabled\n\
                 Profiles:        {} ({} enforce, {} complain)\n\
                 ─────────────────────────────────────\n\
                 Enola Profiles:\n{}\n\
                 ─────────────────────────────────────\n\
                 Violations (24h):\n{}",
                status.profiles.len(),
                status.enforce_count(),
                status.complain_count(),
                profiles_table,
                violations_info
            ))
        }

        AppArmorCommands::Mode {
            enforce,
            complain,
            disable,
            profile,
        } => {
            let mode = if enforce {
                AppArmorMode::Enforce
            } else if complain {
                AppArmorMode::Complain
            } else if disable {
                AppArmorMode::Disabled
            } else {
                return Err(CliError::InvalidInput(
                    "Specify one of: --enforce, --complain, or --disable".to_string(),
                ));
            };

            manager
                .change_mode(mode, profile.as_deref())
                .map_err(|e| CliError::Generic(e.to_string()))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PORTS LIST (🔌 Port Management)
// ═══════════════════════════════════════════════════════════════════════════

async fn execute_ports(cmd: PortsCommands, format: &str) -> CliResult<String> {
    match cmd {
        PortsCommands::List { json } => {
            let entries: Vec<commands::ports::PortEntry> =
                commands::ports::list_all_ports().await?;
            if json || format == "json" {
                return serde_json::to_string_pretty(&entries)
                    .map_err(|e| CliError::Generic(format!("JSON error: {}", e)));
            }
            if entries.is_empty() {
                return Ok("No Enola services found.".to_string());
            }
            let mut out = format!(
                "🔌 Ports in use by Enola services ({})\n\
                 {}\n\
                 {:<22} {:<10} {:<14} {:<7} {:<16} {}\n\
                 {}\n",
                entries.len(),
                "═".repeat(80),
                "Service",
                "Type",
                "Role",
                "Port",
                "Interface",
                "Status",
                "─".repeat(80),
            );
            for e in &entries {
                out.push_str(&format!(
                    "{:<22} {:<10} {:<14} {:<7} {:<16} {}\n",
                    e.service, e.service_type, e.role, e.port, e.interface, e.status
                ));
            }
            out.push_str(&"─".repeat(80));
            out.push_str("\n💡 Internal ports (nginx-listen, backend, api) are bound to\n");
            out.push_str("   127.0.0.1 — only accessible from localhost via Nginx/Tor.\n");
            out.push_str("   onion-http/onion-https ports are virtual (.onion URL) — no socket.\n");
            Ok(out)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DEV EXECUTOR — Solo compila con --features testing
// ═══════════════════════════════════════════════════════════════════════════
/// Ejecuta subcomandos de desarrollo.
/// Llamado desde main.rs ANTES del parse de clap para evitar conflictos de traits.
/// Args: slice de argumentos después de "dev" (e.g. &["test-token"])
#[cfg(feature = "testing")]
pub async fn execute_dev_subcommand(args: &[String]) -> CliResult<String> {
    use crate::infrastructure::test_token;
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "test-token" => {
            match test_token::generate_test_token() {
                Ok(token) => {
                    eprintln!("\x1b[1;33m⚠ BUILD CON FEATURE 'testing' — NO usar en producción\x1b[0m");
                    eprintln!("Token válido durante 5 minutos.");
                    Ok(token)
                }
                Err(e) => Err(CliError::Generic(format!("Error generando test token: {}", e))),
            }
        }
        "setup-test-key" => {
            match test_token::get_or_create_test_key() {
                Ok(key) => {
                    let key_path = dirs::home_dir()
                        .unwrap_or_default()
                        .join(".enola")
                        .join("test.key");
                    Ok(format!(
                        "✅ Clave de test en {}\n   {} bytes, permisos 0600",
                        key_path.display(),
                        key.len()
                    ))
                }
                Err(e) => Err(CliError::Generic(format!("Error: {}", e))),
            }
        }
        "verify-token" => {
            let token = args.get(1).cloned()
                .unwrap_or_else(|| std::env::var("ENOLA_TEST_TOKEN").unwrap_or_default());
            match test_token::verify_test_token(&token) {
                Ok(()) => Ok("✅ Token válido y no expirado".into()),
                Err(e) => Err(CliError::Generic(format!("❌ Token inválido: {}", e))),
            }
        }
        "--help" | "-h" | "" => Ok(
            "enola-cli dev <SUBCOMMAND>\n\nSUBCOMMANDS:\n  test-token       Genera token HMAC-SHA256 (TTL: 5min)\n  setup-test-key   Inicializa ~/.enola/test.key\n  verify-token     Verifica ENOLA_TEST_TOKEN del entorno\n\nNOTA: Solo disponible en builds --features testing".into()
        ),
        other => Err(CliError::InvalidInput(format!("Subcomando dev desconocido: '{}'", other))),
    }
}
// ═══════════════════════════════════════════════════════════════════════════
// DOCS EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════
async fn execute_docs(cmd: DocsCommands) -> CliResult<String> {
    use crate::cli::docs;
    match cmd {
        DocsCommands::Quickstart => docs::quickstart(),
        DocsCommands::Commands { group } => docs::commands_ref(group.as_deref()),
        DocsCommands::Concepts { topic } => docs::concepts(topic.as_deref()),
        DocsCommands::Faq { filter } => docs::faq(filter.as_deref()),
        DocsCommands::Examples { case } => docs::examples(case.as_deref()),
        DocsCommands::Search { term } => docs::search(&term),
        DocsCommands::QuantumSecurity => docs::quantum_security(),
        DocsCommands::VerifyDownloads => docs::verify_downloads(),
        DocsCommands::Security => docs::security(),
        DocsCommands::InstallFromIso => docs::install_from_iso(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// VPN EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════
async fn execute_vpn(cmd: VpnCommands) -> CliResult<String> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    use std::sync::Arc;

    let mgr = VpnManager::new(Arc::new(WireGuardAdapter::new()));
    match cmd {
        VpnCommands::List => {
            let interfaces = mgr
                .list_vpns()
                .map_err(|e| CliError::Generic(e.to_string()))?;
            if interfaces.is_empty() {
                return Ok("No WireGuard VPN interfaces found.\n  Create one with: sudo enola-cli vpn create wg0".to_string());
            }
            let mut out = format!("🔒 WireGuard VPN Interfaces ({})\n", interfaces.len());
            for iface in &interfaces {
                out.push_str(&format!("  • {}\n", iface));
            }
            Ok(out)
        }
        VpnCommands::Create {
            name,
            port,
            subnet,
            autostart,
            sync_firewall,
            tor,
        } => {
            crate::domain::vpn::validate_vpn_name(&name)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            println!("🔒 Creating WireGuard VPN '{}'...", name);
            let vpn_port = port.unwrap_or(51820);
            let pub_key = mgr
                .create_vpn(&name, port, subnet.as_deref(), autostart)
                .map_err(|e| CliError::Generic(e.to_string()))?;

            let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
            let _ = manifest.append("vpn_config", &name);
            if autostart {
                let _ = manifest.append("vpn_service", &format!("wg-quick@{}", name));
            }

            if sync_firewall {
                sync_vpn_firewall_allow(vpn_port);
            }

            let mut onion: Option<String> = None;
            if tor {
                use crate::adapters::infra::vpn_bridge::SocatBridgeAdapter;
                use crate::adapters::tor::TorConfigAdapter;
                use crate::application::vpn_tor_manager::VpnTorManager;

                println!("🧅 Exposing VPN '{}' through Tor...", name);
                let tor_mgr = VpnTorManager::new(
                    Arc::new(WireGuardAdapter::new()),
                    Arc::new(SocatBridgeAdapter::new()),
                    Arc::new(TorConfigAdapter::new()),
                );
                let addr = tor_mgr
                    .enable_tor(&name)
                    .await
                    .map_err(|e| CliError::Generic(e.to_string()))?;
                let _ = manifest.append("vpn_bridge", &name);
                let _ = manifest.append("tor_service", &format!("vpn-{}", name));
                onion = Some(addr);
            }

            let mut out = format!(
                "✅ VPN '{}' created and started!\n\
                 \n\
                 📋 Server public key:\n\
                 {}\n\
                 \n\
                 🔑 Add a peer:   sudo enola-cli vpn peer add {} <peer-name> --endpoint <your-ip-or-hostname>\n\
                 📊 Status:       sudo enola-cli vpn status {}\n",
                name, pub_key, name, name
            );
            if autostart {
                out.push_str(&format!("⚡ Autostart:     enabled (wg-quick@{})\n", name));
            }
            if sync_firewall {
                out.push_str(&format!(
                    "🛡  Firewall:      UFW rule added (allow {}/udp)\n",
                    vpn_port
                ));
            } else {
                out.push_str(&format!(
                    "💡 Firewall:      NOT synced. To allow VPN traffic:\n\
                     \x20                 sudo ufw allow {}/udp\n",
                    vpn_port
                ));
            }
            if let Some(addr) = onion {
                out.push_str(&format!(
                    "🧅 Tor:           {}\n\
                     \x20                 Add a Tor peer: sudo enola-cli vpn peer add {} <peer-name> --endpoint <ip> --tor\n",
                    addr, name
                ));
            }
            Ok(out)
        }
        VpnCommands::Start { name } => {
            mgr.start_vpn(&name)
                .map_err(|e| CliError::Generic(e.to_string()))?;
            Ok(format!("✅ VPN '{}' started", name))
        }
        VpnCommands::Stop { name } => {
            mgr.stop_vpn(&name)
                .map_err(|e| CliError::Generic(e.to_string()))?;
            Ok(format!("✅ VPN '{}' stopped", name))
        }
        VpnCommands::Status { name } => {
            let status = mgr
                .get_status(&name)
                .map_err(|e| CliError::Generic(e.to_string()))?;
            let mut out = mgr.format_status(&status);

            use crate::adapters::infra::vpn_bridge::SocatBridgeAdapter;
            use crate::adapters::tor::TorConfigAdapter;
            use crate::application::vpn_tor_manager::VpnTorManager;
            use crate::ports::vpn_bridge::VpnBridgePort;

            if SocatBridgeAdapter::new().is_bridge_active(&name) {
                let tor_mgr = VpnTorManager::new(
                    Arc::new(WireGuardAdapter::new()),
                    Arc::new(SocatBridgeAdapter::new()),
                    Arc::new(TorConfigAdapter::new()),
                );
                if let Ok(onion) = tor_mgr.get_onion(&name).await {
                    out.push_str(&format!("\n🧅 Tor onion:    {}\n", onion));
                }
            }
            Ok(out)
        }
        VpnCommands::Delete {
            name,
            force,
            sync_firewall,
        } => {
            if !force {
                return Err(CliError::InvalidInput(
                    format!("This will permanently delete VPN '{}' and all peer configs.\nUse --force to confirm.", name)
                ));
            }
            let vpn_port = if sync_firewall {
                mgr.get_status(&name).map(|s| s.listen_port).ok()
            } else {
                None
            };
            mgr.delete_vpn(&name)
                .map_err(|e| CliError::Generic(e.to_string()))?;
            let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
            let _ = manifest.remove("vpn_config", &name);
            let _ = manifest.remove("vpn_service", &format!("wg-quick@{}", name));

            // Remove Tor exposure (hidden service + socat bridge) if present.
            {
                use crate::adapters::infra::vpn_bridge::SocatBridgeAdapter;
                use crate::adapters::tor::TorConfigAdapter;
                use crate::application::vpn_tor_manager::VpnTorManager;
                let tor_mgr = VpnTorManager::new(
                    Arc::new(WireGuardAdapter::new()),
                    Arc::new(SocatBridgeAdapter::new()),
                    Arc::new(TorConfigAdapter::new()),
                );
                let _ = tor_mgr.disable_tor(&name).await;
            }
            let _ = manifest.remove("vpn_bridge", &name);
            let _ = manifest.remove("tor_service", &format!("vpn-{}", name));

            if let Some(port) = vpn_port {
                sync_vpn_firewall_remove(port);
            }
            let mut out = format!(
                "✅ VPN '{}' deleted (interface stopped, config removed)",
                name
            );
            if let Some(port) = vpn_port {
                out.push_str(&format!("\n🛡  Firewall: UFW rule for {}/udp removed", port));
            }
            Ok(out)
        }
        VpnCommands::Peer(peer_cmd) => match peer_cmd {
            VpnPeerCommands::Add {
                interface,
                peer_name,
                endpoint,
                dns,
                psk,
                ip,
                tor,
            } => {
                crate::domain::vpn::validate_vpn_name(&interface)
                    .map_err(|e| CliError::InvalidInput(e.to_string()))?;

                let status = mgr
                    .get_status(&interface)
                    .map_err(|e| CliError::Generic(e.to_string()))?;

                let server_port = status.listen_port;
                let server_pub_key = status.public_key.clone();

                let peer_ip = if let Some(explicit_ip) = ip {
                    explicit_ip
                } else {
                    let mut tmp_server =
                        crate::domain::vpn::VpnServer::new(&interface, server_port, "10.8.0.0/24");
                    for p in &status.peers {
                        tmp_server.peers.push(crate::domain::vpn::VpnPeer::new(
                            &p.public_key,
                            &p.public_key,
                            p.allowed_ips
                                .first()
                                .map(|s| s.trim_end_matches("/32"))
                                .unwrap_or("10.8.0.2"),
                        ));
                    }
                    tmp_server
                        .next_peer_ip()
                        .ok_or_else(|| CliError::Generic("VPN subnet is full".to_string()))?
                };

                // Resolve the onion before adding the peer so the command is
                // atomic: if the VPN has no Tor exposure, fail without adding.
                let onion = if tor {
                    use crate::adapters::infra::vpn_bridge::SocatBridgeAdapter;
                    use crate::adapters::tor::TorConfigAdapter;
                    use crate::application::vpn_tor_manager::VpnTorManager;

                    let tor_mgr = VpnTorManager::new(
                        Arc::new(WireGuardAdapter::new()),
                        Arc::new(SocatBridgeAdapter::new()),
                        Arc::new(TorConfigAdapter::new()),
                    );
                    Some(tor_mgr.get_onion(&interface).await.map_err(|e| {
                        CliError::Generic(format!(
                            "Cannot get Tor onion for VPN '{}'. Create it with --tor first: {}",
                            interface, e
                        ))
                    })?)
                } else {
                    None
                };

                println!("🔑 Generating key pair for peer '{}'...", peer_name);
                let client_config = mgr
                    .add_peer(
                        &interface,
                        &peer_name,
                        &endpoint,
                        server_port,
                        &server_pub_key,
                        &peer_ip,
                        psk,
                        dns.as_deref(),
                    )
                    .map_err(|e| CliError::Generic(e.to_string()))?;

                let mut out = format!(
                    "✅ Peer '{}' added to VPN '{}' (IP: {})\n\
                     \n\
                     ─── Client Configuration (direct) ──────────────────\n\
                     {}\n\
                     ────────────────────────────────────────────────────\n\
                     💡 Save this config to /etc/wireguard/{}-client.conf on the remote device\n\
                     💡 Or use 'qrencode -t ansiutf8' to display as QR code for mobile\n",
                    peer_name, interface, peer_ip, client_config, peer_name,
                );

                if let Some(onion) = onion {
                    // Tor config: same as direct but Endpoint points to the local
                    // socat bridge (127.0.0.1) instead of the public endpoint.
                    let tor_config = client_config
                        .lines()
                        .map(|line| {
                            if line.trim_start().starts_with("Endpoint = ") {
                                format!("Endpoint = 127.0.0.1:{}", server_port)
                            } else {
                                line.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    out.push_str(&format!(
                        "\n\n\
                         ─── Client Configuration (Tor) ─────────────────────\n\
                         {}\n\
                         ────────────────────────────────────────────────────\n\
                         🧅 Onion: {}\n\
                         \n\
                         📋 Client-side setup (Linux/macOS):\n\
                         \x20  sudo apt install tor socat\n\
                         \x20  sudo systemctl start tor\n\
                         \x20  socat UDP-LISTEN:{},fork SOCKS4A:127.0.0.1:{}:{},socksport=9050\n\
                         \n\
                         💡 Then import the Tor config above into WireGuard.\n",
                        tor_config, onion, server_port, onion, server_port,
                    ));
                }

                Ok(out)
            }
            VpnPeerCommands::AddPubkey {
                interface,
                peer_name,
                public_key,
                ip,
            } => {
                mgr.add_peer_by_pubkey(&interface, &peer_name, &public_key, &ip)
                    .map_err(|e| CliError::Generic(e.to_string()))?;
                Ok(format!(
                    "✅ Peer '{}' (pubkey) added to VPN '{}' (IP: {})",
                    peer_name, interface, ip
                ))
            }
            VpnPeerCommands::Remove {
                interface,
                public_key,
            } => {
                mgr.remove_peer(&interface, &public_key)
                    .map_err(|e| CliError::Generic(e.to_string()))?;
                Ok(format!("✅ Peer removed from VPN '{}'", interface))
            }
        },
    }
}

async fn execute_setup(all: bool, vpn: bool, security: bool, pqc_tls: bool) -> CliResult<String> {
    use crate::adapters::infra::dependencies::SystemDependencyAdapter;
    use crate::application::dependency_manager::DependencyManager;
    use crate::domain::dependencies::SetupScope;
    use std::sync::Arc;
    use tokio::process::Command;

    let scope = if all {
        SetupScope::All
    } else if vpn {
        SetupScope::Vpn
    } else if security {
        SetupScope::Security
    } else {
        SetupScope::Core
    };

    let adapter = Arc::new(SystemDependencyAdapter::new());
    let mgr = DependencyManager::new(adapter);

    let result = mgr
        .setup(scope)
        .map_err(|e| CliError::Generic(e.to_string()))?;

    let mut out = mgr.format_setup_result(&result);

    if pqc_tls {
        let script_path = "/tmp/enola_install_pqc_tls_stack.sh";
        tokio::fs::write(
            script_path,
            crate::infrastructure::pqc_tls::embedded_installer_script(),
        )
        .await
        .map_err(|e| CliError::Generic(format!("Failed to write PQC TLS installer: {}", e)))?;

        let status = Command::new("chmod")
            .args(["700", script_path])
            .status()
            .await
            .map_err(|e| CliError::Generic(format!("Failed to chmod PQC TLS installer: {}", e)))?;
        if !status.success() {
            return Err(CliError::Generic(
                "Failed to chmod PQC TLS installer".to_string(),
            ));
        }

        let output = Command::new("bash")
            .arg(script_path)
            .output()
            .await
            .map_err(|e| CliError::Generic(format!("Failed to run PQC TLS installer: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(CliError::Generic(format!(
                "PQC TLS installer failed:\n{}{}",
                stdout, stderr,
            )));
        }

        out.push('\n');
        out.push_str(&stdout);
    }

    Ok(out)
}

async fn execute_doctor(security: bool) -> CliResult<String> {
    if security {
        let (report, exit_code) = crate::application::system_doctor::security_report();
        if exit_code == 0 {
            Ok(report)
        } else {
            Err(CliError::ControlledExit {
                code: exit_code,
                stdout: Some(report),
                stderr: None,
            })
        }
    } else {
        Ok(crate::application::system_doctor::full_report().await)
    }
}

pub(crate) async fn execute_uninstall(
    yes: bool,
    keep_data: bool,
    only: Option<String>,
    force: bool,
    remove_deps: bool,
) -> crate::cli::commands::CliResult<String> {
    use crate::cli::commands::CliError;
    use crate::domain::error::EnolaError;
    use std::path::PathBuf;
    use std::process::Command;

    let candidates: Vec<PathBuf> = vec![
        PathBuf::from("/usr/local/share/enola/uninstall.sh"),
        PathBuf::from("scripts/ops/uninstall.sh"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("../share/enola/uninstall.sh")))
            .unwrap_or_else(|| PathBuf::from("scripts/ops/uninstall.sh")),
    ];
    let script = candidates.into_iter().find(|p| p.exists())
        .ok_or_else(|| CliError::Domain(EnolaError::InfrastructureError(
            "No se encontró scripts/ops/uninstall.sh ni /usr/local/share/enola/uninstall.sh. Reinstala el CLI o descarga la release completa.".to_string()
        )))?;

    let mut args: Vec<String> = vec![script.to_string_lossy().to_string()];
    if yes {
        args.push("--yes".to_string());
    }
    if keep_data {
        args.push("--keep-data".to_string());
    }
    if force {
        args.push("--force".to_string());
    }
    if remove_deps {
        args.push("--remove-deps".to_string());
    }
    if let Some(sections) = only {
        args.push("--only".to_string());
        args.push(sections);
    }

    let status = Command::new("bash").args(&args).status().map_err(|e| {
        CliError::Domain(EnolaError::InfrastructureError(format!(
            "No se pudo lanzar uninstall.sh: {}",
            e
        )))
    })?;
    if !status.success() {
        return Err(CliError::Domain(EnolaError::InfrastructureError(format!(
            "uninstall.sh terminó con código {} (ver salida arriba)",
            status.code().unwrap_or(-1)
        ))));
    }
    Ok(String::new())
}

async fn execute_update(cmd: crate::cli::UpdateCommands) -> CliResult<String> {
    use crate::application::update_checker;
    use crate::cli::commands::CliError;
    use crate::cli::UpdateCommands;
    match cmd {
        UpdateCommands::Check { json, force } => {
            let url = update_checker::feed_url();
            let report = update_checker::check_for_updates_with_options(&url, force).await;
            let rendered = if json {
                serde_json::to_string_pretty(&report.json_value().map_err(|e| {
                    CliError::Generic(format!("failed to build update report JSON: {}", e))
                })?)
                .map_err(|e| {
                    CliError::Generic(format!("failed to render update report as JSON: {}", e))
                })?
            } else {
                report.human_summary()
            };
            if report.exit_code() == update_checker::UPDATE_EXIT_OK {
                Ok(rendered)
            } else {
                Err(CliError::ControlledExit {
                    code: report.exit_code(),
                    stdout: Some(rendered),
                    stderr: None,
                })
            }
        }
        UpdateCommands::Schema { json } => {
            let schema = r#"{
  "schema_version": "1",
  "latest": "1.5.0",
  "min_supported": "1.4.0",
  "download_url": "<download-url>",
  "published_at": "2026-05-07T12:00:00Z",
  "docs_url": "<optional-docs-url>",
  "severity_summary": {
    "critical": 0,
    "high": 1,
    "medium": 0,
    "low": 0
  },
  "signature_urls": [
    "<optional-sidecar-url-1>",
    "<optional-sidecar-url-2>"
  ],
  "advisories": [
    {
      "id": "ENOLA-ADV-2026-001",
      "severity": "critical|high|medium|low",
      "title": "Short advisory title",
      "description": "Detailed technical description",
      "affected_versions": [">=1.3.0", "<1.5.0"],
      "fixed_in": "1.5.0"
    }
  ],
  "pqc_milestones": [
    {
      "id": "PQC-TOR-ARTI",
      "status": "pending|released",
      "description": "Tor arti with ML-KEM-768 hybrid",
      "target_version": "2.0.0"
    }
  ]
}"#;
            if json {
                let payload = serde_json::json!({
                    "schema_version": "1",
                    "sample_feed": serde_json::from_str::<serde_json::Value>(schema)
                        .map_err(|e| CliError::Generic(format!("failed to parse embedded update schema JSON: {}", e)))?,
                    "signature": {
                        "default_sidecar_suffix": ".minisig",
                        "override_env": "ENOLA_UPDATE_SIGNATURE_URL",
                        "override_config_key": "update.signature_url",
                        "public_key_sources": [
                            "ENOLA_UPDATE_MINISIGN_PUBKEY",
                            "update.minisign_pubkey",
                            "embedded minisign.pub"
                        ]
                    },
                    "exit_codes": {
                        "ok": update_checker::UPDATE_EXIT_OK,
                        "critical_advisory": update_checker::UPDATE_EXIT_CRITICAL_ADVISORY,
                        "below_min_supported": update_checker::UPDATE_EXIT_BELOW_MIN_SUPPORTED,
                        "feed_invalid": update_checker::UPDATE_EXIT_FEED_INVALID,
                        "signature_invalid": update_checker::UPDATE_EXIT_SIGNATURE_INVALID
                    }
                });
                serde_json::to_string_pretty(&payload).map_err(|e| {
                    CliError::Generic(format!("failed to render update schema JSON: {}", e))
                })
            } else {
                Ok(format!(
                    "Advisory feed schema v1 (UPD-FEED-001)\n\
                     Serve this JSON at your update feed URL.\n\
                     Optional v1 fields supported: published_at, docs_url, severity_summary, signature_urls.\n\
                     Sidecar signature: {{feed_url}}.minisig (or override with [update].signature_url).\n\
                     Verification key: [update].minisign_pubkey or embedded minisign.pub.\n\
                     Exit codes: 0=ok, 11=critical advisory, 12=below min_supported, 20=feed invalid, 21=signature invalid.\n\
                     Configure: ENOLA_UPDATE_FEED_URL or [update] feed_url in ~/.enola/config.toml\n\n\
                     {}",
                    schema
                ))
            }
        }
        UpdateCommands::VerifyFeed {
            source,
            signature,
            json,
        } => {
            let report = update_checker::verify_feed_source(&source, signature.as_deref()).await;
            let rendered = if json {
                serde_json::to_string_pretty(&report.json_value().map_err(|e| {
                    CliError::Generic(format!("failed to build verify-feed JSON: {}", e))
                })?)
                .map_err(|e| {
                    CliError::Generic(format!("failed to render verify-feed JSON: {}", e))
                })?
            } else {
                report.human_summary()
            };

            if report.exit_code() == update_checker::UPDATE_EXIT_OK {
                Ok(rendered)
            } else {
                Err(CliError::ControlledExit {
                    code: report.exit_code(),
                    stdout: Some(rendered),
                    stderr: None,
                })
            }
        }
        UpdateCommands::Download {
            yes,
            dry_run,
            json,
            force,
            allow_unsigned,
        } => {
            if dry_run {
                let result = update_checker::dry_run_update(force).await;
                match result {
                    Ok(dl) => {
                        let msg = format!(
                            "Dry-run: would download version {} → {}\n  Download URL: {}\n  Current binary: {}",
                            dl.current_version, dl.latest_version, dl.download_url,
                            std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
                        );
                        if json {
                            serde_json::to_string_pretty(&dl).map_err(|e| {
                                CliError::Generic(format!("download dry-run JSON: {}", e))
                            })
                        } else {
                            Ok(msg)
                        }
                    }
                    Err(e) => Err(CliError::Generic(format!("update download dry-run: {}", e))),
                }
            } else {
                if allow_unsigned {
                    std::env::set_var("ENOLA_ALLOW_UNSIGNED_UPDATE", "1");
                }
                let result = update_checker::download_update(force).await;
                match result {
                    Ok(dl) => {
                        let sig_status = if dl.signature_verified {
                            "✅ verified"
                        } else {
                            "⚠️ not verified (minisign unavailable)"
                        };
                        let mut msg = format!(
                            "✅ Downloaded enola-cli {} → {}\n  Binary: {}\n  SHA256: {}\n  Signature: {}",
                            dl.current_version, dl.latest_version, dl.binary_path, dl.sha256, sig_status
                        );
                        if yes {
                            match update_checker::apply_update(Some(&dl.binary_path)) {
                                Ok(applied) => {
                                    msg.push_str(&format!("\n\n✅ Applied! Binary replaced at: {}\n  Backup: enola-cli.bak\n  SHA256 updated.", applied.binary_path));
                                    if json {
                                        let mut val = serde_json::to_value(&dl).map_err(|e| CliError::Generic(format!("download JSON: {}", e)))?;
                                        if let serde_json::Value::Object(ref mut m) = val {
                                            m.insert("applied".to_string(), serde_json::json!(true));
                                            m.insert("install_path".to_string(), serde_json::json!(applied.binary_path));
                                        }
                                        serde_json::to_string_pretty(&val).map_err(|e| CliError::Generic(format!("download JSON: {}", e)))
                                    } else {
                                        Ok(msg)
                                    }
                                }
                                Err(e) => {
                                    Err(CliError::Generic(format!("download succeeded but apply failed: {}\n  Binary saved at: {}", e, dl.binary_path)))
                                }
                            }
                        } else {
                            msg.push_str("\n\nTo apply: sudo enola-cli update apply --binary ");
                            msg.push_str(&dl.binary_path);
                            if json {
                                serde_json::to_string_pretty(&dl)
                                    .map_err(|e| CliError::Generic(format!("download JSON: {}", e)))
                            } else {
                                Ok(msg)
                            }
                        }
                    }
                    Err(e) => Err(CliError::Generic(format!("update download: {}", e))),
                }
            }
        }
        UpdateCommands::Apply {
            binary,
            json,
            allow_unsigned,
        } => {
            if allow_unsigned {
                std::env::set_var("ENOLA_ALLOW_UNSIGNED_UPDATE", "1");
            }
            if std::env::var("ENOLA_SKIP_ROOT_CHECK").is_err() {
                if std::fs::metadata("/usr/local/bin/enola-cli").is_err() {
                    // Not installed in standard location — allow anyway
                } else if std::env::var("USER").unwrap_or_default() != "root"
                    && unsafe { libc::geteuid() } != 0
                {
                    return Err(CliError::Generic(
                        "update apply requires root. Run with sudo.".to_string(),
                    ));
                }
            }
            let result = update_checker::apply_update(binary.as_deref());
            match result {
                Ok(applied) => {
                    let msg = format!(
                        "✅ Update applied!\n  Binary: {}\n  SHA256: {}\n  Backup: enola-cli.bak",
                        applied.binary_path, applied.sha256
                    );
                    if json {
                        serde_json::to_string_pretty(&applied)
                            .map_err(|e| CliError::Generic(format!("apply JSON: {}", e)))
                    } else {
                        Ok(msg)
                    }
                }
                Err(e) => Err(CliError::Generic(format!("update apply: {}", e))),
            }
        }
    }
}
