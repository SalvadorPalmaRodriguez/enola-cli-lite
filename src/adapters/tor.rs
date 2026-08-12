// MED-01: Prevent unwrap/expect in non-test code — panics in tor config
// can leave torrc/services in a half-written state.
#![warn(clippy::unwrap_used, clippy::expect_used)]
// LOW-02: `obfstr!` provides friction against casual `strings` analysis,
// NOT real protection. It does not stop static analysis (Ghidra, radare2).
// Do not add new uses of `obfstr!` — existing ones are kept for backwards
// compatibility but should not be expanded.
use crate::domain::error::{EnolaError, Result};
use crate::infrastructure::file_lock::FileLock;
use crate::ports::tor::{TorManagerPort, TorServiceInfo};
use data_encoding::BASE32_NOPAD;
use obfstr::obfstr;
use rand::rngs::OsRng;
use std::path::PathBuf;
use tokio::fs;
use tokio::fs::{create_dir_all, read_to_string, write, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use x25519_dalek::{PublicKey, StaticSecret};

/// Helper module for consistent permission error messages
mod permission_errors {
    use std::path::Path;

    const SUDO_HINT: &str = "This operation requires root privileges.\n\n  \
        Solution: Run with sudo\n    \
        sudo enola <command>\n\n  \
        Note: Tor hidden services management requires write access to:\n    \
        - /etc/tor/       (Tor configuration)\n    \
        - /var/lib/tor/   (Hidden service directories)\n    \
        - systemctl       (Service management)";

    const READ_ONLY_HINT: &str =
        "For read-only operations (listing services), you can alternatively:\n    \
        sudo usermod -aG debian-tor $USER && sudo chmod 750 /var/lib/tor\n    \
        (Requires logout/login to take effect)";

    pub fn read_error(path: &Path, operation: &str) -> String {
        format!(
            "Permission denied: Cannot {} '{}'\n\n  {}\n\n  {}",
            operation,
            path.display(),
            SUDO_HINT,
            READ_ONLY_HINT
        )
    }

    pub fn write_error(path: &Path, operation: &str) -> String {
        format!(
            "Permission denied: Cannot {} '{}'\n\n  {}",
            operation,
            path.display(),
            SUDO_HINT
        )
    }

    pub fn systemctl_error(action: &str) -> String {
        format!(
            "Permission denied: Cannot execute 'systemctl {} tor'\n\n  {}",
            action, SUDO_HINT
        )
    }

    #[allow(dead_code)]
    pub fn chown_chmod_error(path: &Path, operation: &str) -> String {
        format!(
            "Permission denied: Cannot {} '{}'\n\n  {}",
            operation,
            path.display(),
            SUDO_HINT
        )
    }
}

pub struct TorConfigAdapter {
    torrc_path: PathBuf,
    tor_svc_dir: PathBuf,
    enola_conf_dir: PathBuf,
}
impl Default for TorConfigAdapter {
    fn default() -> Self {
        Self::new()
    }
}
impl TorConfigAdapter {
    pub fn new() -> Self {
        Self {
            torrc_path: PathBuf::from(obfstr!("/etc/tor/torrc")),
            tor_svc_dir: PathBuf::from(obfstr!("/var/lib/tor")),
            enola_conf_dir: PathBuf::from(obfstr!("/etc/tor/enola.d")),
        }
    }
    pub fn with_paths(torrc: PathBuf, svc_dir: PathBuf, conf_dir: PathBuf) -> Self {
        Self {
            torrc_path: torrc,
            tor_svc_dir: svc_dir,
            enola_conf_dir: conf_dir,
        }
    }

    /// Set correct ownership for Tor config files (root:debian-tor, 640)
    async fn set_config_permissions(path: &std::path::Path) -> Result<()> {
        Command::new("chown")
            .arg("root:debian-tor")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chown failed: {}", e)))?;

        Command::new("chmod")
            .arg("640")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chmod failed: {}", e)))?;

        Ok(())
    }

    /// Set correct ownership for Tor service directories (debian-tor:debian-tor, 700)
    async fn set_service_dir_permissions(path: &std::path::Path) -> Result<()> {
        Command::new("chown")
            .arg("-R")
            .arg("debian-tor:debian-tor")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chown failed: {}", e)))?;

        Command::new("chmod")
            .arg("700")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chmod failed: {}", e)))?;

        Ok(())
    }

    /// Set correct ownership for client auth files (debian-tor:debian-tor, 600)
    async fn set_auth_file_permissions(path: &std::path::Path) -> Result<()> {
        Command::new("chown")
            .arg("debian-tor:debian-tor")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chown failed: {}", e)))?;

        Command::new("chmod")
            .arg("600")
            .arg(path)
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("chmod failed: {}", e)))?;

        Ok(())
    }
    async fn ensure_include(&self) -> Result<()> {
        let include_directive = format!(
            "{} {}/*.conf",
            obfstr!("%include"),
            self.enola_conf_dir.to_string_lossy()
        );
        if !self.torrc_path.exists() {
            return Ok(());
        }
        let content = read_to_string(&self.torrc_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(permission_errors::read_error(
                    &self.torrc_path,
                    "read Tor configuration",
                ))
            } else {
                EnolaError::InfrastructureError(format!("Failed to read torrc: {}", e))
            }
        })?;
        if !content.contains(&include_directive) {
            create_dir_all(&self.enola_conf_dir).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &self.enola_conf_dir,
                        "create configuration directory",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!(
                        "Failed to create enola conf dir: {}",
                        e
                    ))
                }
            })?;
            Command::new(obfstr!("chmod"))
                .arg(obfstr!("755"))
                .arg(&self.enola_conf_dir)
                .status()
                .await
                .ok();
            let mut file = OpenOptions::new()
                .append(true)
                .open(&self.torrc_path)
                .await
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        EnolaError::InfrastructureError(permission_errors::write_error(
                            &self.torrc_path,
                            "modify Tor configuration",
                        ))
                    } else {
                        EnolaError::InfrastructureError(format!("Failed to open torrc: {}", e))
                    }
                })?;
            file.write_all(format!("\n{}\n", include_directive).as_bytes())
                .await
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Failed to append include to torrc: {}",
                        e
                    ))
                })?;
        }
        if !self.enola_conf_dir.exists() {
            create_dir_all(&self.enola_conf_dir).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to create enola conf dir: {}", e))
            })?;
            Command::new(obfstr!("chmod"))
                .arg(obfstr!("755"))
                .arg(&self.enola_conf_dir)
                .status()
                .await
                .ok();
        }

        // Ensure at least one .conf exists to prevent Tor from failing on empty include globs
        let mut has_conf = false;
        if let Ok(mut rd) = fs::read_dir(&self.enola_conf_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                if let Ok(ft) = entry.file_type().await {
                    if ft.is_file() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.ends_with(".conf") {
                                has_conf = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if !has_conf {
            let placeholder = self.enola_conf_dir.join("00-empty.conf");
            write(&placeholder, b"# placeholder for Enola Tor includes\n")
                .await
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Failed to write placeholder conf: {}",
                        e
                    ))
                })?;
            // Set correct permissions for config file (root:debian-tor, 640)
            Self::set_config_permissions(&placeholder).await?;
        }
        Ok(())
    }

    /// Check if port 9050 is in use by an orphan Tor process (not managed by systemd) and kill it
    /// Returns true if an orphan process was killed
    async fn kill_orphan_tor_process(&self) -> bool {
        // First, check if tor@default is supposed to be running (managed by systemd)
        let systemd_status = Command::new("systemctl")
            .arg("is-active")
            .arg("tor@default")
            .output()
            .await;

        // If systemd says tor@default is "active", don't kill anything
        if let Ok(status) = systemd_status {
            if String::from_utf8_lossy(&status.stdout).trim() == "active" {
                return false; // Tor is managed by systemd, don't interfere
            }
        }

        // Check if something is using port 9050
        let lsof_output = Command::new("lsof")
            .arg("-i")
            .arg(":9050")
            .arg("-t") // Just output PIDs
            .output()
            .await;

        if let Ok(output) = lsof_output {
            let pids_str = String::from_utf8_lossy(&output.stdout);
            for pid_str in pids_str.lines() {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Check if this PID is from a systemd-managed tor service
                    let systemd_pid = Command::new("systemctl")
                        .arg("show")
                        .arg("tor@default")
                        .arg("--property=MainPID")
                        .arg("--value")
                        .output()
                        .await;

                    let is_systemd_managed = systemd_pid
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .trim()
                                .parse::<u32>()
                                .unwrap_or(0)
                                == pid
                        })
                        .unwrap_or(false);

                    if is_systemd_managed {
                        continue; // Skip systemd-managed processes
                    }

                    // Check if this is a tor process
                    let ps_output = Command::new("ps")
                        .arg("-p")
                        .arg(pid.to_string())
                        .arg("-o")
                        .arg("comm=")
                        .output()
                        .await;

                    if let Ok(ps) = ps_output {
                        let comm = String::from_utf8_lossy(&ps.stdout).trim().to_string();
                        if comm == "tor" {
                            eprintln!(
                                "   ⚠️  Found orphan Tor process (PID {}), killing it...",
                                pid
                            );
                            let _ = std::io::Write::flush(&mut std::io::stderr());
                            let _ = Command::new("kill").arg(pid.to_string()).output().await;
                            // Wait a moment for the process to die
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Find an available port within a range
    #[allow(dead_code)]
    async fn find_available_port(&self, preferred: u16, range_start: u16, range_end: u16) -> u16 {
        self.find_available_port_with_lock(preferred, range_start, range_end)
            .await
            .map(|(port, _lock)| port)
            .unwrap_or(preferred)
    }

    /// SEC-EXT-RACE-012: versión con lock RAII para cerrar TOCTOU.
    #[allow(dead_code)]
    async fn find_available_port_with_lock(
        &self,
        preferred: u16,
        range_start: u16,
        range_end: u16,
    ) -> std::result::Result<(u16, FileLock), std::io::Error> {
        use std::io::ErrorKind;
        use std::net::TcpListener;

        // Try preferred port first
        if TcpListener::bind(format!("127.0.0.1:{}", preferred)).is_ok() {
            if let Ok(lock) = crate::infrastructure::port_lock::acquire_port_lock(preferred) {
                if TcpListener::bind(format!("127.0.0.1:{}", preferred)).is_ok() {
                    return Ok((preferred, lock));
                }
            }
        }

        // Find a random available port in the range
        use rand::Rng;
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let port = rng.gen_range(range_start..range_end);
            if TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
                continue;
            }
            match crate::infrastructure::port_lock::acquire_port_lock(port) {
                Ok(lock) => {
                    if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
                        return Ok((port, lock));
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        }

        // Fallback: let the OS choose
        if let Ok(listener) = TcpListener::bind("127.0.0.1:0") {
            if let Ok(addr) = listener.local_addr() {
                let port = addr.port();
                if let Ok(lock) = crate::infrastructure::port_lock::acquire_port_lock(port) {
                    if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
                        return Ok((port, lock));
                    }
                }
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!(
                "No available port with lock in range {}-{} (preferred {})",
                range_start, range_end, preferred
            ),
        ))
    }
}

#[async_trait::async_trait]
impl TorManagerPort for TorConfigAdapter {
    async fn deploy_hidden_service(&self, name: &str, ports: Vec<(u16, u16)>) -> Result<String> {
        self.ensure_include().await?;

        let svc_path = self.tor_svc_dir.join(format!("enola_{}", name));

        // Create service directory with proper permissions
        if !svc_path.exists() {
            create_dir_all(&svc_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &svc_path,
                        "create service directory",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to create service dir: {}", e))
                }
            })?;
        }

        // Always set correct permissions (debian-tor:debian-tor, 700)
        Self::set_service_dir_permissions(&svc_path).await?;

        // Use ports as provided - for web services, Nginx is already listening on these ports
        // Port verification should be done BEFORE creating Nginx, not here
        let final_ports = ports;

        // Write configuration
        let mut conf_content = format!("# Enola Service: {}\n", name);
        conf_content.push_str(&format!(
            "HiddenServiceDir {}\n",
            svc_path.to_string_lossy()
        ));
        for (pub_port, target_port) in &final_ports {
            conf_content.push_str(&format!(
                "HiddenServicePort {} 127.0.0.1:{}\n",
                pub_port, target_port
            ));
        }

        let conf_file = self.enola_conf_dir.join(format!("{}.conf", name));
        write(&conf_file, &conf_content).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(permission_errors::write_error(
                    &conf_file,
                    "create service configuration",
                ))
            } else {
                EnolaError::InfrastructureError(format!("Failed to write conf file: {}", e))
            }
        })?;

        // Set correct permissions for config file (root:debian-tor, 640)
        Self::set_config_permissions(&conf_file).await?;

        // Reload Tor to pick up new config
        eprintln!("🔄 Reloading Tor service...");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        self.reload_tor().await?;

        // Wait for hostname file with longer timeout (30 seconds)
        eprint!("⏳ Waiting for onion address generation");
        use std::io::Write; // Import Write trait for flushing stdout/stderr if needed
        for i in 0..60 {
            if i % 2 == 0 {
                eprint!(".");
                let _ = std::io::stderr().flush();
            }
            if let Ok(onion) = self.get_onion_address(name).await {
                eprintln!("\n✅ Onion address generated.");
                return Ok(onion);
            }
            if i == 10 {
                eprint!("\n⚠️  Taking longer than expected. Attempting full restart...");
                // After 5 seconds, try restarting Tor instead of just reloading
                // Try tor@default first, then fallback to tor
                let _ = Command::new("systemctl")
                    .arg("restart")
                    .arg("tor@default")
                    .output()
                    .await;
                let _ = Command::new("systemctl")
                    .arg("restart")
                    .arg("tor")
                    .output()
                    .await;
                eprint!("\n⏳ Still waiting");
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        eprintln!("\n❌ Timeout waiting for hostname file.");

        // Check if Tor is running (try both service names)
        let mut tor_active = false;
        for service in ["tor@default", "tor"] {
            let output = Command::new("systemctl")
                .arg("is-active")
                .arg(service)
                .output()
                .await;

            if let Ok(o) = output {
                if String::from_utf8_lossy(&o.stdout).trim() == "active" {
                    tor_active = true;
                    break;
                }
            }
        }

        if !tor_active {
            return Err(EnolaError::InfrastructureError(
                "Tor service is not running. Try: sudo systemctl start tor".to_string(),
            ));
        }

        Err(EnolaError::InfrastructureError(
            "Tor did not generate hostname in 30s. Check /var/log/tor/log for errors.".to_string(),
        ))
    }
    async fn remove_hidden_service(&self, name: &str) -> Result<()> {
        let conf_file = self.enola_conf_dir.join(format!("{}.conf", name));
        let disabled_conf = self.enola_conf_dir.join(format!("{}.conf.disabled", name));
        let svc_path = self.tor_svc_dir.join(format!("enola_{}", name));

        // Track if we actually removed anything — prevents false success
        // when the service name doesn't match any existing config.
        let mut removed_something = false;

        if conf_file.exists() {
            tokio::fs::remove_file(&conf_file).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &conf_file,
                        "remove service configuration",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to remove conf file: {}", e))
                }
            })?;
            removed_something = true;
        }
        if disabled_conf.exists() {
            tokio::fs::remove_file(&disabled_conf).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &disabled_conf,
                        "remove disabled configuration",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!(
                        "Failed to remove disabled conf: {}",
                        e
                    ))
                }
            })?;
            removed_something = true;
        }
        if svc_path.exists() {
            tokio::fs::remove_dir_all(&svc_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &svc_path,
                        "remove service directory",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to delete keys dir: {}", e))
                }
            })?;
            removed_something = true;
        }

        if !removed_something {
            return Err(EnolaError::NotFound(format!(
                "Tor service '{}' not found (no conf or service dir exists)",
                name
            )));
        }

        self.reload_tor().await?;
        Ok(())
    }

    async fn get_onion_address(&self, name: &str) -> Result<String> {
        let hostname_path = self
            .tor_svc_dir
            .join(format!("enola_{}", name))
            .join("hostname");

        if !hostname_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Hostname file not found for {}",
                name
            )));
        }

        let content = read_to_string(&hostname_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(permission_errors::read_error(
                    &hostname_path,
                    "read onion address",
                ))
            } else {
                EnolaError::InfrastructureError(format!("Failed to read hostname: {}", e))
            }
        })?;

        Ok(content.trim().to_string())
    }

    async fn reload_tor(&self) -> Result<()> {
        // Order matters: try tor@default first (real instance), then tor (may be multi-instance master)
        let services = ["tor@default", "tor"];
        let mut last_error = String::new();
        let mut any_success = false;
        let mut retry_after_kill = false;

        eprintln!("🔍 Checking Tor service status...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        for service in services {
            // Check status
            let status = Command::new("systemctl")
                .arg("is-active")
                .arg(service)
                .output()
                .await;

            let status_str = status
                .as_ref()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|e| format!("error: {}", e));

            eprintln!("   {} status: {}", service, status_str);
            let _ = std::io::Write::flush(&mut std::io::stderr());

            // Determine actions based on status
            let actions: Vec<&str> = match status_str.as_str() {
                "active" => vec!["reload"],
                "failed" => vec!["restart", "start"],
                "inactive" => vec!["start"],
                _ => vec!["start", "restart"],
            };

            for action in actions {
                eprintln!("   ⚙️  Trying: systemctl {} {}...", action, service);
                let _ = std::io::Write::flush(&mut std::io::stderr());

                let output = Command::new("systemctl")
                    .arg(action)
                    .arg(service)
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        eprintln!("   ✅ Success: {} {}", action, service);
                        let _ = std::io::Write::flush(&mut std::io::stderr());
                        any_success = true;
                        // Don't return yet - we may need to reload another service too
                        break;
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        let error_detail = if !stderr.is_empty() {
                            stderr.clone()
                        } else {
                            format!("exit code {}", o.status)
                        };
                        eprintln!("   ⚠️  {} {} failed: {}", action, service, error_detail);
                        let _ = std::io::Write::flush(&mut std::io::stderr());
                        last_error = error_detail;

                        // Check if failure is due to port 9050 being in use (orphan tor process)
                        if service == "tor@default"
                            && !retry_after_kill
                            && (stderr.contains("Address already in use")
                                || stderr.contains("exit-code"))
                            && self.kill_orphan_tor_process().await
                        {
                            retry_after_kill = true;
                            eprintln!("   🔄 Retrying after killing orphan process...");
                            let _ = std::io::Write::flush(&mut std::io::stderr());
                            // Try restart again
                            let retry = Command::new("systemctl")
                                .arg("restart")
                                .arg("tor@default")
                                .output()
                                .await;
                            if let Ok(r) = retry {
                                if r.status.success() {
                                    eprintln!(
                                        "   ✅ Success after killing orphan: restart tor@default"
                                    );
                                    let _ = std::io::Write::flush(&mut std::io::stderr());
                                    any_success = true;
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("   ❌ Command error: {}", e);
                        let _ = std::io::Write::flush(&mut std::io::stderr());
                        last_error = e.to_string();
                    }
                }
            }
        }

        // If at least one service was successfully reloaded/started, consider it success
        if any_success {
            eprintln!("✅ Tor service ready.");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            return Ok(());
        }

        // All failed
        eprintln!("❌ All Tor reload/start attempts failed.");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        if last_error.contains("Access denied")
            || last_error.contains("Permission denied")
            || last_error.contains("authentication required")
            || last_error.contains("polkit")
        {
            return Err(EnolaError::InfrastructureError(
                permission_errors::systemctl_error("start/reload"),
            ));
        }

        Err(EnolaError::InfrastructureError(format!(
            "Tor start/reload failed. Last error: {}.\n\nTroubleshooting:\n  1. sudo systemctl status tor@default\n  2. sudo journalctl -u tor@default -n 50\n  3. Check /var/log/tor/log",
            last_error
        )))
    }

    async fn stop_hidden_service(&self, name: &str) -> Result<()> {
        let conf_file = self.enola_conf_dir.join(format!("{}.conf", name));
        let disabled_conf = self.enola_conf_dir.join(format!("{}.conf.disabled", name));
        if !conf_file.exists() {
            if disabled_conf.exists() {
                return Ok(());
            }
            return Err(EnolaError::NotFound(format!(
                "Service '{}' not found. Use 'enola-cli tor list' to see available services.",
                name
            )));
        }
        tokio::fs::rename(&conf_file, &disabled_conf)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &conf_file,
                        "disable service (rename config)",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to disable service: {}", e))
                }
            })?;
        self.reload_tor().await?;
        Ok(())
    }
    async fn start_hidden_service(&self, name: &str) -> Result<()> {
        let conf_file = self.enola_conf_dir.join(format!("{}.conf", name));
        let disabled_conf = self.enola_conf_dir.join(format!("{}.conf.disabled", name));
        if !disabled_conf.exists() {
            if conf_file.exists() {
                return Ok(());
            }
            return Err(EnolaError::NotFound(format!(
                "Service '{}' not found. Use 'enola-cli tor list' to see available services.",
                name
            )));
        }
        tokio::fs::rename(&disabled_conf, &conf_file)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &disabled_conf,
                        "enable service (rename config)",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to enable service: {}", e))
                }
            })?;
        self.reload_tor().await?;
        Ok(())
    }
    async fn generate_client_keys(&self, _client_name: &str) -> Result<(String, String)> {
        let rng = OsRng;
        let private_key = StaticSecret::random_from_rng(rng);
        let public_key = PublicKey::from(&private_key);
        let public_b32 = BASE32_NOPAD.encode(public_key.as_bytes());
        let private_b32 = BASE32_NOPAD.encode(private_key.as_bytes());
        Ok((public_b32, private_b32))
    }
    async fn add_client_auth(
        &self,
        service_name: &str,
        client_name: &str,
        public_key: &str,
    ) -> Result<()> {
        let svc_dir = self
            .tor_svc_dir
            .join(format!("{}_{}", obfstr!("enola"), service_name));
        let auth_dir = svc_dir.join(obfstr!("authorized_clients"));
        if !auth_dir.exists() {
            return Err(EnolaError::InfrastructureError(format!(
                "Client authorization is not enabled for service '{}'. Enable it first with:\n  sudo enola-cli tor auth enable --service {}",
                service_name, service_name
            )));
        }
        let file_path = auth_dir.join(format!("{}.auth", client_name));
        let content = format!(
            "{}:{}:{}",
            obfstr!("descriptor"),
            obfstr!("x25519"),
            public_key
        );
        write(&file_path, content).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(permission_errors::write_error(
                    &file_path,
                    "write client auth file",
                ))
            } else {
                EnolaError::InfrastructureError(format!("Failed to write auth file: {}", e))
            }
        })?;

        // Set correct permissions for auth file
        Self::set_auth_file_permissions(&file_path).await?;

        Ok(())
    }
    async fn revoke_client_auth(&self, service_name: &str, client_name: &str) -> Result<()> {
        let svc_dir = self.tor_svc_dir.join(format!("enola_{}", service_name));
        let auth_dir = svc_dir.join("authorized_clients");
        let file_path = auth_dir.join(format!("{}.auth", client_name));
        if file_path.exists() {
            tokio::fs::remove_file(&file_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &file_path,
                        "remove client auth file",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to delete auth file: {}", e))
                }
            })?;
        } else {
            return Err(EnolaError::NotFound(format!(
                "Client '{}' not found in service '{}'. Use 'enola-cli tor auth list --service {}' to see clients.",
                client_name, service_name, service_name
            )));
        }
        Ok(())
    }
    async fn enable_client_auth(&self, service_name: &str) -> Result<()> {
        let svc_dir = self.tor_svc_dir.join(format!("enola_{}", service_name));
        let auth_dir = svc_dir.join("authorized_clients");
        if !svc_dir.exists() {
            return Err(EnolaError::NotFound(format!(
                "Service '{}' not found. Use 'enola-cli tor list' to see available services.",
                service_name
            )));
        }
        if !auth_dir.exists() {
            create_dir_all(&auth_dir).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    EnolaError::InfrastructureError(permission_errors::write_error(
                        &auth_dir,
                        "create authorized_clients directory",
                    ))
                } else {
                    EnolaError::InfrastructureError(format!("Failed to create auth dir: {}", e))
                }
            })?;

            // Set correct permissions for auth directory
            Self::set_service_dir_permissions(&auth_dir).await?;
        }
        Ok(())
    }
    async fn disable_client_auth(&self, service_name: &str) -> Result<()> {
        let svc_dir = self.tor_svc_dir.join(format!("enola_{}", service_name));
        let auth_dir = svc_dir.join("authorized_clients");
        let disabled_auth_dir = svc_dir.join("authorized_clients.disabled");
        if auth_dir.exists() {
            tokio::fs::rename(&auth_dir, &disabled_auth_dir)
                .await
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        EnolaError::InfrastructureError(permission_errors::write_error(
                            &auth_dir,
                            "disable client auth (rename directory)",
                        ))
                    } else {
                        EnolaError::InfrastructureError(format!(
                            "Failed to disable auth (rename dir): {}",
                            e
                        ))
                    }
                })?;
        }
        self.reload_tor().await?;
        Ok(())
    }
    async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
        // If Tor service directory doesn't exist, return empty list (Tor not installed/configured)
        if !self.tor_svc_dir.exists() {
            return Ok(vec![]);
        }

        let mut services = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.tor_svc_dir).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(
                    permission_errors::read_error(&self.tor_svc_dir, "list hidden services")
                )
            } else if e.kind() == std::io::ErrorKind::NotFound {
                EnolaError::InfrastructureError(format!(
                    "Tor directory '{}' not found.\n\n  Is Tor installed? Try:\n    sudo apt install tor\n    sudo systemctl start tor",
                    self.tor_svc_dir.display()
                ))
            } else {
                EnolaError::InfrastructureError(format!(
                    "Cannot read Tor directory '{}': {}",
                    self.tor_svc_dir.display(), e
                ))
            }
        })?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            // Support both legacy "hidden_service_" prefix and new "enola_" prefix
            let name = if dir_name.starts_with("enola_") {
                dir_name.strip_prefix("enola_").map(|s| s.to_string())
            } else if dir_name.starts_with("hidden_service_") {
                dir_name
                    .strip_prefix("hidden_service_")
                    .map(|s| s.to_string())
            } else {
                None
            };
            if let Some(name) = name {
                let hostname = self
                    .get_onion_address(&name)
                    .await
                    .unwrap_or_else(|_| "pending...".to_string());
                let mut clients = Vec::new();
                let auth_dir = path.join("authorized_clients");
                if auth_dir.exists() {
                    if let Ok(mut client_entries) = tokio::fs::read_dir(&auth_dir).await {
                        while let Ok(Some(client_entry)) = client_entries.next_entry().await {
                            let fname = client_entry.file_name().to_string_lossy().to_string();
                            if fname.ends_with(".auth") {
                                clients.push(fname.trim_end_matches(".auth").to_string());
                            }
                        }
                    }
                }
                let conf_file = self.enola_conf_dir.join(format!("{}.conf", name));
                let disabled_conf = self.enola_conf_dir.join(format!("{}.conf.disabled", name));
                let active = conf_file.exists();
                let effective_conf_path = if active { &conf_file } else { &disabled_conf };
                let mut ports = Vec::new();
                let mut auth_enabled = !clients.is_empty();
                if effective_conf_path.exists() {
                    if let Ok(content) = read_to_string(effective_conf_path).await {
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("HiddenServicePort") {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 3 {
                                    if let Ok(pub_p) = parts[1].parse::<u16>() {
                                        ports.push((pub_p, parts[2].to_string()));
                                    }
                                }
                            } else if trimmed.starts_with("HiddenServiceAuthorizeClient") {
                                auth_enabled = true;
                            }
                        }
                    }
                }

                services.push(TorServiceInfo {
                    name,
                    hostname,
                    hidden_service_dir: path.to_string_lossy().to_string(),
                    ports,
                    clients,
                    active,
                    auth_enabled,
                });
            }
        }
        Ok(services)
    }
    async fn rotate_hidden_service_identity(&self, name: &str) -> Result<String> {
        self.stop_hidden_service(name).await?;
        let svc_path = self.tor_svc_dir.join(format!("enola_{}", name));
        if svc_path.exists() {
            let files_to_delete = vec![
                "hostname",
                "hs_ed25519_public_key",
                "hs_ed25519_secret_key",
                "private_key",
            ];
            for f in files_to_delete {
                let p = svc_path.join(f);
                if p.exists() {
                    tokio::fs::remove_file(p).await.ok();
                }
            }
        }
        self.start_hidden_service(name).await?;
        for _ in 0..60 {
            if let Ok(onion) = self.get_onion_address(name).await {
                return Ok(onion);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(EnolaError::InfrastructureError(
            "Timeout waiting for new address".to_string(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a TorConfigAdapter with temp directories for testing
    fn test_adapter() -> (TorConfigAdapter, TempDir, TempDir, TempDir) {
        let torrc_dir = TempDir::new().unwrap();
        let svc_dir = TempDir::new().unwrap();
        let conf_dir = TempDir::new().unwrap();

        let torrc_path = torrc_dir.path().join("torrc");
        std::fs::write(&torrc_path, "# test torrc\n").unwrap();

        let adapter = TorConfigAdapter::with_paths(
            torrc_path,
            svc_dir.path().to_path_buf(),
            conf_dir.path().to_path_buf(),
        );
        (adapter, torrc_dir, svc_dir, conf_dir)
    }

    // ═══════════════════════════════════════════════════════════════
    // list_hidden_services
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_list_hidden_services_empty_dir() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let services = adapter.list_hidden_services().await.unwrap();
        assert!(services.is_empty());
    }

    #[tokio::test]
    async fn test_list_hidden_services_nonexistent_dir_returns_empty() {
        let adapter = TorConfigAdapter::with_paths(
            PathBuf::from("/nonexistent/torrc"),
            PathBuf::from("/nonexistent/tor_svc_dir"),
            PathBuf::from("/nonexistent/enola.d"),
        );
        let services = adapter.list_hidden_services().await.unwrap();
        assert!(services.is_empty());
    }

    #[tokio::test]
    async fn test_list_hidden_services_finds_enola_prefix() {
        let (adapter, _torrc, svc_dir, conf_dir) = test_adapter();

        // Create a service directory with enola_ prefix and hostname
        let svc_path = svc_dir.path().join("enola_myservice");
        std::fs::create_dir_all(&svc_path).unwrap();
        std::fs::write(svc_path.join("hostname"), "abc123.onion\n").unwrap();

        // Create matching config
        let conf_content = "# Enola Service: myservice\nHiddenServiceDir /tmp/test\nHiddenServicePort 80 127.0.0.1:8080\n";
        std::fs::write(conf_dir.path().join("myservice.conf"), conf_content).unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "myservice");
        assert_eq!(services[0].hostname, "abc123.onion");
        assert!(services[0].active);
        assert_eq!(services[0].ports.len(), 1);
        assert_eq!(services[0].ports[0].0, 80);
        assert_eq!(services[0].ports[0].1, "127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_list_hidden_services_finds_legacy_prefix() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        let svc_path = svc_dir.path().join("hidden_service_legacy");
        std::fs::create_dir_all(&svc_path).unwrap();
        std::fs::write(svc_path.join("hostname"), "legacy.onion\n").unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "legacy");
        // get_onion_address looks in enola_ prefix, so legacy hostname shows as pending
        assert_eq!(services[0].hostname, "pending...");
    }

    #[tokio::test]
    async fn test_list_hidden_services_ignores_non_prefixed() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        // Create a dir that doesn't match any prefix
        let svc_path = svc_dir.path().join("random_dir");
        std::fs::create_dir_all(&svc_path).unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert!(services.is_empty());
    }

    #[tokio::test]
    async fn test_list_hidden_services_inactive_service() {
        let (adapter, _torrc, svc_dir, conf_dir) = test_adapter();

        let svc_path = svc_dir.path().join("enola_stopped");
        std::fs::create_dir_all(&svc_path).unwrap();
        std::fs::write(svc_path.join("hostname"), "stopped.onion\n").unwrap();

        // Only .conf.disabled exists (service stopped)
        let conf_content = "HiddenServiceDir /tmp/test\nHiddenServicePort 443 127.0.0.1:8443\n";
        std::fs::write(conf_dir.path().join("stopped.conf.disabled"), conf_content).unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert!(!services[0].active);
        assert_eq!(services[0].ports[0].0, 443);
    }

    #[tokio::test]
    async fn test_list_hidden_services_with_clients() {
        let (adapter, _torrc, svc_dir, conf_dir) = test_adapter();

        let svc_path = svc_dir.path().join("enola_authed");
        let auth_dir = svc_path.join("authorized_clients");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::write(svc_path.join("hostname"), "authed.onion\n").unwrap();
        std::fs::write(auth_dir.join("alice.auth"), "descriptor:x25519:KEYDATA").unwrap();
        std::fs::write(auth_dir.join("bob.auth"), "descriptor:x25519:KEYDATA").unwrap();

        std::fs::write(
            conf_dir.path().join("authed.conf"),
            "HiddenServiceDir /tmp\nHiddenServicePort 80 127.0.0.1:8080\n",
        )
        .unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].clients.len(), 2);
        assert!(services[0].clients.contains(&"alice".to_string()));
        assert!(services[0].clients.contains(&"bob".to_string()));
    }

    #[tokio::test]
    async fn test_list_hidden_services_pending_hostname() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        // Dir exists but no hostname file yet
        let svc_path = svc_dir.path().join("enola_pending");
        std::fs::create_dir_all(&svc_path).unwrap();

        let services = adapter.list_hidden_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].hostname, "pending...");
    }

    // ═══════════════════════════════════════════════════════════════
    // get_onion_address
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_get_onion_address_success() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        let svc_path = svc_dir.path().join("enola_test");
        std::fs::create_dir_all(&svc_path).unwrap();
        std::fs::write(svc_path.join("hostname"), "test123.onion\n").unwrap();

        let addr = adapter.get_onion_address("test").await.unwrap();
        assert_eq!(addr, "test123.onion");
    }

    #[tokio::test]
    async fn test_get_onion_address_not_found() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let result = adapter.get_onion_address("nonexistent").await;
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════
    // stop / start hidden service
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_stop_hidden_service_not_found() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let result = adapter.stop_hidden_service("nonexistent").await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found': {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_start_hidden_service_not_found() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let result = adapter.start_hidden_service("nonexistent").await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found': {}",
            err_msg
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // enable / disable client auth
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_enable_client_auth_service_not_found() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let result = adapter.enable_client_auth("nonexistent").await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found': {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_enable_client_auth_creates_dir() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        let svc_path = svc_dir.path().join("enola_authtest");
        std::fs::create_dir_all(&svc_path).unwrap();

        // Should succeed and create authorized_clients dir
        let result = adapter.enable_client_auth("authtest").await;
        assert!(result.is_ok());
        assert!(svc_path.join("authorized_clients").exists());
    }

    #[tokio::test]
    async fn test_add_client_auth_not_enabled() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        let svc_path = svc_dir.path().join("enola_noauth");
        std::fs::create_dir_all(&svc_path).unwrap();
        // No authorized_clients dir → error

        let result = adapter.add_client_auth("noauth", "alice", "PUBKEY").await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not enabled"),
            "Error should mention 'not enabled': {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_revoke_client_auth_not_found() {
        let (adapter, _torrc, svc_dir, _conf) = test_adapter();

        let svc_path = svc_dir.path().join("enola_revoketest");
        let auth_dir = svc_path.join("authorized_clients");
        std::fs::create_dir_all(&auth_dir).unwrap();

        let result = adapter
            .revoke_client_auth("revoketest", "nonexistent")
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found': {}",
            err_msg
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // generate_client_keys
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_generate_client_keys_returns_valid_keys() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let (pub_key, priv_key) = adapter.generate_client_keys("testclient").await.unwrap();
        assert!(!pub_key.is_empty());
        assert!(!priv_key.is_empty());
        // Keys should be base32 encoded (uppercase alpha + digits)
        assert!(pub_key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        assert!(priv_key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    }

    #[tokio::test]
    async fn test_generate_client_keys_unique() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        let (pub1, priv1) = adapter.generate_client_keys("client1").await.unwrap();
        let (pub2, priv2) = adapter.generate_client_keys("client2").await.unwrap();
        // Keys should differ between calls
        assert_ne!(pub1, pub2);
        assert_ne!(priv1, priv2);
    }

    // ═══════════════════════════════════════════════════════════════
    // Display / user-friendly error messages
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_error_messages_are_user_friendly() {
        let adapter = TorConfigAdapter::with_paths(
            PathBuf::from("/nonexistent/torrc"),
            PathBuf::from("/nonexistent/tor_svc"),
            PathBuf::from("/nonexistent/enola.d"),
        );

        // list should gracefully return empty (not error)
        let list_result = adapter.list_hidden_services().await;
        assert!(list_result.is_ok());
        assert!(list_result.unwrap().is_empty());

        // get_onion_address should give NotFound
        let addr_result = adapter.get_onion_address("test").await;
        assert!(addr_result.is_err());

        // stop should give user-friendly error
        let stop_result = adapter.stop_hidden_service("test").await;
        assert!(stop_result.is_err());
        let err = format!("{}", stop_result.unwrap_err());
        assert!(
            err.contains("not found") || err.contains("enola-cli tor list"),
            "Error should be user-friendly: {}",
            err
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // remove_hidden_service (partial — no systemctl reload)
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_remove_nonexistent_service_no_panic() {
        let (adapter, _torrc, _svc, _conf) = test_adapter();
        // remove_hidden_service calls reload_tor which needs systemctl,
        // but if conf/dir don't exist it should still proceed to reload
        // (which will fail in test env, but shouldn't panic)
        let result = adapter.remove_hidden_service("nonexistent").await;
        // Will error at reload_tor but that's expected in test env
        // The important thing is it doesn't panic
        let _ = result;
    }
}
