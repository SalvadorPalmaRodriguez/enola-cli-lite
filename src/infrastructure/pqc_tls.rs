pub(crate) const PQC_TLS_MARKER: &str = "/usr/local/share/enola/pqc_tls.env";
pub(crate) const PQC_TLS_GROUPS: &str = "X25519MLKEM768:X25519:prime256v1";
const OPENSSL_PREFIX: &str = "OpenSSL ";
const REQUIRED_OPENSSL_MAJOR_MINOR: &str = "3.5";
const INSTALLER_SCRIPT: &str = include_str!("../../scripts/ops/install_pqc_tls_stack.sh");

pub(crate) fn embedded_installer_script() -> &'static str {
    INSTALLER_SCRIPT
}

/// R-8/G-2 + SEC-001: escribe el instalador embebido en un directorio temporal
/// privado (0700, nombre aleatorio) y devuelve el `TempDir` junto a la ruta del
/// script (modo 0700, listo para `bash`). Una ruta fija bajo `/tmp` sería un
/// vector de pre-plantado/symlink para código que corre como root: `fs::write`
/// sigue ficheros ajenos y conserva al dueño original.
///
/// El `TempDir` devuelto DEBE mantenerse vivo hasta que el proceso del
/// instalador termine (`child.wait()`); al soltarse, el directorio se borra.
pub(crate) fn stage_installer() -> std::io::Result<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempfile::tempdir()?;
    // `tempdir()` crea con 0o777 & !umask — forzar 0700 explícito para que el
    // directorio sea realmente privado aunque el umask sea laxo.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let script = dir.path().join("enola_install_pqc_tls_stack.sh");
    write_executable_private(&script, embedded_installer_script().as_bytes())?;
    Ok((dir, script))
}

#[cfg(unix)]
fn write_executable_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // O_EXCL + modo en el open: sin ventana entre create y chmod.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(not(unix))]
fn write_executable_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

fn command_output(bin: &str, args: &[&str], use_stderr: bool) -> Option<String> {
    // SEC-004: probes de versión (`openssl version`, `nginx -V`) — sin root.
    let output = crate::infrastructure::drop_privs::command_as_invoking_user(bin)
        .args(args)
        .output()
        .ok()?;
    let bytes = if use_stderr && !output.stderr.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    Some(String::from_utf8_lossy(bytes).trim().to_string())
}

fn has_required_openssl_prefix(version: &str) -> bool {
    version.starts_with(REQUIRED_OPENSSL_MAJOR_MINOR)
}

pub(crate) fn parse_openssl_version(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    let rest = line.strip_prefix(OPENSSL_PREFIX)?;
    Some(rest.split_whitespace().next()?.to_string())
}

pub(crate) fn parse_nginx_built_with_openssl(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.split_once("built with OpenSSL ").map(|(_, rest)| rest))
        .and_then(|rest| rest.split_whitespace().next().map(|s| s.to_string()))
}

pub(crate) fn installed_openssl_version() -> Option<String> {
    command_output("openssl", &["version"], false).and_then(|s| parse_openssl_version(&s))
}

pub(crate) fn nginx_linked_openssl_version() -> Option<String> {
    command_output("nginx", &["-V"], true).and_then(|s| parse_nginx_built_with_openssl(&s))
}

pub(crate) fn openssl_35_available() -> bool {
    openssl_35_available_from(installed_openssl_version())
}

pub(crate) fn nginx_pqc_ready() -> bool {
    nginx_pqc_ready_from(nginx_linked_openssl_version())
}

pub(crate) fn pqc_tls_available() -> bool {
    pqc_tls_available_with(
        std::path::Path::new(PQC_TLS_MARKER).exists(),
        openssl_35_available(),
        nginx_pqc_ready(),
    )
}

pub(crate) fn nginx_pqc_curve_directive() -> String {
    nginx_pqc_curve_directive_with(pqc_tls_available())
}

fn nginx_pqc_curve_directive_with(available: bool) -> String {
    if available {
        format!("    ssl_ecdh_curve {};\n", PQC_TLS_GROUPS)
    } else {
        String::new()
    }
}

fn openssl_35_available_from(version: Option<String>) -> bool {
    version
        .as_deref()
        .map(has_required_openssl_prefix)
        .unwrap_or(false)
}

fn nginx_pqc_ready_from(version: Option<String>) -> bool {
    version
        .as_deref()
        .map(has_required_openssl_prefix)
        .unwrap_or(false)
}

fn pqc_tls_available_with(marker_exists: bool, openssl_ok: bool, nginx_ok: bool) -> bool {
    marker_exists && openssl_ok && nginx_ok
}

#[allow(dead_code)]
fn doctor_section_with(
    openssl: String,
    nginx: String,
    openssl_ok: bool,
    nginx_ok: bool,
    pqc_ok: bool,
) -> String {
    let mut out = String::from("\n── PQC TLS stack ──\n");
    out.push_str(&format!(
        "  {} {:<20} {}\n",
        if openssl_ok { "✅" } else { "❌" },
        "openssl-pqc",
        openssl,
    ));
    out.push_str(&format!(
        "  {} {:<20} {}\n",
        if nginx_ok { "✅" } else { "❌" },
        "nginx-pqc",
        nginx,
    ));
    if !pqc_ok {
        out.push_str("\n💡 Enable PQC TLS with:\n   sudo enola-cli setup --pqc-tls\n");
    }
    out
}

pub(crate) fn doctor_section() -> String {
    let openssl = installed_openssl_version().unwrap_or_else(|| "not found".to_string());
    let nginx =
        nginx_linked_openssl_version().unwrap_or_else(|| "not linked to OpenSSL 3.5".to_string());
    let openssl_ok = openssl_35_available();
    let nginx_ok = nginx_pqc_ready();
    let pqc_ok = pqc_tls_available();
    doctor_section_with(openssl, nginx, openssl_ok, nginx_ok, pqc_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_openssl_standard_version() {
        assert_eq!(
            parse_openssl_version("OpenSSL 3.5.6 7 Apr 2026 (Library: OpenSSL 3.5.6 7 Apr 2026)"),
            Some("3.5.6".to_string())
        );
    }

    #[test]
    fn parse_nginx_build_info() {
        let sample = "nginx version: nginx/1.28.3\nbuilt with OpenSSL 3.5.6 7 Apr 2026\nTLS SNI support enabled";
        assert_eq!(
            parse_nginx_built_with_openssl(sample),
            Some("3.5.6".to_string())
        );
    }

    #[test]
    fn pqc_curve_directive_constant_contains_mlkem() {
        assert!(PQC_TLS_GROUPS.contains("MLKEM"));
    }

    #[test]
    fn embedded_installer_script_is_present() {
        let s = embedded_installer_script();
        assert!(!s.trim().is_empty());
        assert!(s.contains("install_pqc_tls_stack") || s.contains("openssl"));
    }

    // SEC-001: el staging del instalador usa un directorio temporal privado
    // (0700, nombre aleatorio) — nunca una ruta fija bajo /tmp que un usuario
    // local pudiera pre-plantar.
    #[cfg(unix)]
    #[test]
    fn stage_installer_uses_private_random_dir() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let (dir, script) = stage_installer().expect("stage_installer");

        // El script vive dentro del tempdir, no en /tmp raíz.
        assert!(script.starts_with(dir.path()));
        assert!(script.is_file());

        // Directorio 0700 y nombre no predecible.
        let dir_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "tempdir debe ser 0700");

        // El script nace 0700 (O_EXCL + mode en open — sin ventana chmod).
        let script_mode = std::fs::metadata(&script).unwrap().permissions().mode() & 0o777;
        assert_eq!(script_mode, 0o700, "script debe ser 0700");
        assert_eq!(
            std::fs::metadata(&script).unwrap().uid(),
            unsafe { libc::geteuid() },
            "el script debe pertenecer al euid actual"
        );

        // Dos stagings consecutivos producen rutas distintas.
        let (dir2, script2) = stage_installer().expect("second stage_installer");
        assert_ne!(dir.path(), dir2.path());
        assert_ne!(script, script2);

        // Un fichero pre-plantado en /tmp NO es tocado ni usado.
        let planted = std::path::Path::new("/tmp/enola_install_pqc_tls_stack.sh");
        if planted.exists() {
            // Si existiera (atacante lo creó), el staging no lo usa.
            assert!(!script.starts_with(planted));
        }
    }

    #[test]
    fn pqc_curve_directive_with_available_true_returns_directive() {
        let d = nginx_pqc_curve_directive_with(true);
        assert!(d.contains("ssl_ecdh_curve"));
        assert!(d.contains(PQC_TLS_GROUPS));
    }

    #[test]
    fn pqc_curve_directive_with_available_false_returns_empty() {
        assert_eq!(nginx_pqc_curve_directive_with(false), "");
    }

    #[test]
    fn pqc_curve_directive_public_wrapper_executes() {
        let d = nginx_pqc_curve_directive();
        assert!(d.is_empty() || d.contains("ssl_ecdh_curve"));
    }

    #[test]
    fn command_output_reads_stdout_and_stderr_and_handles_missing_binary() {
        let out = command_output("sh", &["-c", "printf 'ok'"], false);
        assert_eq!(out.as_deref(), Some("ok"));

        let err = command_output("sh", &["-c", "printf 'err' 1>&2"], true);
        assert_eq!(err.as_deref(), Some("err"));

        let missing = command_output("definitely-not-a-real-binary-enola", &[], false);
        assert!(missing.is_none());
    }

    #[test]
    fn required_prefix_and_availability_helpers_cover_branches() {
        assert!(has_required_openssl_prefix("3.5.1"));
        assert!(!has_required_openssl_prefix("3.4.9"));

        assert!(openssl_35_available_from(Some("3.5.6".to_string())));
        assert!(!openssl_35_available_from(Some("3.4.9".to_string())));
        assert!(!openssl_35_available_from(None));

        assert!(nginx_pqc_ready_from(Some("3.5.7".to_string())));
        assert!(!nginx_pqc_ready_from(Some("1.1.1".to_string())));
        assert!(!nginx_pqc_ready_from(None));
    }

    #[test]
    fn pqc_tls_available_with_truth_table() {
        assert!(pqc_tls_available_with(true, true, true));
        assert!(!pqc_tls_available_with(false, true, true));
        assert!(!pqc_tls_available_with(true, false, true));
        assert!(!pqc_tls_available_with(true, true, false));
    }

    #[test]
    fn doctor_section_with_includes_hint_only_when_not_ready() {
        let ok = doctor_section_with("3.5.6".to_string(), "3.5.6".to_string(), true, true, true);
        assert!(!ok.contains("Enable PQC TLS"));

        let not_ok = doctor_section_with(
            "not found".to_string(),
            "not linked".to_string(),
            false,
            false,
            false,
        );
        assert!(not_ok.contains("Enable PQC TLS"));
    }
}
