/// Domain types for system dependency management (DEP-001..003)
///
/// Pure domain logic — no external dependencies.
/// Defines what dependencies Enola needs and their categories.
use std::fmt;
// ─────────────────────────────────────────────────────────────────────────────
// Dependency definition
// ─────────────────────────────────────────────────────────────────────────────
/// A system dependency that Enola CLI requires.
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    /// Package name for installation (e.g., "wireguard-tools")
    pub package: &'static str,
    /// Binary to check for existence (e.g., "wg")
    pub binary: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Which category this belongs to
    pub category: DepCategory,
    /// Which Enola features need this
    pub needed_by: &'static str,
}
/// Category of dependency — determines when it gets installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepCategory {
    /// Required for core functionality (Docker, Nginx, Tor, curl, OpenSSL)
    Core,
    /// Required only for VPN features
    Vpn,
    /// Required only for security hardening (UFW, AppArmor)
    Security,
}
impl fmt::Display for DepCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DepCategory::Core => write!(f, "core"),
            DepCategory::Vpn => write!(f, "vpn"),
            DepCategory::Security => write!(f, "security"),
        }
    }
}
/// Result of checking a single dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct DepStatus {
    pub dep: Dependency,
    pub installed: bool,
    pub version: Option<String>,
}
impl DepStatus {
    pub fn icon(&self) -> &'static str {
        if self.installed {
            "✅"
        } else {
            "❌"
        }
    }
}
/// Setup scope — what to install.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetupScope {
    /// Core dependencies only (default)
    Core,
    /// VPN dependencies
    Vpn,
    /// Security dependencies
    Security,
    /// Everything
    All,
}
impl fmt::Display for SetupScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupScope::Core => write!(f, "core"),
            SetupScope::Vpn => write!(f, "vpn"),
            SetupScope::Security => write!(f, "security"),
            SetupScope::All => write!(f, "all"),
        }
    }
}
/// Linux package manager detected on the system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PackageManager {
    Apt,    // Debian, Ubuntu
    Dnf,    // Fedora, RHEL
    Pacman, // Arch
    Unknown,
}
impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageManager::Apt => write!(f, "apt"),
            PackageManager::Dnf => write!(f, "dnf"),
            PackageManager::Pacman => write!(f, "pacman"),
            PackageManager::Unknown => write!(f, "unknown"),
        }
    }
}
// ─────────────────────────────────────────────────────────────────────────────
// Canonical dependency list — single source of truth
// ─────────────────────────────────────────────────────────────────────────────
/// All system dependencies Enola CLI may need.
pub const ALL_DEPENDENCIES: &[Dependency] = &[
    // ── Core ─────────────────────────────────────────────────────────────
    Dependency {
        package: "docker.io",
        binary: "docker",
        description: "Container runtime for services (Git, WordPress, CMS)",
        category: DepCategory::Core,
        needed_by: "git, wp, cms",
    },
    Dependency {
        package: "nginx",
        binary: "nginx",
        description: "Reverse proxy and SSL termination",
        category: DepCategory::Core,
        needed_by: "tor publish, ssl, fileserver",
    },
    Dependency {
        package: "tor",
        binary: "tor",
        description: "Hidden services (.onion addresses)",
        category: DepCategory::Core,
        needed_by: "tor, publish, expose",
    },
    Dependency {
        package: "curl",
        binary: "curl",
        description: "HTTP client for health checks and downloads",
        category: DepCategory::Core,
        needed_by: "health checks, downloads",
    },
    Dependency {
        package: "openssl",
        binary: "openssl",
        description: "SSL/TLS certificate generation",
        category: DepCategory::Core,
        needed_by: "ssl, certificates",
    },
    // ── VPN ──────────────────────────────────────────────────────────────
    Dependency {
        package: "wireguard-tools",
        binary: "wg",
        description: "WireGuard VPN userspace tools (wg, wg-quick)",
        category: DepCategory::Vpn,
        needed_by: "vpn create, vpn peer",
    },
    Dependency {
        package: "qrencode",
        binary: "qrencode",
        description: "QR code generator for VPN peer configs (mobile scanning)",
        category: DepCategory::Vpn,
        needed_by: "vpn peer add (QR display)",
    },
    Dependency {
        package: "socat",
        binary: "socat",
        description: "UDP-over-TCP bridge for WireGuard over Tor",
        category: DepCategory::Vpn,
        needed_by: "vpn create --tor",
    },
    // ── Security ─────────────────────────────────────────────────────────
    Dependency {
        package: "ufw",
        binary: "ufw",
        description: "Uncomplicated Firewall",
        category: DepCategory::Security,
        needed_by: "firewall setup",
    },
    Dependency {
        package: "apparmor",
        binary: "apparmor_parser",
        description: "Mandatory Access Control (sandboxing)",
        category: DepCategory::Security,
        needed_by: "apparmor setup",
    },
    Dependency {
        package: "apparmor-utils",
        binary: "aa-status",
        description: "AppArmor management utilities",
        category: DepCategory::Security,
        needed_by: "apparmor status/mode",
    },
];
/// Get dependencies matching a scope.
pub fn deps_for_scope(scope: SetupScope) -> Vec<&'static Dependency> {
    ALL_DEPENDENCIES
        .iter()
        .filter(|d| match scope {
            SetupScope::Core => d.category == DepCategory::Core,
            SetupScope::Vpn => d.category == DepCategory::Vpn,
            SetupScope::Security => d.category == DepCategory::Security,
            SetupScope::All => true,
        })
        .collect()
}
// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("No supported package manager found (need apt, dnf, or pacman)")]
    NoPackageManager,
    #[error("Failed to install {package}: {reason}")]
    InstallFailed { package: String, reason: String },
    #[error("System error: {0}")]
    SystemError(String),
}
// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_all_dependencies_have_required_fields() {
        for dep in ALL_DEPENDENCIES {
            assert!(!dep.package.is_empty(), "package empty for {}", dep.binary);
            assert!(!dep.binary.is_empty(), "binary empty for {}", dep.package);
            assert!(
                !dep.description.is_empty(),
                "description empty for {}",
                dep.package
            );
            assert!(
                !dep.needed_by.is_empty(),
                "needed_by empty for {}",
                dep.package
            );
        }
    }
    #[test]
    fn test_deps_for_scope_core() {
        let core = deps_for_scope(SetupScope::Core);
        assert!(core.iter().all(|d| d.category == DepCategory::Core));
        assert!(core.iter().any(|d| d.binary == "docker"));
        assert!(core.iter().any(|d| d.binary == "nginx"));
        assert!(core.iter().any(|d| d.binary == "tor"));
    }
    #[test]
    fn test_deps_for_scope_vpn() {
        let vpn = deps_for_scope(SetupScope::Vpn);
        assert_eq!(vpn.len(), 3);
        assert_eq!(vpn[0].binary, "wg");
        assert_eq!(vpn[1].binary, "qrencode");
        assert_eq!(vpn[2].binary, "socat");
    }
    #[test]
    fn test_deps_for_scope_security() {
        let sec = deps_for_scope(SetupScope::Security);
        assert!(sec.iter().all(|d| d.category == DepCategory::Security));
        assert!(sec.iter().any(|d| d.binary == "ufw"));
    }
    #[test]
    fn test_deps_for_scope_all() {
        let all = deps_for_scope(SetupScope::All);
        assert_eq!(all.len(), ALL_DEPENDENCIES.len());
    }
    #[test]
    fn test_dep_status_icon() {
        let dep = ALL_DEPENDENCIES[0].clone();
        let ok = DepStatus {
            dep: dep.clone(),
            installed: true,
            version: Some("1.0".into()),
        };
        assert_eq!(ok.icon(), "✅");
        let missing = DepStatus {
            dep,
            installed: false,
            version: None,
        };
        assert_eq!(missing.icon(), "❌");
    }
    #[test]
    fn test_dep_category_display() {
        assert_eq!(DepCategory::Core.to_string(), "core");
        assert_eq!(DepCategory::Vpn.to_string(), "vpn");
        assert_eq!(DepCategory::Security.to_string(), "security");
    }
    #[test]
    fn test_package_manager_display() {
        assert_eq!(PackageManager::Apt.to_string(), "apt");
        assert_eq!(PackageManager::Dnf.to_string(), "dnf");
        assert_eq!(PackageManager::Pacman.to_string(), "pacman");
    }
    #[test]
    fn test_dependency_error_display() {
        let e = DependencyError::NoPackageManager;
        assert!(e.to_string().contains("package manager"));
        let e2 = DependencyError::InstallFailed {
            package: "nginx".into(),
            reason: "exit 1".into(),
        };
        assert!(e2.to_string().contains("nginx"));
    }

    // ── Error-path tests ──

    #[test]
    fn test_package_manager_unknown_display() {
        assert_eq!(PackageManager::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_setup_scope_all_display() {
        assert_eq!(SetupScope::All.to_string(), "all");
    }

    #[test]
    fn test_dependency_error_system_error() {
        let e = DependencyError::SystemError("permission denied".to_string());
        assert!(e.to_string().contains("permission denied"));
        assert!(e.to_string().contains("System error"));
    }

    #[test]
    fn test_dependency_error_install_failed_contains_reason() {
        let e = DependencyError::InstallFailed {
            package: "wireguard-tools".into(),
            reason: "apt returned 100".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("wireguard-tools"));
        assert!(msg.contains("apt returned 100"));
    }

    #[test]
    fn test_deps_for_scope_core_excludes_vpn_and_security() {
        let core = deps_for_scope(SetupScope::Core);
        assert!(!core.iter().any(|d| d.category == DepCategory::Vpn));
        assert!(!core.iter().any(|d| d.category == DepCategory::Security));
    }

    #[test]
    fn test_deps_for_scope_vpn_excludes_core_and_security() {
        let vpn = deps_for_scope(SetupScope::Vpn);
        assert!(!vpn.iter().any(|d| d.category == DepCategory::Core));
        assert!(!vpn.iter().any(|d| d.category == DepCategory::Security));
    }

    #[test]
    fn test_dep_status_icon_not_installed() {
        let dep = ALL_DEPENDENCIES[0].clone();
        let status = DepStatus {
            dep,
            installed: false,
            version: None,
        };
        assert_eq!(status.icon(), "❌");
    }

    #[test]
    fn test_dep_status_with_version_when_not_installed() {
        let dep = ALL_DEPENDENCIES[1].clone();
        let status = DepStatus {
            dep,
            installed: false,
            version: Some("stale".to_string()),
        };
        assert_eq!(status.icon(), "❌");
        assert!(status.version.is_some());
    }

    #[test]
    fn test_all_dependencies_have_valid_categories() {
        for dep in ALL_DEPENDENCIES {
            assert!(
                dep.category == DepCategory::Core
                    || dep.category == DepCategory::Vpn
                    || dep.category == DepCategory::Security,
                "{} has invalid category",
                dep.package
            );
        }
    }
}
