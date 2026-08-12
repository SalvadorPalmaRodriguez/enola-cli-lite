use crate::domain::apparmor::{
    generate_profile_content, AppArmorMode, AppArmorServiceType, AppArmorStatus,
};
use crate::domain::error::EnolaError;
use crate::ports::apparmor::AppArmorPort;
/// Application layer — AppArmor orchestration use cases.
/// Tareas AA-004..007 (204-207).
///
/// Uses only AppArmorPort trait (dependency inversion).
/// NEVER imports AppArmorAdapter directly.
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, EnolaError>;

/// Orchestrates all AppArmor operations.
/// Entry point for all CLI commands related to AppArmor.
pub struct AppArmorManager {
    apparmor: Arc<dyn AppArmorPort>,
}

impl AppArmorManager {
    pub fn new(apparmor: Arc<dyn AppArmorPort>) -> Self {
        Self { apparmor }
    }

    /// Setup base Enola profiles (nginx, tor, docker-base).
    /// Called by `enola-cli security apparmor setup`.
    ///
    /// Loads the 3 system-level profiles. Per-service profiles
    /// are created automatically when services are created.
    pub fn setup_base_profiles(&self, mode: AppArmorMode) -> Result<String> {
        if !self.apparmor.is_installed() {
            return Err(EnolaError::InfrastructureError(
                "AppArmor is not installed. Install it with:\n  sudo apt install apparmor apparmor-utils".to_string()
            ));
        }

        let enabled = self.apparmor.is_enabled()?;
        if !enabled {
            return Err(EnolaError::InfrastructureError(
                "AppArmor kernel module is not enabled.\n\
                 On WSL2: the standard kernel does not include AppArmor.\n\
                 On native Linux: check /sys/module/apparmor/parameters/enabled"
                    .to_string(),
            ));
        }

        let base_services = [
            AppArmorServiceType::Nginx,
            AppArmorServiceType::Tor,
            AppArmorServiceType::DockerBase,
        ];

        let mut results = Vec::new();

        for svc_type in &base_services {
            let profile_name = svc_type.profile_name("");
            let content = generate_profile_content(svc_type, "");

            self.apparmor
                .load_profile(&profile_name, &content, mode.clone())?;
            results.push(format!("  ✅ {} loaded ({})", profile_name, mode));
        }

        Ok(format!(
            "🛡️  AppArmor Setup Complete\n\
             ─────────────────────────────────\n\
             {}\n\
             \n\
             Per-service profiles will be created automatically\n\
             when you create services with git/wp create.\n\
             \n\
             To change mode later:\n\
             sudo enola-cli security apparmor mode --enforce\n\
             sudo enola-cli security apparmor mode --complain",
            results.join("\n")
        ))
    }

    /// Create and load an AppArmor profile for a specific service instance.
    /// Called automatically during git/wp create when AppArmor is active.
    pub fn apply_to_service(
        &self,
        service_type: &AppArmorServiceType,
        instance_name: &str,
        mode: AppArmorMode,
    ) -> Result<String> {
        let enabled = self.apparmor.is_enabled().unwrap_or(false);
        if !enabled {
            // AppArmor not available — skip silently (just a warning)
            return Ok(String::new());
        }

        let profile_name = service_type.profile_name(instance_name);
        let content = generate_profile_content(service_type, instance_name);

        self.apparmor
            .load_profile(&profile_name, &content, mode.clone())?;

        Ok(format!(
            "🛡️ AppArmor: profile '{}' loaded ({})",
            profile_name, mode
        ))
    }

    /// Remove the AppArmor profile for a service instance.
    /// Called automatically during git/wp delete.
    pub fn remove_from_service(
        &self,
        service_type: &AppArmorServiceType,
        instance_name: &str,
    ) -> Result<()> {
        let enabled = self.apparmor.is_enabled().unwrap_or(false);
        if !enabled {
            return Ok(());
        }

        let profile_name = service_type.profile_name(instance_name);
        self.apparmor.unload_profile(&profile_name)
    }

    /// Get status of all AppArmor Enola profiles.
    pub fn get_status(&self) -> Result<AppArmorStatus> {
        self.apparmor.status()
    }

    /// Change mode for a specific profile or all Enola profiles.
    pub fn change_mode(&self, mode: AppArmorMode, profile_name: Option<&str>) -> Result<String> {
        if let Some(name) = profile_name {
            self.apparmor.set_mode(name, mode.clone())?;
            Ok(format!("✅ Profile '{}' set to {}", name, mode))
        } else {
            // Change ALL Enola profiles
            let status = self.apparmor.status()?;
            if status.profiles.is_empty() {
                return Ok(
                    "No Enola AppArmor profiles loaded. Run 'security apparmor setup' first."
                        .to_string(),
                );
            }

            let mut count = 0;
            for profile in &status.profiles {
                self.apparmor.set_mode(&profile.name, mode.clone())?;
                count += 1;
            }

            Ok(format!("✅ {} profiles set to {}", count, mode))
        }
    }

    /// Get the Docker --security-opt value for a service, if AppArmor is active.
    /// Returns None if AppArmor is not available.
    pub fn docker_security_opt(
        &self,
        service_type: &AppArmorServiceType,
        instance_name: &str,
    ) -> Option<String> {
        let profile_name = service_type.profile_name(instance_name);
        self.apparmor.docker_security_opt(&profile_name)
    }

    /// Returns a warning message if AppArmor is not active.
    /// Non-blocking — used during service creation.
    pub fn inactive_warning(&self) -> Option<String> {
        if !self.apparmor.is_installed() {
            return Some(
                "⚠️  AppArmor is not installed. For enhanced security:\n  \
                 sudo apt install apparmor apparmor-utils\n  \
                 sudo enola-cli security apparmor setup"
                    .to_string(),
            );
        }
        match self.apparmor.is_enabled() {
            Ok(true) => None,
            Ok(false) => Some(
                "⚠️  AppArmor kernel module is not enabled.\n  \
                 Sandboxing is not active. See AppArmor documentation"
                    .to_string(),
            ),
            Err(_) => None, // Can't check — don't bother the user
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::apparmor::MockAppArmorPort;

    fn make_manager(mock: MockAppArmorPort) -> AppArmorManager {
        AppArmorManager::new(Arc::new(mock))
    }

    #[test]
    fn test_setup_not_installed() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| false);
        let mgr = make_manager(mock);
        let result = mgr.setup_base_profiles(AppArmorMode::Complain);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn test_setup_not_enabled() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_enabled().returning(|| Ok(false));
        let mgr = make_manager(mock);
        let result = mgr.setup_base_profiles(AppArmorMode::Complain);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not enabled"));
    }

    #[test]
    fn test_setup_success_loads_3_profiles() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_enabled().returning(|| Ok(true));
        mock.expect_load_profile()
            .times(3)
            .returning(|_, _, _| Ok(()));
        let mgr = make_manager(mock);
        let result = mgr.setup_base_profiles(AppArmorMode::Complain);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("enola-nginx"));
        assert!(msg.contains("enola-tor"));
        assert!(msg.contains("enola-docker-base"));
    }

    #[test]
    fn test_apply_to_service_apparmor_disabled_returns_empty() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_enabled().returning(|| Ok(false));
        let mgr = make_manager(mock);
        let result =
            mgr.apply_to_service(&AppArmorServiceType::Git, "test", AppArmorMode::Complain);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_apply_to_service_success() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_enabled().returning(|| Ok(true));
        mock.expect_load_profile()
            .withf(|name, content, mode| {
                name == "enola-git-myserver"
                    && content.contains("/srv/enola-git/myserver")
                    && *mode == AppArmorMode::Enforce
            })
            .returning(|_, _, _| Ok(()));
        let mgr = make_manager(mock);
        let result =
            mgr.apply_to_service(&AppArmorServiceType::Git, "myserver", AppArmorMode::Enforce);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("enola-git-myserver"));
    }

    #[test]
    fn test_remove_from_service_apparmor_disabled() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_enabled().returning(|| Ok(false));
        let mgr = make_manager(mock);
        let result = mgr.remove_from_service(&AppArmorServiceType::Git, "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_from_service_success() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_enabled().returning(|| Ok(true));
        mock.expect_unload_profile()
            .withf(|name| name == "enola-wp-myblog")
            .returning(|_| Ok(()));
        let mgr = make_manager(mock);
        let result = mgr.remove_from_service(&AppArmorServiceType::WordPress, "myblog");
        assert!(result.is_ok());
    }

    #[test]
    fn test_change_mode_single_profile() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_set_mode()
            .withf(|name, mode| name == "enola-nginx" && *mode == AppArmorMode::Enforce)
            .returning(|_, _| Ok(()));
        let mgr = make_manager(mock);
        let result = mgr.change_mode(AppArmorMode::Enforce, Some("enola-nginx"));
        assert!(result.is_ok());
        assert!(result.unwrap().contains("enola-nginx"));
    }

    #[test]
    fn test_change_mode_all_profiles() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_status().returning(|| {
            Ok(AppArmorStatus {
                installed: true,
                enabled: true,
                profiles: vec![
                    crate::domain::apparmor::AppArmorProfile {
                        name: "enola-nginx".to_string(),
                        mode: AppArmorMode::Complain,
                        service_type: AppArmorServiceType::Nginx,
                    },
                    crate::domain::apparmor::AppArmorProfile {
                        name: "enola-tor".to_string(),
                        mode: AppArmorMode::Complain,
                        service_type: AppArmorServiceType::Tor,
                    },
                ],
                recent_violations: vec![],
            })
        });
        mock.expect_set_mode().times(2).returning(|_, _| Ok(()));
        let mgr = make_manager(mock);
        let result = mgr.change_mode(AppArmorMode::Enforce, None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("2 profiles"));
    }

    #[test]
    fn test_change_mode_no_profiles_loaded() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_status().returning(|| {
            Ok(AppArmorStatus {
                installed: true,
                enabled: true,
                profiles: vec![],
                recent_violations: vec![],
            })
        });
        let mgr = make_manager(mock);
        let result = mgr.change_mode(AppArmorMode::Enforce, None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("No Enola AppArmor profiles"));
    }

    #[test]
    fn test_docker_security_opt() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_docker_security_opt()
            .withf(|name| name == "enola-git-myserver")
            .returning(|name| Some(format!("apparmor={}", name)));
        let mgr = make_manager(mock);
        let opt = mgr.docker_security_opt(&AppArmorServiceType::Git, "myserver");
        assert_eq!(opt, Some("apparmor=enola-git-myserver".to_string()));
    }

    #[test]
    fn test_inactive_warning_not_installed() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| false);
        let mgr = make_manager(mock);
        let warning = mgr.inactive_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("not installed"));
    }

    #[test]
    fn test_inactive_warning_not_enabled() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_enabled().returning(|| Ok(false));
        let mgr = make_manager(mock);
        let warning = mgr.inactive_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("not enabled"));
    }

    #[test]
    fn test_inactive_warning_active() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_enabled().returning(|| Ok(true));
        let mgr = make_manager(mock);
        assert!(mgr.inactive_warning().is_none());
    }

    #[test]
    fn test_get_status() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_status().returning(|| {
            Ok(AppArmorStatus {
                installed: true,
                enabled: true,
                profiles: vec![],
                recent_violations: vec![],
            })
        });
        let mgr = make_manager(mock);
        let status = mgr.get_status().unwrap();
        assert!(status.is_operational());
    }
}
