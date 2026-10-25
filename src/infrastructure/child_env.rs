//! R-3 / A25-02 — Entorno mínimo para subprocesos `enola-cli` despachados.
//!
//! Compartido por los dos brazos que ejecutan el propio binario con argv de
//! un tercero: el runner MCP (`adapters/process_runner.rs`) y la consola web
//! (`/api/console/run`). El hijo arranca con `env_clear()` y solo recibe esta
//! allowlist — una denylist de `ENOLA_*` sería porosa (cualquier var nueva
//! reabriría el agujero; ver auditoría F-15: `ENOLA_UPDATE_MINISIGN_PUBKEY`
//! reemplaza el trust anchor de firmas; A25-02: `ENOLA_WEB_TOKEN`).

use std::ffi::OsString;

/// Variables permitidas en hijos despachados.
///
/// `HOME`/`USER`/`SUDO_USER` se conservan: el hijo resuelve su config y
/// estado desde ellos (`effective_home()`); la confianza sobre `[mcp]` en
/// homes de usuario se trata en R-5, no aquí. `ENOLA_ACCEPT_LICENSE` se
/// conserva: es el gate legal, no de seguridad.
pub(crate) const CHILD_ENV_EXACT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SUDO_USER",
    "LANG",
    "TERM",
    "ENOLA_ACCEPT_LICENSE",
];

/// Prefijos permitidos (locale).
pub(crate) const CHILD_ENV_PREFIXES: &[&str] = &["LC_"];

/// Entorno filtrado para el hijo: solo la allowlist + `ENOLA_MCP_CHILD=1`
/// (marca que desactiva el preflight automático en el hijo — R-3/SEC-009).
///
/// Uso: `Command::new(exe).env_clear().envs(child_env())`.
pub(crate) fn child_env() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(k, _)| {
            k.to_str()
                .map(|k| {
                    CHILD_ENV_EXACT.contains(&k)
                        || CHILD_ENV_PREFIXES.iter().any(|p| k.starts_with(p))
                })
                .unwrap_or(false)
        })
        .collect();
    env.push((OsString::from("ENOLA_MCP_CHILD"), OsString::from("1")));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-3 / A25-02: el entorno del hijo es `env_clear()` + allowlist
    /// explícita — ningún `ENOLA_*` peligroso se hereda (tampoco el token del
    /// dashboard), `ENOLA_MCP_CHILD` se marca.
    #[test]
    fn child_env_strips_enola_secrets_and_marks_child() {
        // Serializa con el resto de tests que mutan `std::env` (§13.33).
        let _g = crate::infrastructure::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Simular un env envenenado para la comprobación.
        std::env::set_var("ENOLA_UPDATE_MINISIGN_PUBKEY", "evil");
        std::env::set_var("ENOLA_ALLOW_UNSIGNED_UPDATE", "1");
        std::env::set_var("ENOLA_WEB_TOKEN", "x");
        std::env::set_var("ENOLA_AI_ENDPOINT", "http://10.0.0.5:11434");
        let env = child_env();
        let keys: Vec<&str> = env.iter().filter_map(|(k, _)| k.to_str()).collect();

        assert!(keys.contains(&"ENOLA_MCP_CHILD"));
        for poisoned in [
            "ENOLA_UPDATE_MINISIGN_PUBKEY",
            "ENOLA_ALLOW_UNSIGNED_UPDATE",
            "ENOLA_WEB_TOKEN",
            "ENOLA_AI_ENDPOINT",
            "ENOLA_SKIP_ROOT_CHECK",
            "ENOLA_TOR_SOCKS_PROXY",
            "ENOLA_MANIFEST_PATH",
            "ENOLA_TEST_TOKEN",
        ] {
            assert!(
                !keys.contains(&poisoned),
                "{poisoned} no debe llegar al hijo despachado"
            );
        }
        // Solo la allowlist (+ la marca) sobrevive.
        for k in &keys {
            assert!(
                *k == "ENOLA_MCP_CHILD"
                    || CHILD_ENV_EXACT.contains(k)
                    || CHILD_ENV_PREFIXES.iter().any(|p| k.starts_with(p)),
                "{k} fuera de la allowlist"
            );
        }
        assert!(
            keys.contains(&"PATH"),
            "PATH debe preservarse para que el hijo funcione"
        );
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");
        std::env::remove_var("ENOLA_ALLOW_UNSIGNED_UPDATE");
        std::env::remove_var("ENOLA_WEB_TOKEN");
        std::env::remove_var("ENOLA_AI_ENDPOINT");
    }
}
