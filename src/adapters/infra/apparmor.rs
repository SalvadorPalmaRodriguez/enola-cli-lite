/// AppArmor adapter — executes system commands (apparmor_parser, aa-status, aa-complain, aa-enforce).
/// Tarea AA-003 (203).
///
/// Concrete implementation of `AppArmorPort`.
use std::process::Command;

use crate::domain::apparmor::{
    AppArmorMode, AppArmorProfile, AppArmorServiceType, AppArmorStatus, AppArmorViolation,
};
use crate::domain::error::EnolaError;
use crate::ports::apparmor::AppArmorPort;

type Result<T> = std::result::Result<T, EnolaError>;

/// Path where Enola stores its AppArmor profiles
const APPARMOR_PROFILE_DIR: &str = "/etc/apparmor.d";

pub struct AppArmorAdapter;

impl Default for AppArmorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AppArmorAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Write profile content to /etc/apparmor.d/{profile_name}
    fn write_profile_file(&self, profile_name: &str, content: &str) -> Result<()> {
        let path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);
        std::fs::write(&path, content).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write AppArmor profile to {}: {}. Run with sudo.",
                path, e
            ))
        })
    }

    /// Remove profile file from /etc/apparmor.d/
    fn remove_profile_file(&self, profile_name: &str) -> Result<()> {
        let path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_file(&path).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Failed to remove AppArmor profile {}: {}",
                    path, e
                ))
            })?;
        }
        Ok(())
    }

    /// Parse `aa-status` output to extract Enola profiles
    fn parse_aa_status_output(&self, output: &str) -> Vec<AppArmorProfile> {
        let mut profiles = Vec::new();
        let mut current_mode = AppArmorMode::Enforce;

        for line in output.lines() {
            let trimmed = line.trim();

            // Detect section headers
            if trimmed.contains("profiles are in enforce mode") {
                current_mode = AppArmorMode::Enforce;
                continue;
            }
            if trimmed.contains("profiles are in complain mode") {
                current_mode = AppArmorMode::Complain;
                continue;
            }

            // Only care about enola-* profiles
            if !trimmed.starts_with("enola-") {
                continue;
            }

            let profile_name = trimmed.trim_end_matches(')').trim();
            // Remove trailing " (enforce)" or " (complain)" if present
            let name = profile_name
                .replace(" (enforce)", "")
                .replace(" (complain)", "")
                .trim()
                .to_string();

            if name.is_empty() {
                continue;
            }

            let service_type = if name.starts_with("enola-git-") {
                AppArmorServiceType::Git
            } else if name.starts_with("enola-wp-") {
                AppArmorServiceType::WordPress
            } else if name == "enola-nginx" {
                AppArmorServiceType::Nginx
            } else if name == "enola-tor" {
                AppArmorServiceType::Tor
            } else if name == "enola-docker-base" {
                AppArmorServiceType::DockerBase
            } else {
                continue; // Not an Enola profile
            };

            profiles.push(AppArmorProfile {
                name,
                mode: current_mode.clone(),
                service_type,
            });
        }

        profiles
    }

    /// Parse syslog for recent AppArmor violations (last 24h)
    fn parse_recent_violations(&self) -> Vec<AppArmorViolation> {
        let output = Command::new("grep")
            .args(["-i", "apparmor.*denied", "/var/log/syslog"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return vec![],
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut violations = Vec::new();

        // Only take last 50 lines to avoid flooding
        for line in stdout.lines().rev().take(50) {
            // Only include enola-* profile violations
            if !line.contains("enola-") {
                continue;
            }

            let timestamp = line
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            let operation = if line.contains("DENIED") {
                "DENIED"
            } else {
                "AUDIT"
            };

            // Extract path if present: name="/path/to/file"
            let path = line
                .split("name=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(String::from);

            // Extract profile name
            let profile = line
                .split("profile=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or("unknown")
                .to_string();

            violations.push(AppArmorViolation {
                timestamp,
                profile,
                operation: operation.to_string(),
                path,
            });
        }

        violations.reverse();
        violations
    }
}

impl AppArmorPort for AppArmorAdapter {
    fn is_installed(&self) -> bool {
        Command::new("which")
            .arg("apparmor_parser")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_enabled(&self) -> Result<bool> {
        // Check /sys/module/apparmor/parameters/enabled
        let path = "/sys/module/apparmor/parameters/enabled";
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(content.trim() == "Y"),
            Err(_) => {
                // File doesn't exist = kernel module not loaded (e.g., WSL2)
                Ok(false)
            }
        }
    }

    fn load_profile(
        &self,
        profile_name: &str,
        profile_content: &str,
        mode: AppArmorMode,
    ) -> Result<()> {
        // 1. Write profile to /etc/apparmor.d/
        self.write_profile_file(profile_name, profile_content)?;

        let profile_path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);

        // 2. Load with apparmor_parser based on mode
        let args = match mode {
            AppArmorMode::Complain => vec!["-r", "-C", "-W", &profile_path],
            AppArmorMode::Enforce => vec!["-r", "-W", &profile_path],
            AppArmorMode::Disabled => {
                // Just write the file, don't load
                return Ok(());
            }
        };

        let output = Command::new("apparmor_parser")
            .args(&args)
            .output()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Failed to run apparmor_parser: {}. Is apparmor installed?",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EnolaError::InfrastructureError(format!(
                "apparmor_parser failed for '{}': {}",
                profile_name, stderr
            )));
        }

        Ok(())
    }

    fn unload_profile(&self, profile_name: &str) -> Result<()> {
        let profile_path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);

        // Unload from kernel
        if std::path::Path::new(&profile_path).exists() {
            let output = Command::new("apparmor_parser")
                .args(["-R", &profile_path])
                .output()
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Failed to unload AppArmor profile '{}': {}",
                        profile_name, e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Not fatal — profile may not have been loaded
                tracing::warn!(
                    "apparmor_parser -R warning for '{}': {}",
                    profile_name,
                    stderr
                );
            }
        }

        // Remove file
        self.remove_profile_file(profile_name)
    }

    fn set_mode(&self, profile_name: &str, mode: AppArmorMode) -> Result<()> {
        let profile_path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);

        if !std::path::Path::new(&profile_path).exists() {
            return Err(EnolaError::InfrastructureError(format!(
                "AppArmor profile '{}' not found in {}",
                profile_name, APPARMOR_PROFILE_DIR
            )));
        }

        let cmd = match mode {
            AppArmorMode::Complain => "aa-complain",
            AppArmorMode::Enforce => "aa-enforce",
            AppArmorMode::Disabled => {
                return self.unload_profile(profile_name);
            }
        };

        let output = Command::new(cmd).arg(&profile_path).output().map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to run {}: {}. Install apparmor-utils.",
                cmd, e
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EnolaError::InfrastructureError(format!(
                "{} failed for '{}': {}",
                cmd, profile_name, stderr
            )));
        }

        Ok(())
    }

    fn status(&self) -> Result<AppArmorStatus> {
        if !self.is_installed() {
            return Ok(AppArmorStatus {
                installed: false,
                enabled: false,
                profiles: vec![],
                recent_violations: vec![],
            });
        }

        let enabled = self.is_enabled()?;
        if !enabled {
            return Ok(AppArmorStatus {
                installed: true,
                enabled: false,
                profiles: vec![],
                recent_violations: vec![],
            });
        }

        // Get loaded profiles from aa-status
        let output = Command::new("aa-status").output().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to run aa-status: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let profiles = self.parse_aa_status_output(&stdout);
        let recent_violations = self.parse_recent_violations();

        Ok(AppArmorStatus {
            installed: true,
            enabled: true,
            profiles,
            recent_violations,
        })
    }

    fn docker_security_opt(&self, profile_name: &str) -> Option<String> {
        // Check if AppArmor is enabled and profile is loaded
        let enabled = self.is_enabled().unwrap_or(false);
        if !enabled {
            return None;
        }

        // Check if profile file exists (means it was loaded)
        let profile_path = format!("{}/{}", APPARMOR_PROFILE_DIR, profile_name);
        if std::path::Path::new(&profile_path).exists() {
            Some(format!("apparmor={}", profile_name))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_aa_status_empty() {
        let adapter = AppArmorAdapter::new();
        let profiles = adapter.parse_aa_status_output("");
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_parse_aa_status_with_enola_profiles() {
        let adapter = AppArmorAdapter::new();
        let output = r#"
apparmor module is loaded.
47 profiles are loaded.
42 profiles are in enforce mode.
   /snap/snapd/21759/usr/lib/snapd/snap-confine
   enola-nginx
   enola-tor
   enola-docker-base
3 profiles are in complain mode.
   enola-git-myserver
   enola-wp-myblog
2 processes have profiles defined.
"#;
        let profiles = adapter.parse_aa_status_output(output);
        assert_eq!(profiles.len(), 5);

        // Enforce profiles
        assert_eq!(profiles[0].name, "enola-nginx");
        assert_eq!(profiles[0].mode, AppArmorMode::Enforce);

        assert_eq!(profiles[1].name, "enola-tor");
        assert_eq!(profiles[1].mode, AppArmorMode::Enforce);

        assert_eq!(profiles[2].name, "enola-docker-base");
        assert_eq!(profiles[2].mode, AppArmorMode::Enforce);

        // Complain profiles
        assert_eq!(profiles[3].name, "enola-git-myserver");
        assert_eq!(profiles[3].mode, AppArmorMode::Complain);

        assert_eq!(profiles[4].name, "enola-wp-myblog");
        assert_eq!(profiles[4].mode, AppArmorMode::Complain);
    }

    #[test]
    fn test_parse_aa_status_ignores_non_enola() {
        let adapter = AppArmorAdapter::new();
        let output = r#"
3 profiles are in enforce mode.
   /usr/sbin/mysqld
   /snap/snapd/21759/usr/lib/snapd/snap-confine
   enola-nginx
1 profiles are in complain mode.
   /usr/sbin/ntpd
"#;
        let profiles = adapter.parse_aa_status_output(output);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "enola-nginx");
    }

    #[test]
    fn test_parse_aa_status_detects_service_types() {
        let adapter = AppArmorAdapter::new();
        let output = r#"
4 profiles are in enforce mode.
   enola-git-server1
   enola-wp-blog1
   enola-nginx
   enola-tor
"#;
        let profiles = adapter.parse_aa_status_output(output);
        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[0].service_type, AppArmorServiceType::Git);
        assert_eq!(profiles[1].service_type, AppArmorServiceType::WordPress);
        assert_eq!(profiles[2].service_type, AppArmorServiceType::Nginx);
        assert_eq!(profiles[3].service_type, AppArmorServiceType::Tor);
    }
}
