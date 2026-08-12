/// DependencyPort — injectable trait for system dependency operations (DEP-001..003)
///
/// Synchronous trait (for mockall compatibility).
use crate::domain::dependencies::{DepStatus, Dependency, DependencyError, PackageManager};
/// Abstraction over system package management.
pub trait DependencyPort: Send + Sync {
    /// Detect which package manager is available on this system.
    fn detect_package_manager(&self) -> PackageManager;
    /// Check if a binary exists in PATH and optionally get its version.
    fn check_binary(&self, binary: &str) -> DepStatus;
    /// Install a package using the system package manager.
    /// Requires root privileges.
    fn install_package(&self, package: &str, pm: PackageManager) -> Result<(), DependencyError>;
    /// Run `apt-get update` / `dnf check-update` / `pacman -Sy` before installing.
    fn update_package_index(&self, pm: PackageManager) -> Result<(), DependencyError>;
    /// Check all dependencies and return their status.
    fn check_all(&self, deps: &[&Dependency]) -> Vec<DepStatus> {
        deps.iter()
            .map(|d| {
                let mut status = self.check_binary(d.binary);
                status.dep = (*d).clone();
                status
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::dependencies::{DepCategory, ALL_DEPENDENCIES};
    struct MockDep {
        all_installed: bool,
    }
    impl DependencyPort for MockDep {
        fn detect_package_manager(&self) -> PackageManager {
            PackageManager::Apt
        }
        fn check_binary(&self, _binary: &str) -> DepStatus {
            DepStatus {
                dep: Dependency {
                    package: "mock",
                    binary: "mock",
                    description: "mock",
                    category: DepCategory::Core,
                    needed_by: "test",
                },
                installed: self.all_installed,
                version: if self.all_installed {
                    Some("1.0".into())
                } else {
                    None
                },
            }
        }
        fn install_package(&self, _pkg: &str, _pm: PackageManager) -> Result<(), DependencyError> {
            Ok(())
        }
        fn update_package_index(&self, _pm: PackageManager) -> Result<(), DependencyError> {
            Ok(())
        }
    }
    #[test]
    fn test_mock_check_all() {
        let mock = MockDep {
            all_installed: true,
        };
        let deps: Vec<&Dependency> = ALL_DEPENDENCIES.iter().collect();
        let results = mock.check_all(&deps);
        assert_eq!(results.len(), ALL_DEPENDENCIES.len());
        assert!(results.iter().all(|s| s.installed));
    }
    #[test]
    fn test_mock_not_installed() {
        let mock = MockDep {
            all_installed: false,
        };
        let status = mock.check_binary("docker");
        assert!(!status.installed);
        assert!(status.version.is_none());
    }
}
