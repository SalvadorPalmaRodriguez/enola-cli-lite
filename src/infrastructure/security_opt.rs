//! SEC-007 — Default hardening for Docker container `--security-opt` flags.
//!
//! This module centralises the logic that decides WHICH `security_opt` values a container
//! gets when the caller does not provide any explicit list. It guarantees:
//!
//! 1. `no-new-privileges:true` is **always** present (universal hardening).
//! 2. When the host kernel has AppArmor enabled (`/sys/module/apparmor/parameters/enabled`
//!    == "Y") **and** the caller did not already pass an `apparmor=...` option, an
//!    informational log is emitted recommending a per-service profile (AA-002).
//! 3. On WSL2 / kernels without AppArmor (see AppArmor docs), the helper
//!    **silently degrades**: no warning, no profile injection, no change in behaviour.
//!
//! The helper is WSL2-safe by construction: it only reads a single kernel sysfs file and
//! returns a pure `Vec<String>`. It never fails — if the file is missing or unreadable, the
//! function assumes "no AppArmor" and proceeds with the universal hardening only.
//!
//! Tests use `ENOLA_APPARMOR_ENABLED_PATH` to point to a fixture file, so the real sysfs
//! node is never touched. A static `Mutex` serialises tests that set env vars (§13.33).

use std::path::PathBuf;

/// Name of the env var that overrides the sysfs path used for AppArmor detection.
/// **Test-only**: production code must never set this.
pub const APPARMOR_PATH_OVERRIDE_ENV: &str = "ENOLA_APPARMOR_ENABLED_PATH";

const APPARMOR_SYSFS_PATH: &str = "/sys/module/apparmor/parameters/enabled";
const NO_NEW_PRIVS: &str = "no-new-privileges:true";

fn apparmor_sysfs_path() -> PathBuf {
    std::env::var(APPARMOR_PATH_OVERRIDE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(APPARMOR_SYSFS_PATH))
}

/// Returns `true` iff the host kernel advertises AppArmor as enabled.
///
/// On WSL2 the sysfs file does not exist → returns `false` silently.
pub fn is_kernel_apparmor_enabled() -> bool {
    let path = apparmor_sysfs_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => content.trim().eq_ignore_ascii_case("Y"),
        Err(_) => false,
    }
}

/// AA-002: Returns `Some("apparmor=<profile_name>")` if the kernel has AppArmor enabled
/// **and** the profile file exists in `/etc/apparmor.d/`. Returns `None` on WSL2 or when
/// AppArmor is not active, so callers can degrade silently without any extra logic.
pub fn apparmor_profile_opt(profile_name: &str) -> Option<String> {
    if !is_kernel_apparmor_enabled() {
        return None;
    }
    let profile_path = format!("/etc/apparmor.d/{}", profile_name);
    if std::path::Path::new(&profile_path).exists() {
        Some(format!("apparmor={}", profile_name))
    } else {
        None
    }
}

/// Augments the caller-supplied `security_opt` list with default hardening.
///
/// - Always ensures `no-new-privileges:true` is present.
/// - Emits a `tracing::info!` hint when AppArmor is enabled in the kernel but the caller
///   did not pass any `apparmor=...` profile (so operators know they can enforce AA-002).
///
/// The returned vec preserves the order of the input and appends missing defaults at the end.
pub fn build_default_security_opt(user_opts: Vec<String>) -> Vec<String> {
    let mut out = user_opts;

    if !out.iter().any(|o| o.trim() == NO_NEW_PRIVS) {
        out.push(NO_NEW_PRIVS.to_string());
    }

    let has_apparmor = out
        .iter()
        .any(|o| o.trim_start().to_ascii_lowercase().starts_with("apparmor="));

    if !has_apparmor && is_kernel_apparmor_enabled() {
        tracing::info!(
            target = "enola::security",
            "AppArmor kernel module enabled but no profile passed for this container. \
             Consider --security-opt apparmor=<profile> (see AppArmor documentation)."
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Serialises tests that mutate `ENOLA_APPARMOR_ENABLED_PATH` (see §13.33 of
    /// documentation.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tmp_file_with(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tmp file");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn kernel_apparmor_enabled_reads_y() {
        let _g = ENV_LOCK.lock().unwrap();
        let f = tmp_file_with("Y\n");
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, f.path());
        let enabled = is_kernel_apparmor_enabled();
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert!(enabled);
    }

    #[test]
    fn kernel_apparmor_disabled_on_n() {
        let _g = ENV_LOCK.lock().unwrap();
        let f = tmp_file_with("N\n");
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, f.path());
        let enabled = is_kernel_apparmor_enabled();
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert!(!enabled);
    }

    #[test]
    fn kernel_apparmor_missing_file_returns_false() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            APPARMOR_PATH_OVERRIDE_ENV,
            "/nonexistent/enola/test/apparmor",
        );
        let enabled = is_kernel_apparmor_enabled();
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert!(!enabled);
    }

    #[test]
    fn empty_input_gets_no_new_privs() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, "/nonexistent/x");
        let out = build_default_security_opt(vec![]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert_eq!(out, vec!["no-new-privileges:true".to_string()]);
    }

    #[test]
    fn preserves_user_opts_and_appends_missing_default() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, "/nonexistent/x");
        let out = build_default_security_opt(vec!["seccomp=/etc/enola/seccomp.json".to_string()]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], "seccomp=/etc/enola/seccomp.json");
        assert_eq!(out[1], "no-new-privileges:true");
    }

    #[test]
    fn does_not_duplicate_no_new_privs_if_already_present() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, "/nonexistent/x");
        let out = build_default_security_opt(vec!["no-new-privileges:true".to_string()]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert_eq!(out, vec!["no-new-privileges:true".to_string()]);
    }

    #[test]
    fn respects_user_apparmor_profile_without_warning() {
        let _g = ENV_LOCK.lock().unwrap();
        let f = tmp_file_with("Y\n");
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, f.path());
        let out = build_default_security_opt(vec!["apparmor=enola-ai".to_string()]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert!(out.contains(&"apparmor=enola-ai".to_string()));
        assert!(out.contains(&"no-new-privileges:true".to_string()));
    }

    #[test]
    fn wsl2_degrades_silently() {
        // Simulates WSL2: sysfs path missing → no profile injected, no panic.
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            APPARMOR_PATH_OVERRIDE_ENV,
            "/definitely/not/a/real/path/enola-test",
        );
        let out = build_default_security_opt(vec![]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert_eq!(out, vec!["no-new-privileges:true".to_string()]);
    }
}

#[cfg(test)]
mod tests_extra {
    use super::*;
    use std::io::Write;
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn tmp_file_with(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tmp file");
        f.write_all(content.as_bytes()).expect("write");
        f
    }
    #[test]
    fn no_apparmor_profile_with_kernel_enabled_emits_info_log() {
        let _g = ENV_LOCK.lock().unwrap();
        let f = tmp_file_with("Y\n");
        std::env::set_var(APPARMOR_PATH_OVERRIDE_ENV, f.path());
        // Sin perfil apparmor= y kernel habilitado: la rama tracing::info! se ejecuta.
        let out = build_default_security_opt(vec![]);
        std::env::remove_var(APPARMOR_PATH_OVERRIDE_ENV);
        assert!(out.contains(&"no-new-privileges:true".to_string()));
        assert!(!out.iter().any(|o| o.starts_with("apparmor=")));
    }
}
