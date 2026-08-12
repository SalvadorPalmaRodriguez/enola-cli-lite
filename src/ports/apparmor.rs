/// Trait inyectable para gestión de AppArmor sandboxing.
/// Mockeable con mockall. Tarea AA-002 (202).
///
/// Implementación concreta: `src/adapters/infra/apparmor.rs`
use crate::domain::apparmor::{AppArmorMode, AppArmorStatus};
use crate::domain::error::EnolaError;

pub type Result<T> = std::result::Result<T, EnolaError>;

#[cfg_attr(test, mockall::automock)]
pub trait AppArmorPort: Send + Sync {
    /// Whether apparmor_parser and aa-status are installed
    fn is_installed(&self) -> bool;

    /// Whether the AppArmor kernel module is enabled
    fn is_enabled(&self) -> Result<bool>;

    /// Load a profile into the kernel.
    /// `profile_name`: e.g. "enola-git-myserver"
    /// `profile_content`: full AppArmor profile text
    /// `mode`: complain or enforce
    fn load_profile(
        &self,
        profile_name: &str,
        profile_content: &str,
        mode: AppArmorMode,
    ) -> Result<()>;

    /// Unload a profile from the kernel
    fn unload_profile(&self, profile_name: &str) -> Result<()>;

    /// Change mode of an already-loaded profile
    fn set_mode(&self, profile_name: &str, mode: AppArmorMode) -> Result<()>;

    /// Get overall AppArmor status (profiles, violations)
    fn status(&self) -> Result<AppArmorStatus>;

    /// Generate the Docker `--security-opt` value for a service.
    /// Returns `Some("apparmor=enola-git-myserver")` if the profile is loaded,
    /// `None` if AppArmor is not available or profile not loaded.
    fn docker_security_opt(&self, profile_name: &str) -> Option<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::apparmor::{AppArmorMode, AppArmorStatus};

    #[test]
    fn test_mock_is_installed() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_installed().returning(|| true);
        assert!(mock.is_installed());
    }

    #[test]
    fn test_mock_is_enabled() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_is_enabled().returning(|| Ok(true));
        assert!(mock.is_enabled().unwrap());
    }

    #[test]
    fn test_mock_load_profile() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_load_profile()
            .withf(|name, _content, mode| {
                name == "enola-git-test" && *mode == AppArmorMode::Complain
            })
            .returning(|_, _, _| Ok(()));
        assert!(mock
            .load_profile("enola-git-test", "content", AppArmorMode::Complain)
            .is_ok());
    }

    #[test]
    fn test_mock_unload_profile() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_unload_profile()
            .withf(|name| name == "enola-git-test")
            .returning(|_| Ok(()));
        assert!(mock.unload_profile("enola-git-test").is_ok());
    }

    #[test]
    fn test_mock_set_mode() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_set_mode()
            .withf(|name, mode| name == "enola-nginx" && *mode == AppArmorMode::Enforce)
            .returning(|_, _| Ok(()));
        assert!(mock.set_mode("enola-nginx", AppArmorMode::Enforce).is_ok());
    }

    #[test]
    fn test_mock_status() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_status().returning(|| {
            Ok(AppArmorStatus {
                installed: true,
                enabled: true,
                profiles: vec![],
                recent_violations: vec![],
            })
        });
        let s = mock.status().unwrap();
        assert!(s.is_operational());
    }

    #[test]
    fn test_mock_docker_security_opt_some() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_docker_security_opt()
            .withf(|name| name == "enola-git-test")
            .returning(|name| Some(format!("apparmor={}", name)));
        assert_eq!(
            mock.docker_security_opt("enola-git-test"),
            Some("apparmor=enola-git-test".to_string())
        );
    }

    #[test]
    fn test_mock_docker_security_opt_none() {
        let mut mock = MockAppArmorPort::new();
        mock.expect_docker_security_opt().returning(|_| None);
        assert_eq!(mock.docker_security_opt("enola-git-test"), None);
    }
}
