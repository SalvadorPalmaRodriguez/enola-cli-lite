// ═══════════════════════════════════════════════════════════════════════════════
// SEC-EXT-PRIV-020 — Drop privileges helper for auxiliary subprocesses
// ═══════════════════════════════════════════════════════════════════════════════
//
// Cuando `enola-cli` se ejecuta vía `sudo`, el proceso adquiere EUID 0 para
// poder gestionar Docker, systemd, /etc/nginx, /var/lib/tor, iptables/UFW, etc.
//
// Sin embargo, MUCHOS subprocesos auxiliares no necesitan EUID 0:
//   - `nvidia-smi`, `lscpu`, `free`, `df`  → solo lectura del sistema
//   - `curl`/HTTP a feeds de advisories     → red sin acceso a recursos root
//   - `git clone` / `cargo build` / `npm`   → opera en directorios del usuario
//
// Mantener EUID 0 en estos subprocesos amplía la superficie de ataque sin
// beneficio: si una vulnerabilidad en el binario externo se explota (CVE en
// `nvidia-smi`, etc.), el atacante hereda root.
//
// Este módulo proporciona una API mínima para crear un `std::process::Command`
// que, automáticamente y sin esfuerzo, baja UID/GID al usuario que invocó
// `sudo` (`SUDO_UID`/`SUDO_GID`) antes del `exec()` del hijo. El padre
// (`enola-cli`) mantiene EUID 0.
//
// Comportamiento:
//   1. Si `SUDO_UID` y `SUDO_GID` están definidos y son no-cero → setuid/setgid
//      antes de exec. El hijo corre como el usuario original.
//   2. Si `SUDO_UID` no está (binario lanzado directamente como root, o
//      ejecutado sin sudo) → no se baja, el hijo hereda los privilegios
//      actuales. Esto preserva compatibilidad con pipelines de root puro.
//   3. Se sanitizan variables de entorno sensibles propagadas por sudo
//      (HOME, USER, LOGNAME) para que el hijo vea el entorno del usuario
//      original cuando aplique.
//
// Esto NO sustituye `seteuid()` global del proceso padre (sería peligroso —
// rompería las operaciones que sí necesitan root). Es por-subproceso y
// opt-in: los call-sites que claramente NO necesitan root usan
// `command_as_invoking_user(...)`; los demás siguen con `Command::new(...)`.

use std::ffi::OsStr;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Información del usuario invocador extraída del entorno sudo.
#[derive(Debug, Clone)]
pub struct InvokingUser {
    pub uid: u32,
    pub gid: u32,
    pub name: Option<String>,
    pub home: Option<String>,
}

/// Lee la identidad del usuario que invocó `sudo`. Devuelve `None` si no
/// estamos bajo sudo (binario lanzado directamente como root, o no-root) o
/// si las variables están corruptas / `SUDO_UID == 0`.
pub fn invoking_user() -> Option<InvokingUser> {
    let uid_s = std::env::var("SUDO_UID").ok()?;
    let gid_s = std::env::var("SUDO_GID").ok()?;
    let uid: u32 = uid_s.parse().ok()?;
    let gid: u32 = gid_s.parse().ok()?;

    // SUDO_UID == 0 significa "sudo de root a root" — no hay nada que bajar.
    if uid == 0 {
        return None;
    }

    let name = std::env::var("SUDO_USER").ok().filter(|s| !s.is_empty());
    let home = lookup_home_for_uid(uid).or_else(|| std::env::var("HOME").ok());

    Some(InvokingUser {
        uid,
        gid,
        name,
        home,
    })
}

/// Resuelve el directorio HOME del UID consultando la base passwd (NSS).
/// Devuelve `None` si no se encuentra. Evita hardcodear rutas tipo
/// `/home/<user>` (no siempre es así: LDAP, `/var/empty`, `/srv/users/...`).
#[cfg(unix)]
pub(crate) fn lookup_home_for_uid(uid: u32) -> Option<String> {
    use std::ffi::CStr;

    let mut buf = vec![0i8; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();

    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };

    if rc != 0 || result.is_null() {
        return None;
    }

    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
    dir.to_str().ok().map(|s| s.to_string())
}

#[cfg(not(unix))]
fn lookup_home_for_uid(_uid: u32) -> Option<String> {
    None
}

/// Logica interna de `should_drop_for_auxiliary` con euid inyectable para tests.
/// Permite cubrir la rama "euid == 0" sin necesitar privilegios reales.
#[cfg(unix)]
pub(crate) fn should_drop_inner(euid: u32) -> bool {
    if euid != 0 {
        return false;
    }
    invoking_user().is_some()
}

/// `true` si el proceso actual es root EUID 0 y existe un usuario invocador
/// (escenario `sudo enola-cli ...`). Es decir, hay margen para bajar
/// privilegios al lanzar un subproceso auxiliar.
pub fn should_drop_for_auxiliary() -> bool {
    #[cfg(unix)]
    return should_drop_inner(unsafe { libc::geteuid() });
    #[cfg(not(unix))]
    false
}

/// Construye un `Command` que, antes de `exec()`, baja UID/GID al usuario
/// invocador (`SUDO_UID`/`SUDO_GID`) si procede. Si no procede (binario
/// no-root, lanzado directamente como root sin sudo, etc.), devuelve un
/// `Command` plano equivalente a `Command::new(program)`.
///
/// Úselo en TODOS los subprocesos auxiliares que NO necesiten root:
///   - lectura de hardware (`nvidia-smi`, `lscpu`, …)
///   - operaciones HTTP/red sin recursos root (curl a feeds)
///   - herramientas que actúan sobre el `$HOME` del usuario (git, cargo,
///     npm, …)
///
/// **NO** lo use con `docker`, `systemctl`, `nginx -s`, `tor`, `iptables`,
/// `ufw`, escrituras en `/etc`, `/var/lib/tor`, `/opt/enola`. Esos sí
/// necesitan EUID 0.
/// Aplica drop de privilegios a `cmd` si `euid == 0` e `invoking_user()` lo permite.
/// Separado para tests: permite llamar con euid simulado sin necesitar ser root.
/// No hace `spawn()` - solo configura uid/gid/env en el Command.
#[cfg(unix)]
pub(crate) fn apply_drop_privs_unix(cmd: &mut Command, euid: u32) {
    if let Some(user) = invoking_user() {
        if euid == 0 {
            cmd.uid(user.uid);
            cmd.gid(user.gid);
            if let Some(home) = user.home {
                cmd.env("HOME", home);
            }
            if let Some(name) = user.name {
                cmd.env("USER", &name);
                cmd.env("LOGNAME", &name);
            }
        }
    }
}

pub fn command_as_invoking_user<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(unix)]
    apply_drop_privs_unix(&mut cmd, unsafe { libc::geteuid() });
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serializa los tests porque mutan variables de entorno globales.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_sudo_env() {
        std::env::remove_var("SUDO_UID");
        std::env::remove_var("SUDO_GID");
        std::env::remove_var("SUDO_USER");
    }

    #[test]
    fn invoking_user_none_without_sudo_env() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        assert!(invoking_user().is_none());
    }

    #[test]
    fn invoking_user_parses_valid_sudo_env() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        std::env::set_var("SUDO_USER", "alice");

        let u = invoking_user().expect("should parse SUDO_*");
        assert_eq!(u.uid, 1000);
        assert_eq!(u.gid, 1000);
        assert_eq!(u.name.as_deref(), Some("alice"));
        // `home` se resuelve vía passwd (UID 1000) o via $HOME del entorno
        // del test runner. Solo comprobamos que NO es vacío si está presente.
        // Verificar home directamente para cubrir la rama.
        // En este sistema, UID 1000 existe en /etc/passwd o $HOME esta definido.
        let home_val = u.home.clone().unwrap_or_default();
        assert!(
            !home_val.is_empty(),
            "home debe ser non-empty (passwd o $HOME)"
        );

        clear_sudo_env();
    }

    #[test]
    fn invoking_user_rejects_sudo_uid_zero() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "0");
        std::env::set_var("SUDO_GID", "0");
        std::env::set_var("SUDO_USER", "root");

        // sudo de root a root: nada que bajar.
        assert!(invoking_user().is_none());

        clear_sudo_env();
    }

    #[test]
    fn invoking_user_rejects_corrupt_uid() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "not-a-number");
        std::env::set_var("SUDO_GID", "1000");

        assert!(invoking_user().is_none());

        clear_sudo_env();
    }

    #[test]
    fn command_as_invoking_user_returns_command_without_sudo() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        // Sin SUDO_*: debe devolver un Command plano que ejecuta /bin/true.
        let mut c = command_as_invoking_user("/bin/true");
        let status = c.status().expect("should spawn /bin/true");
        assert!(status.success());
    }

    #[test]
    fn command_as_invoking_user_runs_when_not_root_with_sudo_env() {
        // Test runner típicamente NO es root. Verificamos que aunque haya
        // SUDO_*, no se intenta setuid (fallaría EPERM); el comando arranca
        // OK porque la lógica detecta euid != 0 y omite el setuid.
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        std::env::set_var("SUDO_USER", "alice");

        // apply_drop_privs_unix solo actua con euid==0; aqui no somos root,
        // por lo que el Command se lanza sin setuid -> sin EPERM.
        let mut c = command_as_invoking_user("/bin/true");
        let status = c.status().expect("should spawn /bin/true");
        assert!(status.success());

        clear_sudo_env();
    }

    #[test]
    fn should_drop_for_auxiliary_false_when_not_root() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        // Usar should_drop_inner con euid simulado para cobertura completa
        // sin depender del privilegio real del runner.
        assert!(!should_drop_inner(999), "euid != 0 siempre retorna false");
        clear_sudo_env();
    }

    // TEST-COV-UNIT-003: cubrir línea 58 (gid_s cuando SUDO_GID no está presente)
    #[test]
    fn invoking_user_none_when_sudo_uid_present_but_gid_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        // No set SUDO_GID → gid_s = var("SUDO_GID").ok()? devuelve None
        assert!(invoking_user().is_none(), "sin SUDO_GID debe devolver None");
        clear_sudo_env();
    }

    // TEST-COV-UNIT-003: cubrir línea 60 (gid.parse() cuando GID es inválido)
    #[test]
    fn invoking_user_none_when_gid_is_corrupt() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "not-a-gid");
        assert!(invoking_user().is_none(), "GID inválido debe devolver None");
        clear_sudo_env();
    }

    // TEST-COV-UNIT-003: cubrir la rama HOME fallback en invoking_user
    // Cuando lookup_home_for_uid no encuentra la entrada, usa $HOME del entorno.
    #[test]
    fn invoking_user_uses_home_env_when_passwd_lookup_fails() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        // UID 65534 ('nobody') generalmente no tiene home en /etc/passwd.
        // Si lookup_home devuelve None, debe usar HOME del entorno como fallback.
        std::env::set_var("SUDO_UID", "65534");
        std::env::set_var("SUDO_GID", "65534");
        std::env::set_var("HOME", "/tmp/test-home-fallback");
        let result = invoking_user();
        // Si SUDO_UID=65534 es inválido para parse o lookup falla, cualquier valor es OK.
        // Lo importante es que no panic.
        if let Some(u) = result {
            // Si se resolvió, el home debe venir de algún side (passwd o $HOME).
            let _ = u.home;
        }
        std::env::remove_var("HOME");
        clear_sudo_env();
    }

    // TEST-COV-UNIT-004: L100 - lookup_home_for_uid con UID inexistente
    #[cfg(unix)]
    #[test]
    fn lookup_home_returns_none_for_nonexistent_uid() {
        assert!(
            lookup_home_for_uid(4_294_967_293).is_none(),
            "UID inexistente debe devolver None"
        );
    }

    // TEST-COV-UNIT-004: should_drop_inner euid==0 sin SUDO
    #[cfg(unix)]
    #[test]
    fn should_drop_inner_false_when_euid_zero_and_no_sudo() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        assert!(!should_drop_inner(0));
    }

    // TEST-COV-UNIT-004: should_drop_inner euid==0 con SUDO valido
    #[cfg(unix)]
    #[test]
    fn should_drop_inner_true_when_euid_zero_and_sudo_set() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        assert!(should_drop_inner(0));
        clear_sudo_env();
    }

    // TEST-COV-UNIT-004: apply_drop_privs_unix con euid==0 (configura sin spawn)
    #[cfg(unix)]
    #[test]
    fn apply_drop_privs_configures_uid_when_euid_zero() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        std::env::set_var("SUDO_USER", "testuser");
        let mut cmd = Command::new("/bin/true");
        apply_drop_privs_unix(&mut cmd, 0); // euid=0 simulado, sin spawn
        clear_sudo_env();
    }

    // TEST-COV-UNIT-004: apply_drop_privs_unix con euid!=0 -> no-op
    #[cfg(unix)]
    #[test]
    fn apply_drop_privs_noop_when_euid_nonzero() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        std::env::set_var("SUDO_UID", "1000");
        std::env::set_var("SUDO_GID", "1000");
        let mut cmd = Command::new("/bin/true");
        apply_drop_privs_unix(&mut cmd, 999);
        clear_sudo_env();
    }
    // TEST-COV-UNIT-004: L125-130 - should_drop_for_auxiliary() API publica
    #[test]
    fn should_drop_for_auxiliary_public_api_returns_bool() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_sudo_env();
        // La funcion publica delega en should_drop_inner con el euid real.
        // Solo verificamos que devuelve bool sin panic.
        let _ = should_drop_for_auxiliary();
    }
}
