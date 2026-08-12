// Enola CLI Binary
// Command-line interface for Enola Server management

use clap::Parser;
use enola_core::adapters::infra::security::SecurityAdapter;
use enola_core::cli::{executor, Cli, Commands};
use std::error::Error;

// Build metadata (embebidos por build.rs)
const GIT_HASH: &str = env!("ENOLA_GIT_HASH");
const BUILD_DATE: &str = env!("ENOLA_BUILD_DATE");

/// INT-008: Self-integrity check — verifica que el binario no fue modificado
/// tras la instalación. Compara SHA256 del ejecutable actual con el hash
/// guardado por post_install.sh en /usr/local/share/enola/cli.sha256.
/// Si el archivo de referencia no existe → skip silencioso.
/// Si existe y no coincide → warning (no aborta).
fn check_self_integrity() {
    use std::io::Read;

    // Leer el ejecutable actual vía /proc/self/exe
    let exe_path = match std::fs::read_link("/proc/self/exe") {
        Ok(p) => p,
        Err(_) => return, // No disponible (no Linux)
    };

    let mut exe_data = Vec::new();
    if std::fs::File::open(&exe_path)
        .and_then(|mut f| f.read_to_end(&mut exe_data))
        .is_err()
    {
        return;
    }

    // Calcular SHA256 del binario
    use sha2::{Digest, Sha256};
    let actual_hash = format!("{:x}", Sha256::digest(&exe_data));

    // Leer hash esperado desde archivo instalado por post_install.sh
    let hash_file = "/usr/local/share/enola/cli.sha256";
    let expected = match std::fs::read_to_string(hash_file) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return, // Archivo no existe → instalación sin verificación → skip
    };

    if !expected.is_empty() && actual_hash != expected {
        eprintln!("\x1b[1;33m⚠️  [INT-008] Binary integrity check FAILED.\x1b[0m");
        eprintln!("\x1b[1;33m   The installed binary has been modified since installation.\x1b[0m");
        eprintln!(
            "\x1b[1;33m   Expected: {}...  Got: {}...\x1b[0m",
            &expected[..16],
            &actual_hash[..16]
        );
        eprintln!(
            "\x1b[1;33m   Build: {} ({}). Reinstall from the official source.\x1b[0m",
            GIT_HASH, BUILD_DATE
        );
    }
}

/// Check if running as root (EUID == 0). Exempt commands that don't require root.
fn check_root_permissions(cli: &Cli) -> Result<(), Box<dyn Error>> {
    use enola_core::cli::Commands;
    // Comandos que NO requieren root (solo lectura/verificación o sin efectos de sistema)
    let exempt = matches!(
        &cli.command,
        Commands::Quickref
        | Commands::License
        | Commands::Doctor { .. }
        | Commands::ConfigShow { .. }
        | Commands::ConfigValidate { .. }
        | Commands::Docs(_)
        | Commands::Verify { .. } // RELEASE-VERIFY (PQC-030): verificar descargas no requiere root
        | Commands::Update(_)
    );
    if exempt {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let euid = unsafe { libc::geteuid() };
        if euid != 0 {
            eprintln!("\n\x1b[1;31m╔════════════════════════════════════════════════════════════════╗\x1b[0m");
            eprintln!("\x1b[1;31m║                    ⚠️  ROOT REQUIRED                            ║\x1b[0m");
            eprintln!("\x1b[1;31m╚════════════════════════════════════════════════════════════════╝\x1b[0m\n");
            eprintln!("\x1b[1;33mEnola CLI requires root privileges to manage services.\x1b[0m\n");
            eprintln!("\x1b[1;36mSolution:\x1b[0m Run with sudo:\n");
            eprintln!("    \x1b[1;32msudo enola-cli <command>\x1b[0m\n");
            return Err("Root privileges required. Run with: sudo enola-cli".into());
        }
    }
    Ok(())
}

/// INT-004: Security check — warn if a debugger is attached (TracerPid != 0)
fn check_security() {
    let security = SecurityAdapter::new();
    if security.check_debugger().is_err() {
        eprintln!(
            "\x1b[1;33m⚠️  Warning: Debugger detected. Some features may be restricted.\x1b[0m"
        );
    }
}

/// CONFIG-003: Exporta los overrides de CLI como env vars para que los
/// componentes que ya leen env vars (DistributionSettings::load,
/// WebSettings::load) los respeten sin cambios.
/// Prioridad: flag > env existente > archivo config.toml > default.
fn apply_global_overrides(cli: &Cli) {
    if let Some(v) = cli.binary_base_url.as_deref() {
        std::env::set_var("ENOLA_BINARY_BASE_URL", v);
    }
    if let Some(v) = cli.web_url.as_deref() {
        std::env::set_var("ENOLA_WEB_URL", v);
    }
    if let Some(v) = cli.docs_url.as_deref() {
        std::env::set_var("ENOLA_DOCS_URL", v);
    }
    // QUAL-003: --tor-socks → ENOLA_TOR_SOCKS_PROXY (consumido por infrastructure/http.rs)
    if let Some(v) = cli.tor_socks.as_deref() {
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v);
    }
}

/// UPD-CLI-002: decide si se puede mostrar el aviso automático de updates al
/// final del comando sin romper salidas JSON/scriptables ni comandos sensibles.
fn should_show_update_hint(cli: &Cli) -> bool {
    if cli.format == "json" {
        return false;
    }

    match &cli.command {
        // Nunca en el propio comando update.
        Commands::Update(_) => false,

        // Comandos con bandera JSON propia.
        Commands::ConfigShow { json } => !json,

        Commands::ConfigValidate { json, .. } => !json,

        // Comandos informativos/scriptables donde no queremos ruido extra.
        Commands::Docs(_)
        | Commands::Quickref
        | Commands::License
        | Commands::Doctor { .. }
        | Commands::Uninstall { .. }
        | Commands::Verify { .. } => false,

        // Resto: sí, salida humana.
        _ => true,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // LAUNCH-015: Endurecer proceso ANTES de cargar cualquier secreto.
    // - prctl(PR_SET_DUMPABLE, 0): bloquea core dumps + ptrace attach.
    // - detect_tracer(): aborta si gdb/strace/ltrace está adjunto al arrancar.
    enola_core::infrastructure::anti_debug::harden_process();
    if enola_core::infrastructure::anti_debug::detect_tracer() {
        eprintln!("\x1b[1;31merror:\x1b[0m external debugger detected");
        std::process::exit(2);
    }

    // INT-008: Self-integrity check (warning only, does not abort)
    check_self_integrity();

    // INT-004: Security check (warning only, does not abort)
    check_security();

    // ── Dev subcommand interceptor (solo en builds --features testing) ──
    // Los comandos `dev` se manejan antes del parse de clap para evitar
    // problemas de derives en producción. El enum DevCommands NO está
    // en Commands para que el build de producción no tenga superficie de ataque.
    #[cfg(feature = "testing")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(|s| s.as_str()) == Some("dev") {
            let rt = tokio::runtime::Runtime::new()?;
            let result = rt.block_on(enola_core::cli::executor::execute_dev_subcommand(
                &args[2..],
            ));
            match result {
                Ok(output) => {
                    println!("{}", output);
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("\x1b[1;31mError:\x1b[0m {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    // Parse CLI arguments
    let cli = Cli::parse();

    // Check root permissions (after parse to exempt commands like verify)
    check_root_permissions(&cli)?;

    // CONFIG-003: Propagar flags globales a env vars para que los adapters
    // que ya leen env vars (DistributionSettings, WebSettings)
    // honren los overrides del usuario sin refactorizar cadena env>file>default.
    apply_global_overrides(&cli);
    let show_update_hint = should_show_update_hint(&cli);

    // Create tokio runtime and execute
    let rt = tokio::runtime::Runtime::new()?;

    let result = rt.block_on(async {
        let output = executor::execute(cli).await?;
        let update_hint = if show_update_hint {
            // No bloquear el flujo principal: si el check tarda demasiado,
            // se omite silenciosamente en esta ejecución.
            tokio::time::timeout(
                std::time::Duration::from_millis(800),
                enola_core::application::update_checker::background_update_hint(),
            )
            .await
            .ok()
            .flatten()
        } else {
            None
        };

        Ok::<(String, Option<String>), enola_core::cli::commands::CliError>((output, update_hint))
    });

    match result {
        Ok((output, update_hint)) => {
            println!("{}", output);
            if let Some(hint) = update_hint {
                if !hint.trim().is_empty() {
                    eprintln!("\n{}", hint);
                }
            }
            Ok(())
        }
        Err(e) => match e {
            enola_core::cli::commands::CliError::ControlledExit {
                code,
                stdout,
                stderr,
            } => {
                if let Some(output) = stdout {
                    println!("{}", output);
                }
                if let Some(output) = stderr {
                    eprintln!("{}", output);
                }
                std::process::exit(code);
            }
            other => {
                eprintln!("\x1b[1;31mError:\x1b[0m {}", other);
                std::process::exit(1);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::should_show_update_hint;
    use enola_core::cli::{Cli, Commands, TorCommands, UpdateCommands};

    fn mk_cli(command: Commands) -> Cli {
        Cli {
            command,
            format: "text".to_string(),
            verbose: false,
            binary_base_url: None,
            web_url: None,
            docs_url: None,
            tor_socks: None,
        }
    }

    #[test]
    fn update_hint_disabled_for_global_json_format() {
        let mut cli = mk_cli(Commands::Tor(TorCommands::List));
        cli.format = "json".to_string();
        assert!(!should_show_update_hint(&cli));
    }

    #[test]
    fn update_hint_disabled_for_update_command() {
        let cli = mk_cli(Commands::Update(UpdateCommands::Schema { json: false }));
        assert!(!should_show_update_hint(&cli));
    }

    #[test]
    fn update_hint_disabled_for_config_show_json() {
        let cli = mk_cli(Commands::ConfigShow { json: true });
        assert!(!should_show_update_hint(&cli));
    }

    #[test]
    fn update_hint_enabled_for_normal_human_command() {
        let cli = mk_cli(Commands::Tor(TorCommands::List));
        assert!(should_show_update_hint(&cli));
    }
}
