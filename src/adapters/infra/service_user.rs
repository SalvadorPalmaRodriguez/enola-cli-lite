// SEC-M10 (D6) — Adapter de usuarios de sistema de servicio (`ServiceUserPort`).
//
// `useradd --system` crea un uid del rango de sistema (< 1000), sin home, sin
// shell de login y con grupo propio. Idempotente: si el usuario ya existe se
// devuelven sus ids sin tocarlo.

use crate::domain::error::{EnolaError, Result};
use crate::infrastructure::drop_privs::resolve_user_account;
use crate::ports::service_user::ServiceUserPort;
use std::path::Path;
use std::process::Command;

/// Usuarios de sistema vía `useradd`/`chown`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemServiceUser;

impl SystemServiceUser {
    /// Construye el adapter (sin estado).
    pub fn new() -> Self {
        Self
    }
}

/// argv de `useradd` para un usuario de servicio.
fn useradd_args(name: &str) -> Vec<String> {
    [
        "--system",
        "--user-group",
        "--no-create-home",
        "--home-dir",
        "/nonexistent",
        "--shell",
        "/usr/sbin/nologin",
        "--comment",
        "Enola service user",
        name,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Solo nombres `enola-<minúsculas/dígitos/->`: nunca se crea ni se reutiliza
/// una cuenta ajena (`root`, `www-data`, un usuario humano).
fn validate_service_user_name(name: &str) -> Result<()> {
    let ok = name.len() <= 32
        && name.strip_prefix("enola-").is_some_and(|r| {
            !r.is_empty()
                && r.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(EnolaError::ValidationError(format!(
            "Nombre de usuario de servicio no válido: '{name}'"
        )))
    }
}

impl ServiceUserPort for SystemServiceUser {
    #[tracing::instrument(skip(self))]
    fn ensure(&self, name: &str) -> Result<(u32, u32)> {
        validate_service_user_name(name)?;
        if let Some(acc) = resolve_user_account(Some(name)) {
            return Ok((acc.uid, acc.gid));
        }
        let out = Command::new("useradd")
            .args(useradd_args(name))
            .output()
            .map_err(|e| EnolaError::InfrastructureError(format!("useradd {name}: {e}")))?;
        // 9 = ya existe (carrera con otro create): se resuelve abajo.
        if !out.status.success() && out.status.code() != Some(9) {
            return Err(EnolaError::InfrastructureError(format!(
                "useradd {name}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        resolve_user_account(Some(name))
            .map(|acc| (acc.uid, acc.gid))
            .ok_or_else(|| {
                EnolaError::InfrastructureError(format!(
                    "usuario '{name}' no encontrado tras useradd"
                ))
            })
    }

    #[tracing::instrument(skip(self))]
    fn chown_tree(&self, path: &Path, uid: u32, gid: u32) -> Result<()> {
        // `-R` sin `-L`/`-H` no sigue symlinks; `-h` cambia el propio enlace.
        let out = Command::new("chown")
            .args(["-R", "-h", &format!("{uid}:{gid}")])
            .arg(path)
            .output()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("chown {}: {e}", path.display()))
            })?;
        if !out.status.success() {
            return Err(EnolaError::InfrastructureError(format!(
                "chown {}: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn useradd_creates_system_account_without_login() {
        let a = useradd_args("enola-git").join(" ");
        assert!(a.contains("--system"));
        assert!(a.contains("--no-create-home"));
        assert!(a.contains("--shell /usr/sbin/nologin"));
        assert!(a.ends_with("enola-git"));
    }

    #[test]
    fn only_enola_prefixed_names_are_managed() {
        for ok in ["enola-git", "enola-ghost", "enola-x1"] {
            assert!(validate_service_user_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "root",
            "www-data",
            "sma",
            "enola-",
            "enola-Git",
            "enola-a b",
            "-enola-git",
        ] {
            assert!(validate_service_user_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ensure_rejects_foreign_account_before_any_command() {
        // `root` existe siempre: sin la validación devolvería (0, 0).
        assert!(SystemServiceUser::new().ensure("root").is_err());
    }
}
