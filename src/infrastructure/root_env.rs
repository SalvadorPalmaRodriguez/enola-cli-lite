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
// **Diseño (A25-04, P2.1)**: como root el entorno heredado es ALLOWLIST, no
// denylist (la denylist de 4 variables no escalaba frente a ~60 `ENOLA_*`).
// Bajo euid 0 se conservan SOLO `PATH := SECURE_PATH` y `ROOT_ENV_ALLOW`; toda
// otra `ENOLA_*` heredada se elimina con UN aviso agregado. Se llama al
// principio de `main`, ANTES de exportar los flags globales (`--tor-socks`, …):
// lo que teclea el operador en la línea de comandos se respeta.
//
// Excepciones futuras: solo con justificación escrita en `security/SKILL.md`
// (SD-4). No hay ampliación en builds `testing`: los E2E usan los mismos
// mecanismos que un usuario real (flags, `/etc/enola/config.toml`).
use std::path::{Path, PathBuf};

/// `PATH` fijo para procesos root (D16).
pub const SECURE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Únicas `ENOLA_*` heredadas que sobreviven bajo euid 0 (decisión del
/// operador 2026-10-25): gate legal de licencia, marca de hijo MCP/consola
/// (desactiva el preflight) y consentimiento explícito de auto-setup.
pub const ROOT_ENV_ALLOW: &[&str] = &[
    "ENOLA_ACCEPT_LICENSE",
    "ENOLA_MCP_CHILD",
    "ENOLA_AUTO_SETUP",
];

/// Acción a aplicar sobre una variable del entorno heredado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvAction {
    /// Fijar `name` = `value`.
    Set(&'static str, String),
    /// Eliminar `name` (variable `ENOLA_*` fuera de la allowlist).
    Remove(String),
}

/// Decide qué hacer con el entorno heredado. Pura: `names` son los nombres de
/// las variables presentes. Sin root no cambia nada.
pub fn sanitize_decisions(euid: u32, names: &[String]) -> Vec<EnvAction> {
    let mut actions = Vec::new();
    if euid != 0 {
        return actions;
    }
    // D16: root nunca resuelve binarios con el PATH del usuario.
    actions.push(EnvAction::Set("PATH", SECURE_PATH.to_string()));
    let mut dropped: Vec<&String> = names
        .iter()
        .filter(|k| k.starts_with("ENOLA_") && !ROOT_ENV_ALLOW.contains(&k.as_str()))
        .collect();
    dropped.sort();
    dropped.dedup();
    actions.extend(dropped.into_iter().map(|k| EnvAction::Remove(k.clone())));
    actions
}

/// Aplica [`sanitize_decisions`] al entorno del proceso con un único aviso
/// agregado por stderr (nunca los valores: pueden ser secretos).
pub fn sanitize_inherited_env() {
    // SAFETY: geteuid no tiene precondiciones.
    let euid = unsafe { libc::geteuid() };
    let names: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .collect();
    let mut removed: Vec<String> = Vec::new();
    for action in sanitize_decisions(euid, &names) {
        match action {
            EnvAction::Set(k, v) => std::env::set_var(k, v),
            EnvAction::Remove(k) => {
                std::env::remove_var(&k);
                removed.push(k);
            }
        }
    }
    if let Some(msg) = ignored_warning(&removed) {
        eprintln!("{msg}");
    }
}

/// Texto del aviso agregado (`None` si no se ignoró nada).
fn ignored_warning(removed: &[String]) -> Option<String> {
    if removed.is_empty() {
        return None;
    }
    Some(format!(
        "⚠️  {} ENOLA_* variable(s) ignored when running as root: {}. \
         Use command-line flags or /etc/enola/config.toml (root-owned).",
        removed.len(),
        removed.join(", ")
    ))
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

    fn names(vars: &[&str]) -> Vec<String> {
        vars.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn non_root_environment_is_untouched() {
        let n = names(&[
            "PATH",
            "ENOLA_MANIFEST_PATH",
            "ENOLA_TOR_SOCKS_PROXY",
            "ENOLA_WEB_TOKEN",
        ]);
        assert!(sanitize_decisions(1000, &n).is_empty());
    }

    #[test]
    fn root_gets_secure_path() {
        let actions = sanitize_decisions(0, &names(&["PATH"]));
        assert!(actions.contains(&EnvAction::Set("PATH", SECURE_PATH.to_string())));
    }

    /// A25-04 (P2): con ~60 `ENOLA_*` heredadas solo sobreviven la allowlist
    /// y PATH; variables ajenas a `ENOLA_*` no se tocan.
    #[test]
    fn root_keeps_only_allowlist() {
        let mut n = names(&[
            "PATH",
            "HOME",
            "LANG",
            "ENOLA_ACCEPT_LICENSE",
            "ENOLA_MCP_CHILD",
            "ENOLA_AUTO_SETUP",
            "ENOLA_WEB_TOKEN",
            "ENOLA_ALLOW_UNSIGNED_UPDATE",
            "ENOLA_MCP_AUDIT_LOG",
            "ENOLA_AI_ENDPOINT",
            "ENOLA_APPARMOR_ENABLED_PATH",
            "ENOLA_SKIP_ROOT_CHECK",
            "ENOLA_MINISIGN_BIN",
            "ENOLA_TOR_SOCKS_PROXY",
            "ENOLA_UPDATE_FEED_URL",
            "ENOLA_UPDATE_ENV_ANCHORS",
            "ENOLA_TEST_TOKEN",
        ]);
        n.extend((0..45).map(|i| format!("ENOLA_FUTURE_VAR_{i}")));
        let actions = sanitize_decisions(0, &n);
        let removed: Vec<&str> = actions
            .iter()
            .filter_map(|a| match a {
                EnvAction::Remove(k) => Some(k.as_str()),
                _ => None,
            })
            .collect();
        let kept: Vec<&String> = n
            .iter()
            .filter(|k| !removed.contains(&k.as_str()))
            .collect();
        for k in &kept {
            assert!(
                !k.starts_with("ENOLA_") || ROOT_ENV_ALLOW.contains(&k.as_str()),
                "{k} no debe sobrevivir como root"
            );
        }
        for allowed in ROOT_ENV_ALLOW {
            assert!(!removed.contains(allowed), "{allowed} debe conservarse");
        }
        for other in ["PATH", "HOME", "LANG"] {
            assert!(!removed.contains(&other));
        }
        assert_eq!(removed.len(), n.len() - 6);
    }

    #[test]
    fn warning_is_single_and_lists_names_only() {
        assert!(ignored_warning(&[]).is_none());
        let msg = ignored_warning(&names(&["ENOLA_WEB_TOKEN", "ENOLA_AI_ENDPOINT"])).unwrap();
        assert!(msg.contains("2 ENOLA_* variable(s)"));
        assert!(msg.contains("ENOLA_WEB_TOKEN, ENOLA_AI_ENDPOINT"));
        assert!(msg.contains("/etc/enola/config.toml"));
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
