// SEC-A9 (D12/D16): entorno heredado no confiable cuando el CLI corre como root.
//
// **Causa raíz que cierra**: con `sudo -E` el proceso root hereda el entorno
// del usuario, y cada variable sensible se trataba ad hoc (solo los trust
// anchors de update exigían doble flag, C-08). Variables como
// `ENOLA_MINISIGN_BIN` (elige el verificador de las firmas de update y se
// ejecuta como root), `ENOLA_MANIFEST_PATH` (dónde escribe root el manifest),
// `ENOLA_TOR_SOCKS_PROXY` (a dónde se envía el tráfico .onion) o `PATH`
// (qué binarios ejecuta root) quedaban bajo control del usuario.
//
// **Diseño**: `sanitize_inherited_env()` se llama al principio de `main`, ANTES
// de exportar los flags globales (`--tor-socks`, …): lo que teclea el operador
// en la línea de comandos se respeta; lo heredado del entorno se valida.
use std::path::{Path, PathBuf};

/// `PATH` fijo para procesos root (D16).
pub const SECURE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Acción a aplicar sobre una variable del entorno heredado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvAction {
    /// Fijar `name` = `value`.
    Set(&'static str, String),
    /// Eliminar `name`, con el motivo para el aviso.
    Remove(&'static str, String),
}

/// Decide qué hacer con el entorno heredado. Pura: `get` lee una variable.
/// Sin root no cambia nada.
pub fn sanitize_decisions(euid: u32, get: &dyn Fn(&str) -> Option<String>) -> Vec<EnvAction> {
    let mut actions = Vec::new();
    if euid != 0 {
        return actions;
    }
    // D16: root nunca resuelve binarios con el PATH del usuario.
    actions.push(EnvAction::Set("PATH", SECURE_PATH.to_string()));
    // Documentado solo para tests: root no escribe el manifest donde diga el entorno.
    if get("ENOLA_MANIFEST_PATH").is_some() {
        actions.push(EnvAction::Remove(
            "ENOLA_MANIFEST_PATH",
            "test-only override".to_string(),
        ));
    }
    // Verificador de firmas de update: nombre simple (se resuelve con
    // SECURE_PATH) o ruta absoluta con toda la cadena root-owned.
    if let Some(raw) = get("ENOLA_MINISIGN_BIN").filter(|v| v.contains('/')) {
        match trusted_root_binary(&raw) {
            Some(canon) => actions.push(EnvAction::Set(
                "ENOLA_MINISIGN_BIN",
                canon.to_string_lossy().to_string(),
            )),
            None => actions.push(EnvAction::Remove(
                "ENOLA_MINISIGN_BIN",
                format!(
                    "'{raw}' (and every parent directory) must be root-owned and \
                     not group/other-writable"
                ),
            )),
        }
    }
    if let Some(v) = get("ENOLA_TOR_SOCKS_PROXY").filter(|v| !v.trim().is_empty()) {
        if !socks_proxy_is_loopback(&v) {
            actions.push(EnvAction::Remove(
                "ENOLA_TOR_SOCKS_PROXY",
                format!("'{v}' is not a socks5h:// proxy on 127.0.0.1/::1/localhost"),
            ));
        }
    }
    actions
}

/// Aplica [`sanitize_decisions`] al entorno del proceso, avisando por stderr
/// de cada variable descartada.
pub fn sanitize_inherited_env() {
    // SAFETY: geteuid no tiene precondiciones.
    let euid = unsafe { libc::geteuid() };
    for action in sanitize_decisions(euid, &|k| std::env::var(k).ok()) {
        match action {
            EnvAction::Set(k, v) => std::env::set_var(k, v),
            EnvAction::Remove(k, why) => {
                eprintln!("⚠️  {k} ignored when running as root: {why}");
                std::env::remove_var(k);
            }
        }
    }
}

/// `true` si `value` es `socks5h://` hacia loopback (127.0.0.1, ::1, localhost).
pub fn socks_proxy_is_loopback(value: &str) -> bool {
    let v = value.trim();
    let Some(rest) = v
        .get(..10)
        .filter(|p| p.eq_ignore_ascii_case("socks5h://"))
        .and_then(|_| v.get(10..))
    else {
        return false;
    };
    let authority = rest.trim_end_matches('/');
    if authority.contains(['@', '/', '?', '#']) {
        return false;
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(r) => match r.split_once("]:") {
            Some((h, p)) => (h, p),
            None => return false,
        },
        None => match authority.rsplit_once(':') {
            Some((h, p)) => (h, p),
            None => return false,
        },
    };
    let port_ok = !port.is_empty() && port.parse::<u16>().is_ok();
    port_ok
        && matches!(
            host.to_ascii_lowercase().as_str(),
            "127.0.0.1" | "localhost" | "::1"
        )
}

/// `true` si `path` (ya canónico) y TODOS sus directorios padre son de root,
/// no son enlaces simbólicos y no tienen escritura de grupo/otros.
pub fn root_owned_chain(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() {
        return false;
    }
    let mut cur = Some(path);
    let mut leaf = true;
    while let Some(p) = cur {
        let Ok(md) = std::fs::symlink_metadata(p) else {
            return false;
        };
        let kind_ok = if leaf { md.is_file() } else { md.is_dir() };
        if !kind_ok || md.uid() != 0 || md.mode() & 0o022 != 0 {
            return false;
        }
        leaf = false;
        cur = p.parent();
    }
    true
}

/// Ruta canónica de `raw` si toda su cadena es root-owned (ver arriba).
pub fn trusted_root_binary(raw: &str) -> Option<PathBuf> {
    // Se valida y se usa la ruta CANÓNICA: un symlink intermedio en un
    // directorio ajeno no puede cambiar el destino después de comprobarlo.
    let canon = std::fs::canonicalize(raw).ok()?;
    root_owned_chain(&canon).then_some(canon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn non_root_environment_is_untouched() {
        let get = env(&[
            ("PATH", "/home/u/bin:/usr/bin"),
            ("ENOLA_MANIFEST_PATH", "/tmp/m"),
            ("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.5:9050"),
        ]);
        assert!(sanitize_decisions(1000, &get).is_empty());
    }

    #[test]
    fn root_gets_secure_path() {
        let get = env(&[("PATH", "/home/u/bin:/usr/bin")]);
        let actions = sanitize_decisions(0, &get);
        assert!(actions.contains(&EnvAction::Set("PATH", SECURE_PATH.to_string())));
    }

    #[test]
    fn root_drops_manifest_path_override() {
        let get = env(&[("ENOLA_MANIFEST_PATH", "/etc/passwd")]);
        let actions = sanitize_decisions(0, &get);
        assert!(actions
            .iter()
            .any(|a| matches!(a, EnvAction::Remove("ENOLA_MANIFEST_PATH", _))));
    }

    #[test]
    fn root_drops_non_loopback_socks_proxy_but_keeps_loopback() {
        let remote = env(&[("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.5:9050")]);
        assert!(sanitize_decisions(0, &remote)
            .iter()
            .any(|a| matches!(a, EnvAction::Remove("ENOLA_TOR_SOCKS_PROXY", _))));
        let local = env(&[("ENOLA_TOR_SOCKS_PROXY", "socks5h://127.0.0.1:9150")]);
        assert!(!sanitize_decisions(0, &local)
            .iter()
            .any(|a| matches!(a, EnvAction::Remove("ENOLA_TOR_SOCKS_PROXY", _))));
    }

    #[test]
    fn root_drops_user_owned_minisign_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("minisign");
        std::fs::write(&fake, "#!/bin/sh\necho minisign\n").unwrap();
        let get = env(&[("ENOLA_MINISIGN_BIN", fake.to_str().unwrap())]);
        let actions = sanitize_decisions(0, &get);
        if unsafe { libc::geteuid() } != 0 {
            assert!(actions
                .iter()
                .any(|a| matches!(a, EnvAction::Remove("ENOLA_MINISIGN_BIN", _))));
        }
    }

    #[test]
    fn root_keeps_root_owned_minisign_bin_as_canonical_path() {
        // /bin/sh es root-owned en toda distro; sirve como binario de prueba.
        let get = env(&[("ENOLA_MINISIGN_BIN", "/bin/sh")]);
        let actions = sanitize_decisions(0, &get);
        let canon = std::fs::canonicalize("/bin/sh").unwrap();
        assert!(actions.contains(&EnvAction::Set(
            "ENOLA_MINISIGN_BIN",
            canon.to_string_lossy().to_string()
        )));
    }

    #[test]
    fn root_keeps_simple_minisign_name() {
        let get = env(&[("ENOLA_MINISIGN_BIN", "minisign")]);
        assert!(!sanitize_decisions(0, &get)
            .iter()
            .any(|a| matches!(a, EnvAction::Remove("ENOLA_MINISIGN_BIN", _))));
    }

    #[test]
    fn socks_proxy_loopback_parsing() {
        assert!(socks_proxy_is_loopback("socks5h://127.0.0.1:9050"));
        assert!(socks_proxy_is_loopback("socks5h://localhost:9150"));
        assert!(socks_proxy_is_loopback("socks5h://[::1]:9050"));
        assert!(socks_proxy_is_loopback("SOCKS5H://127.0.0.1:9050"));
        assert!(!socks_proxy_is_loopback("socks5://127.0.0.1:9050"));
        assert!(!socks_proxy_is_loopback("socks5h://10.0.0.5:9050"));
        assert!(!socks_proxy_is_loopback(
            "socks5h://127.0.0.1.evil.com:9050"
        ));
        assert!(!socks_proxy_is_loopback("socks5h://user@10.0.0.5:9050"));
        assert!(!socks_proxy_is_loopback("socks5h://127.0.0.1:notaport"));
    }

    #[test]
    fn root_owned_chain_accepts_system_binary_and_rejects_user_file() {
        let sh = std::fs::canonicalize("/bin/sh").unwrap();
        assert!(root_owned_chain(&sh));
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("x");
        std::fs::write(&f, "x").unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert!(!root_owned_chain(&f));
        }
    }
}
