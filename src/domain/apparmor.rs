/// Domain types for AppArmor sandboxing.
/// No external dependencies — pure business logic.
///
/// Tareas AA-001 (201): Domain types
///
/// Mode of an AppArmor profile
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppArmorMode {
    /// Only log violations, don't block
    Complain,
    /// Block and log violations
    Enforce,
    /// Profile loaded but inactive
    Disabled,
}

impl AppArmorMode {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            AppArmorMode::Complain => "complain",
            AppArmorMode::Enforce => "enforce",
            AppArmorMode::Disabled => "disabled",
        }
    }
}

impl std::fmt::Display for AppArmorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for AppArmorMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "complain" => Ok(AppArmorMode::Complain),
            "enforce" => Ok(AppArmorMode::Enforce),
            "disabled" | "disable" => Ok(AppArmorMode::Disabled),
            other => Err(format!(
                "Unknown AppArmor mode '{}'. Use: complain, enforce, disabled",
                other
            )),
        }
    }
}

/// Type of Enola service (determines which profile template to use)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppArmorServiceType {
    /// Git/Forgejo container
    Git,
    /// WordPress container
    WordPress,
    /// Nginx system service
    Nginx,
    /// Tor system service
    Tor,
    /// Base Docker profile
    DockerBase,
}

impl AppArmorServiceType {
    /// Profile name prefix for this service type
    pub(crate) fn profile_prefix(&self) -> &str {
        match self {
            AppArmorServiceType::Git => "enola-git",
            AppArmorServiceType::WordPress => "enola-wp",
            AppArmorServiceType::Nginx => "enola-nginx",
            AppArmorServiceType::Tor => "enola-tor",
            AppArmorServiceType::DockerBase => "enola-docker-base",
        }
    }

    /// Full profile name for a given service instance
    pub(crate) fn profile_name(&self, instance_name: &str) -> String {
        match self {
            AppArmorServiceType::Nginx
            | AppArmorServiceType::Tor
            | AppArmorServiceType::DockerBase => self.profile_prefix().to_string(),
            _ => format!("{}-{}", self.profile_prefix(), instance_name),
        }
    }

    /// Data path pattern for this service type (used in profile rules)
    #[allow(dead_code)] // Will be used when git/wp create integrates AppArmor (AA-011..013)
    pub(crate) fn data_path_pattern(&self, instance_name: &str) -> Option<String> {
        match self {
            AppArmorServiceType::Git => Some(format!("/srv/enola-git/{}/**", instance_name)),
            AppArmorServiceType::WordPress => {
                Some(format!("/srv/enola-wordpress/{}_wp/**", instance_name))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for AppArmorServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppArmorServiceType::Git => write!(f, "git"),
            AppArmorServiceType::WordPress => write!(f, "wordpress"),
            AppArmorServiceType::Nginx => write!(f, "nginx"),
            AppArmorServiceType::Tor => write!(f, "tor"),
            AppArmorServiceType::DockerBase => write!(f, "docker-base"),
        }
    }
}

/// A loaded AppArmor profile
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppArmorProfile {
    /// Profile name (e.g., "enola-git-myserver")
    pub name: String,
    /// Current mode
    pub mode: AppArmorMode,
    /// Service type
    pub service_type: AppArmorServiceType,
}

/// Violation logged by AppArmor
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppArmorViolation {
    /// Timestamp from syslog
    pub timestamp: String,
    /// Profile that triggered the violation
    pub profile: String,
    /// Operation that was denied/logged
    pub operation: String,
    /// Resource path (if applicable)
    pub path: Option<String>,
}

/// Overall AppArmor system status
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppArmorStatus {
    /// AppArmor is installed on the system
    pub installed: bool,
    /// AppArmor kernel module is enabled
    pub enabled: bool,
    /// Loaded Enola profiles
    pub profiles: Vec<AppArmorProfile>,
    /// Recent violations (last 24h)
    pub recent_violations: Vec<AppArmorViolation>,
}

impl AppArmorStatus {
    /// Whether AppArmor is fully operational
    pub fn is_operational(&self) -> bool {
        self.installed && self.enabled
    }

    /// Count of profiles in enforce mode
    pub fn enforce_count(&self) -> usize {
        self.profiles
            .iter()
            .filter(|p| p.mode == AppArmorMode::Enforce)
            .count()
    }

    /// Count of profiles in complain mode
    pub fn complain_count(&self) -> usize {
        self.profiles
            .iter()
            .filter(|p| p.mode == AppArmorMode::Complain)
            .count()
    }

    /// Summary string for status display
    pub fn summary(&self) -> String {
        if !self.installed {
            return "AppArmor: not installed".to_string();
        }
        if !self.enabled {
            return "AppArmor: installed but kernel module not enabled".to_string();
        }
        format!(
            "AppArmor: enabled | Profiles: {} ({} enforce, {} complain) | Violations (24h): {}",
            self.profiles.len(),
            self.enforce_count(),
            self.complain_count(),
            self.recent_violations.len()
        )
    }
}

/// Generate the AppArmor profile content for a service.
/// Pure function — no I/O, no side effects.
pub(crate) fn generate_profile_content(
    service_type: &AppArmorServiceType,
    instance_name: &str,
) -> String {
    let profile_name = service_type.profile_name(instance_name);

    match service_type {
        AppArmorServiceType::DockerBase => format!(
            r#"#include <tunables/global>

profile {name} flags=(attach_disconnected,mediate_deleted) {{
  #include <abstractions/base>

  # Networking: only TCP/UDP (no raw sockets)
  network tcp,
  network udp,
  deny network raw,

  # Deny mount (prevent container escape)
  deny mount,

  # Deny ptrace (prevent process inspection)
  deny ptrace,

  # Deny access to other Enola service data
  deny /srv/enola-*/** rwx,
  deny /etc/tor/** rwx,
  deny /home/** rwx,
  deny /root/** rwx,
}}
"#,
            name = profile_name
        ),

        AppArmorServiceType::Git => {
            let data_path = format!("/srv/enola-git/{}", instance_name);
            format!(
                r#"#include <tunables/global>

profile {name} flags=(attach_disconnected,mediate_deleted) {{
  #include <abstractions/base>
  #include <abstractions/nameservice>

  # Capabilities needed by Forgejo's su-exec to drop privileges
  capability chown,
  capability setuid,
  capability setgid,
  capability dac_override,

  # Access to OWN data directory
  {data}/** rw,
  {data}/ r,

  # Deny other Enola service data
  deny /srv/enola-wordpress/** rwx,
  deny /srv/enola-files/** rwx,
  deny /etc/tor/** rwx,

  network tcp,
  network udp,
  deny network raw,
  deny mount,
  deny ptrace,
}}
"#,
                name = profile_name,
                data = data_path
            )
        }

        AppArmorServiceType::WordPress => {
            let data_path = format!("/srv/enola-wordpress/{}_wp", instance_name);
            format!(
                r#"#include <tunables/global>

profile {name} flags=(attach_disconnected,mediate_deleted) {{
  #include <abstractions/base>

  # Capabilities needed by WordPress docker-entrypoint.sh
  capability chown,
  capability setuid,
  capability setgid,
  capability dac_override,

  # Access to OWN data directory
  {data}/** rw,
  {data}/ r,

  # Deny other Enola service data
  deny /srv/enola-git/** rwx,
  deny /etc/tor/** rwx,

  network tcp,
  network udp,
  deny network raw,
  deny mount,
  deny ptrace,
}}
"#,
                name = profile_name,
                data = data_path
            )
        }

        AppArmorServiceType::Nginx => format!(
            r#"#include <tunables/global>

profile {name} flags=(attach_disconnected) {{
  #include <abstractions/base>
  #include <abstractions/nameservice>

  /etc/nginx/** r,
  /var/log/nginx/** rw,
  /run/nginx.pid rw,
  /var/www/** r,

  # SSL certificates generated by Enola
  /etc/nginx/ssl/** r,

  # Deny explicit
  deny /srv/enola-*/** rwx,
  deny /etc/tor/** rwx,
  deny /home/** rwx,

  network tcp,
  deny network raw,
  deny mount,
  deny ptrace,
}}
"#,
            name = profile_name
        ),

        AppArmorServiceType::Tor => format!(
            r#"#include <tunables/global>

profile {name} flags=(attach_disconnected) {{
  #include <abstractions/base>
  #include <abstractions/nameservice>

  /etc/tor/** r,
  /var/lib/tor/** rw,
  /var/log/tor/** rw,
  /run/tor/** rw,

  deny /srv/enola-*/** rwx,
  deny /home/** rwx,
  deny /etc/nginx/** w,

  network tcp,
  deny network raw,
  deny mount,
  deny ptrace,
}}
"#,
            name = profile_name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_display() {
        assert_eq!(AppArmorMode::Complain.to_string(), "complain");
        assert_eq!(AppArmorMode::Enforce.to_string(), "enforce");
        assert_eq!(AppArmorMode::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_mode_from_str() {
        assert_eq!(
            "complain".parse::<AppArmorMode>().unwrap(),
            AppArmorMode::Complain
        );
        assert_eq!(
            "ENFORCE".parse::<AppArmorMode>().unwrap(),
            AppArmorMode::Enforce
        );
        assert_eq!(
            "disabled".parse::<AppArmorMode>().unwrap(),
            AppArmorMode::Disabled
        );
        assert_eq!(
            "disable".parse::<AppArmorMode>().unwrap(),
            AppArmorMode::Disabled
        );
        assert!("invalid".parse::<AppArmorMode>().is_err());
    }

    #[test]
    fn test_service_type_profile_name_with_instance() {
        assert_eq!(
            AppArmorServiceType::Git.profile_name("myserver"),
            "enola-git-myserver"
        );
        assert_eq!(
            AppArmorServiceType::WordPress.profile_name("myblog"),
            "enola-wp-myblog"
        );
    }

    #[test]
    fn test_service_type_profile_name_system_services() {
        assert_eq!(
            AppArmorServiceType::Nginx.profile_name("ignored"),
            "enola-nginx"
        );
        assert_eq!(
            AppArmorServiceType::Tor.profile_name("ignored"),
            "enola-tor"
        );
        assert_eq!(
            AppArmorServiceType::DockerBase.profile_name("ignored"),
            "enola-docker-base"
        );
    }

    #[test]
    fn test_data_path_pattern() {
        assert_eq!(
            AppArmorServiceType::Git.data_path_pattern("myserver"),
            Some("/srv/enola-git/myserver/**".to_string())
        );
        assert_eq!(
            AppArmorServiceType::WordPress.data_path_pattern("myblog"),
            Some("/srv/enola-wordpress/myblog_wp/**".to_string())
        );
        assert!(AppArmorServiceType::Nginx.data_path_pattern("x").is_none());
    }

    #[test]
    fn test_status_is_operational() {
        let status = AppArmorStatus {
            installed: true,
            enabled: true,
            profiles: vec![],
            recent_violations: vec![],
        };
        assert!(status.is_operational());
    }

    #[test]
    fn test_status_not_operational_when_not_installed() {
        let status = AppArmorStatus {
            installed: false,
            enabled: false,
            profiles: vec![],
            recent_violations: vec![],
        };
        assert!(!status.is_operational());
    }

    #[test]
    fn test_status_enforce_complain_counts() {
        let status = AppArmorStatus {
            installed: true,
            enabled: true,
            profiles: vec![
                AppArmorProfile {
                    name: "enola-nginx".to_string(),
                    mode: AppArmorMode::Enforce,
                    service_type: AppArmorServiceType::Nginx,
                },
                AppArmorProfile {
                    name: "enola-git-myserver".to_string(),
                    mode: AppArmorMode::Complain,
                    service_type: AppArmorServiceType::Git,
                },
                AppArmorProfile {
                    name: "enola-tor".to_string(),
                    mode: AppArmorMode::Enforce,
                    service_type: AppArmorServiceType::Tor,
                },
            ],
            recent_violations: vec![],
        };
        assert_eq!(status.enforce_count(), 2);
        assert_eq!(status.complain_count(), 1);
    }

    #[test]
    fn test_status_summary_not_installed() {
        let status = AppArmorStatus {
            installed: false,
            enabled: false,
            profiles: vec![],
            recent_violations: vec![],
        };
        assert!(status.summary().contains("not installed"));
    }

    #[test]
    fn test_status_summary_enabled() {
        let status = AppArmorStatus {
            installed: true,
            enabled: true,
            profiles: vec![],
            recent_violations: vec![],
        };
        let s = status.summary();
        assert!(s.contains("enabled"));
        assert!(s.contains("Profiles: 0"));
    }

    #[test]
    fn test_generate_profile_docker_base() {
        let content = generate_profile_content(&AppArmorServiceType::DockerBase, "");
        assert!(content.contains("profile enola-docker-base"));
        assert!(content.contains("deny mount"));
        assert!(content.contains("deny ptrace"));
        assert!(content.contains("deny network raw"));
    }

    #[test]
    fn test_generate_profile_git() {
        let content = generate_profile_content(&AppArmorServiceType::Git, "myserver");
        assert!(content.contains("profile enola-git-myserver"));
        assert!(content.contains("/srv/enola-git/myserver/**"));
        assert!(content.contains("deny /srv/enola-wordpress/**"));
    }

    #[test]
    fn test_generate_profile_wordpress() {
        let content = generate_profile_content(&AppArmorServiceType::WordPress, "myblog");
        assert!(content.contains("profile enola-wp-myblog"));
        assert!(content.contains("/srv/enola-wordpress/myblog_wp/**"));
        assert!(content.contains("deny /srv/enola-git/**"));
    }

    #[test]
    fn test_generate_profile_nginx() {
        let content = generate_profile_content(&AppArmorServiceType::Nginx, "");
        assert!(content.contains("profile enola-nginx"));
        assert!(content.contains("/etc/nginx/**"));
        assert!(content.contains("/var/log/nginx/**"));
    }

    #[test]
    fn test_generate_profile_tor() {
        let content = generate_profile_content(&AppArmorServiceType::Tor, "");
        assert!(content.contains("profile enola-tor"));
        assert!(content.contains("/etc/tor/**"));
        assert!(content.contains("/var/lib/tor/**"));
    }

    #[test]
    fn test_service_type_display() {
        assert_eq!(AppArmorServiceType::Git.to_string(), "git");
        assert_eq!(AppArmorServiceType::Nginx.to_string(), "nginx");
    }

    // ── Error-path tests ──

    #[test]
    fn test_mode_from_str_invalid() {
        let result = "invalid_mode".parse::<AppArmorMode>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown AppArmor mode"));
        assert!(err.contains("invalid_mode"));
    }

    #[test]
    fn test_mode_from_str_empty() {
        assert!("".parse::<AppArmorMode>().is_err());
    }

    #[test]
    fn test_is_operational_installed_but_not_enabled() {
        let status = AppArmorStatus {
            installed: true,
            enabled: false,
            profiles: vec![],
            recent_violations: vec![],
        };
        assert!(!status.is_operational());
    }

    #[test]
    fn test_summary_installed_but_not_enabled() {
        let status = AppArmorStatus {
            installed: true,
            enabled: false,
            profiles: vec![],
            recent_violations: vec![],
        };
        let s = status.summary();
        assert!(s.contains("installed"));
        assert!(s.contains("not enabled"));
    }

    #[test]
    fn test_generate_profile_git_empty_instance() {
        let content = generate_profile_content(&AppArmorServiceType::Git, "");
        assert!(content.contains("profile enola-git-"));
        assert!(content.contains("/srv/enola-git/"));
    }

    #[test]
    fn test_generate_profile_wordpress_empty_instance() {
        let content = generate_profile_content(&AppArmorServiceType::WordPress, "");
        assert!(content.contains("profile enola-wp-"));
        assert!(content.contains("/srv/enola-wordpress/_wp"));
    }

    #[test]
    fn test_enforce_count_zero_when_all_complain() {
        let status = AppArmorStatus {
            installed: true,
            enabled: true,
            profiles: vec![
                AppArmorProfile {
                    name: "a".to_string(),
                    mode: AppArmorMode::Complain,
                    service_type: AppArmorServiceType::Nginx,
                },
                AppArmorProfile {
                    name: "b".to_string(),
                    mode: AppArmorMode::Disabled,
                    service_type: AppArmorServiceType::Tor,
                },
            ],
            recent_violations: vec![],
        };
        assert_eq!(status.enforce_count(), 0);
        assert_eq!(status.complain_count(), 1);
    }

    #[test]
    fn test_summary_with_violations() {
        let status = AppArmorStatus {
            installed: true,
            enabled: true,
            profiles: vec![],
            recent_violations: vec![AppArmorViolation {
                timestamp: "2026-01-01".to_string(),
                profile: "enola-nginx".to_string(),
                operation: "deny".to_string(),
                path: Some("/etc/shadow".to_string()),
            }],
        };
        let s = status.summary();
        assert!(s.contains("Violations (24h): 1"));
    }
}
