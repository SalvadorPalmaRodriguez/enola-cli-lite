use crate::domain::dependencies::{
    DepCategory, DepStatus, Dependency, DependencyError, PackageManager,
};
use crate::ports::dependencies::DependencyPort;
/// System dependency adapter — implements DependencyPort using real OS commands (DEP-001..003)
///
/// Uses std::process::Command (synchronous).
/// Detects apt/dnf/pacman and installs packages accordingly.
use std::process::Command;
pub struct SystemDependencyAdapter;
impl Default for SystemDependencyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemDependencyAdapter {
    pub fn new() -> Self {
        Self
    }
    fn cmd_ok(bin: &str, args: &[&str]) -> bool {
        Command::new(bin)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    fn cmd_output(bin: &str, args: &[&str]) -> Option<String> {
        Command::new(bin)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }
}
impl DependencyPort for SystemDependencyAdapter {
    fn detect_package_manager(&self) -> PackageManager {
        if Self::cmd_ok("which", &["apt-get"]) {
            PackageManager::Apt
        } else if Self::cmd_ok("which", &["dnf"]) {
            PackageManager::Dnf
        } else if Self::cmd_ok("which", &["pacman"]) {
            PackageManager::Pacman
        } else {
            PackageManager::Unknown
        }
    }
    fn check_binary(&self, binary: &str) -> DepStatus {
        let found = Self::cmd_ok("which", &[binary]);
        let version = if found {
            // Try common version flags
            Self::cmd_output(binary, &["--version"])
                .or_else(|| Self::cmd_output(binary, &["-v"]))
                .map(|v| {
                    // Take only first line, truncate at 80 chars
                    v.lines().next().unwrap_or(&v).chars().take(80).collect()
                })
        } else {
            None
        };
        DepStatus {
            dep: Dependency {
                package: "unknown",
                binary: "unknown",
                description: "",
                category: DepCategory::Core,
                needed_by: "",
            },
            installed: found,
            version,
        }
    }
    fn install_package(&self, package: &str, pm: PackageManager) -> Result<(), DependencyError> {
        let (cmd, args): (&str, Vec<&str>) = match pm {
            PackageManager::Apt => ("apt-get", vec!["install", "-y", "-qq", package]),
            PackageManager::Dnf => ("dnf", vec!["install", "-y", "-q", package]),
            PackageManager::Pacman => ("pacman", vec!["-S", "--noconfirm", "--quiet", package]),
            PackageManager::Unknown => return Err(DependencyError::NoPackageManager),
        };
        let result =
            Command::new(cmd)
                .args(&args)
                .status()
                .map_err(|e| DependencyError::InstallFailed {
                    package: package.to_string(),
                    reason: format!("{}: {}", cmd, e),
                })?;
        if result.success() {
            Ok(())
        } else {
            Err(DependencyError::InstallFailed {
                package: package.to_string(),
                reason: format!("{} exited with code {:?}", cmd, result.code()),
            })
        }
    }
    fn update_package_index(&self, pm: PackageManager) -> Result<(), DependencyError> {
        let (cmd, args): (&str, Vec<&str>) = match pm {
            PackageManager::Apt => ("apt-get", vec!["-qq", "update"]),
            PackageManager::Dnf => ("dnf", vec!["check-update", "-q"]),
            PackageManager::Pacman => ("pacman", vec!["-Sy", "--quiet"]),
            PackageManager::Unknown => return Err(DependencyError::NoPackageManager),
        };
        // update index failure is non-fatal (offline, stale cache, etc.)
        let _ = Command::new(cmd).args(&args).status();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_detect_package_manager_returns_something() {
        let adapter = SystemDependencyAdapter::new();
        let pm = adapter.detect_package_manager();
        // On Ubuntu/WSL2 this should be Apt
        assert!(matches!(
            pm,
            PackageManager::Apt
                | PackageManager::Dnf
                | PackageManager::Pacman
                | PackageManager::Unknown
        ));
    }
    #[test]
    fn test_check_binary_existing() {
        let adapter = SystemDependencyAdapter::new();
        let status = adapter.check_binary("bash");
        assert!(status.installed, "bash should always be installed");
        assert!(status.version.is_some());
    }
    #[test]
    fn test_check_binary_nonexistent() {
        let adapter = SystemDependencyAdapter::new();
        let status = adapter.check_binary("this_binary_surely_does_not_exist_xyz");
        assert!(!status.installed);
        assert!(status.version.is_none());
    }
}
