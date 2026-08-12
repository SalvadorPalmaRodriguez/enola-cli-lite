use crate::domain::dependencies::{
    deps_for_scope, DepStatus, DependencyError, PackageManager, SetupScope,
};
use crate::ports::dependencies::DependencyPort;
/// Dependency Manager — orchestrates setup/doctor commands (DEP-001..003)
///
/// Depends only on DependencyPort trait (hexagonal architecture).
use std::sync::Arc;
pub struct DependencyManager {
    dep: Arc<dyn DependencyPort>,
}
impl DependencyManager {
    pub fn new(dep: Arc<dyn DependencyPort>) -> Self {
        Self { dep }
    }
    /// Doctor: check all dependencies and format a report.
    pub fn doctor(&self) -> String {
        let all_deps: Vec<_> = crate::domain::dependencies::ALL_DEPENDENCIES
            .iter()
            .collect();
        let statuses = self.dep.check_all(&all_deps);
        let pm = self.dep.detect_package_manager();
        let installed = statuses.iter().filter(|s| s.installed).count();
        let missing = statuses.iter().filter(|s| !s.installed).count();
        let mut out = String::from("🩺 Enola CLI — System Dependencies\n");
        out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        out.push_str(&format!("Package manager: {}\n\n", pm));
        let mut current_cat = None;
        for s in &statuses {
            let cat = s.dep.category;
            if current_cat != Some(cat) {
                current_cat = Some(cat);
                out.push_str(&format!("── {} ──\n", cat));
            }
            let ver = s.version.as_deref().unwrap_or("—");
            out.push_str(&format!(
                "  {} {:<20} {:<50} {}\n",
                s.icon(),
                s.dep.binary,
                s.dep.description,
                ver
            ));
        }
        out.push_str("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        out.push_str(&format!(
            "  ✅ Installed: {}  ❌ Missing: {}\n",
            installed, missing
        ));
        if missing > 0 {
            out.push_str("\n💡 Install missing dependencies with:\n");
            out.push_str("   sudo enola-cli setup --all\n");
        } else {
            out.push_str("\n✅ All dependencies are installed!\n");
        }
        out
    }
    /// Setup: install missing dependencies for the given scope.
    /// Returns (installed_count, already_ok_count, errors).
    pub fn setup(&self, scope: SetupScope) -> Result<SetupResult, DependencyError> {
        let pm = self.dep.detect_package_manager();
        if pm == PackageManager::Unknown {
            return Err(DependencyError::NoPackageManager);
        }
        let target_deps = deps_for_scope(scope);
        let statuses = self.dep.check_all(&target_deps);
        let missing: Vec<&DepStatus> = statuses.iter().filter(|s| !s.installed).collect();
        let already_ok = statuses.len() - missing.len();
        if missing.is_empty() {
            return Ok(SetupResult {
                installed: 0,
                already_ok,
                failed: vec![],
                scope,
            });
        }
        // Update package index once before installing
        let _ = self.dep.update_package_index(pm);
        let mut installed_count = 0;
        let mut failed = Vec::new();
        for s in &missing {
            match self.dep.install_package(s.dep.package, pm) {
                Ok(()) => installed_count += 1,
                Err(e) => failed.push((s.dep.package.to_string(), e.to_string())),
            }
        }
        Ok(SetupResult {
            installed: installed_count,
            already_ok,
            failed,
            scope,
        })
    }
    /// Format a setup result for CLI output.
    pub fn format_setup_result(&self, r: &SetupResult) -> String {
        let mut out = format!(
            "⚙️  Setup complete (scope: {})\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
             ✅ Already installed: {}\n\
             📦 Newly installed:   {}\n",
            r.scope, r.already_ok, r.installed
        );
        if !r.failed.is_empty() {
            out.push_str(&format!("❌ Failed:            {}\n", r.failed.len()));
            for (pkg, reason) in &r.failed {
                out.push_str(&format!("   • {} — {}\n", pkg, reason));
            }
        }
        if r.installed > 0 && r.failed.is_empty() {
            out.push_str("\n✅ All requested dependencies installed successfully!\n");
        }
        out
    }
}
/// Result of a setup operation.
pub struct SetupResult {
    pub installed: usize,
    pub already_ok: usize,
    pub failed: Vec<(String, String)>,
    pub scope: SetupScope,
}
// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::dependencies::{DepCategory, Dependency};
    struct MockDep {
        installed_binaries: Vec<String>,
    }
    impl DependencyPort for MockDep {
        fn detect_package_manager(&self) -> PackageManager {
            PackageManager::Apt
        }
        fn check_binary(&self, binary: &str) -> DepStatus {
            let found = self.installed_binaries.iter().any(|b| b == binary);
            DepStatus {
                dep: Dependency {
                    package: "mock-pkg",
                    binary: "mock",
                    description: "mock",
                    category: DepCategory::Core,
                    needed_by: "test",
                },
                installed: found,
                version: if found { Some("1.0".into()) } else { None },
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
    fn test_doctor_all_installed() {
        let mock = MockDep {
            installed_binaries: vec![
                "docker".into(),
                "nginx".into(),
                "tor".into(),
                "curl".into(),
                "openssl".into(),
                "wg".into(),
                "ufw".into(),
                "apparmor_parser".into(),
                "aa-status".into(),
            ],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let report = mgr.doctor();
        assert!(report.contains("All dependencies are installed"));
        assert!(report.contains("Missing: 0"));
    }
    #[test]
    fn test_doctor_some_missing() {
        let mock = MockDep {
            installed_binaries: vec!["docker".into(), "curl".into()],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let report = mgr.doctor();
        assert!(report.contains("Missing:"));
        assert!(report.contains("setup --all"));
    }
    #[test]
    fn test_setup_nothing_to_install() {
        let mock = MockDep {
            installed_binaries: vec![
                "docker".into(),
                "nginx".into(),
                "tor".into(),
                "curl".into(),
                "openssl".into(),
            ],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let result = mgr.setup(SetupScope::Core).unwrap();
        assert_eq!(result.installed, 0);
        assert_eq!(result.already_ok, 5);
    }
    #[test]
    fn test_setup_installs_missing() {
        let mock = MockDep {
            installed_binaries: vec!["docker".into(), "curl".into()],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let result = mgr.setup(SetupScope::Core).unwrap();
        assert_eq!(result.installed, 3); // nginx, tor, openssl
        assert_eq!(result.already_ok, 2); // docker, curl
    }
    #[test]
    fn test_setup_vpn_scope() {
        let mock = MockDep {
            installed_binaries: vec![],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let result = mgr.setup(SetupScope::Vpn).unwrap();
        assert_eq!(result.installed, 1); // wg
    }
    #[test]
    fn test_format_setup_result() {
        let mock = MockDep {
            installed_binaries: vec![],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let r = SetupResult {
            installed: 3,
            already_ok: 2,
            failed: vec![],
            scope: SetupScope::Core,
        };
        let out = mgr.format_setup_result(&r);
        assert!(out.contains("Newly installed:   3"));
        assert!(out.contains("Already installed: 2"));
        assert!(out.contains("successfully"));
    }
    #[test]
    fn test_format_setup_result_with_failures() {
        let mock = MockDep {
            installed_binaries: vec![],
        };
        let mgr = DependencyManager::new(Arc::new(mock));
        let r = SetupResult {
            installed: 1,
            already_ok: 0,
            failed: vec![("tor".into(), "exit 1".into())],
            scope: SetupScope::All,
        };
        let out = mgr.format_setup_result(&r);
        assert!(out.contains("Failed:"));
        assert!(out.contains("tor"));
    }
    struct NoPackageManagerMock;
    impl DependencyPort for NoPackageManagerMock {
        fn detect_package_manager(&self) -> PackageManager {
            PackageManager::Unknown
        }
        fn check_binary(&self, _b: &str) -> DepStatus {
            DepStatus {
                dep: Dependency {
                    package: "",
                    binary: "",
                    description: "",
                    category: DepCategory::Core,
                    needed_by: "",
                },
                installed: false,
                version: None,
            }
        }
        fn install_package(&self, _p: &str, _pm: PackageManager) -> Result<(), DependencyError> {
            Err(DependencyError::NoPackageManager)
        }
        fn update_package_index(&self, _pm: PackageManager) -> Result<(), DependencyError> {
            Err(DependencyError::NoPackageManager)
        }
    }
    #[test]
    fn test_setup_no_package_manager() {
        let mgr = DependencyManager::new(Arc::new(NoPackageManagerMock));
        let result = mgr.setup(SetupScope::All);
        assert!(result.is_err());
    }
}
