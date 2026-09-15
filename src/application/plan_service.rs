//! Application use case: `enola plan` — dry-run service planning (Tarea 3).
//!
//! Computes what WOULD happen if a `create` command were executed, WITHOUT
//! any side effects: no Docker, no UFW, no AppArmor, no file writes.
//!
//! Reuses `PortValidator` (application/port_validator.rs) for port resolution
//! — does NOT drop down to the raw `PortCheckerPort` trait.

use crate::application::port_validator::{PortRanges, PortValidator};
use crate::domain::error::Result;
use crate::domain::naming;
use crate::domain::plan::{blueprint_for, PlanKind, ResolvedPort, ServicePlan};
use crate::ports::port_checker::PortCheckerPort;
use std::sync::Arc;

/// Dry-run planner: resolves ports and builds a `ServicePlan` with 0 side effects.
///
/// Guaranteed by construction to have no side effects: it only depends on
/// `PortValidator` (which only READS ports via `PortCheckerPort`) and the
/// pure `domain::plan` module. It has no access to `ContainerPort`,
/// `FirewallPort`, or `AppArmorManager`.
pub struct PlanService {
    port_validator: PortValidator,
}

impl PlanService {
    pub fn new(checker: Arc<dyn PortCheckerPort>) -> Self {
        Self {
            port_validator: PortValidator::new(checker),
        }
    }

    /// Plan a WordPress site creation.
    pub fn plan_wordpress(&self, name: &str, http_port: Option<u16>) -> Result<ServicePlan> {
        naming::validate_service_name(name)?;
        let resolved_port = self.port_validator.resolve_port(
            http_port,
            PortRanges::WORDPRESS_BACKEND,
            "http-port",
        )?;
        let auto = http_port.is_none();
        let resolved = vec![ResolvedPort {
            label: "http-port",
            port: resolved_port,
            auto_assigned: auto,
        }];
        let blueprint = blueprint_for(PlanKind::WordPress);
        Ok(blueprint.build(name, &resolved))
    }

    /// Plan a Git server creation.
    pub fn plan_git(
        &self,
        name: &str,
        http_port: Option<u16>,
        ssh_port: Option<u16>,
    ) -> Result<ServicePlan> {
        naming::validate_service_name(name)?;
        let resolved_http =
            self.port_validator
                .resolve_port(http_port, PortRanges::GIT_HTTP, "http-port")?;
        let resolved_ssh =
            self.port_validator
                .resolve_port(ssh_port, PortRanges::GIT_SSH, "ssh-port")?;
        let resolved = vec![
            ResolvedPort {
                label: "http-port",
                port: resolved_http,
                auto_assigned: http_port.is_none(),
            },
            ResolvedPort {
                label: "ssh-port",
                port: resolved_ssh,
                auto_assigned: ssh_port.is_none(),
            },
        ];
        let blueprint = blueprint_for(PlanKind::Git);
        Ok(blueprint.build(name, &resolved))
    }

    /// Plan a Tor hidden service creation.
    ///
    /// `virtual_port` is the public .onion port (not a real socket — not validated).
    /// `target_port` is the local app port (validated/auto-assigned in NGINX_LISTEN range).
    pub fn plan_tor(
        &self,
        name: &str,
        _service_type: &str,
        virtual_port: u16,
        target_port: Option<u16>,
    ) -> Result<ServicePlan> {
        naming::validate_service_name(name)?;
        let resolved_target = self.port_validator.resolve_port(
            target_port,
            PortRanges::NGINX_LISTEN,
            "target-port",
        )?;
        let resolved = vec![
            ResolvedPort {
                label: "virtual-port",
                port: virtual_port,
                auto_assigned: false, // always manual (default 80)
            },
            ResolvedPort {
                label: "target-port",
                port: resolved_target,
                auto_assigned: target_port.is_none(),
            },
        ];
        let blueprint = blueprint_for(PlanKind::Tor);
        Ok(blueprint.build(name, &resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::port_checker::{MockPortCheckerPort, PortCheckResult};

    fn free_result(port: u16) -> PortCheckResult {
        PortCheckResult {
            port,
            free_os: true,
            free_docker: true,
        }
    }

    fn free_checker() -> MockPortCheckerPort {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(free_result(p)));
        mock.expect_find_free_port().returning(|start, _| Ok(start));
        mock
    }

    // ── WordPress ──

    #[test]
    fn test_plan_wp_resolves_auto_port() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_wordpress("foo", None).unwrap();
        assert_eq!(plan.service_name, "foo");
        assert_eq!(plan.ports.len(), 1);
        assert_eq!(plan.ports[0].label, "http-port");
        // auto-assigned → first free in range (8080)
        assert_eq!(plan.ports[0].port, 8080);
        assert!(plan.ports[0].source.contains("auto-assigned"));
    }

    #[test]
    fn test_plan_wp_manual_port_validated() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_wordpress("foo", Some(8090)).unwrap();
        assert_eq!(plan.ports[0].port, 8090);
        assert_eq!(plan.ports[0].source, "manual");
    }

    #[test]
    fn test_plan_wp_risk_low() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_wordpress("foo", Some(8090)).unwrap();
        assert_eq!(plan.risk, crate::domain::plan::RiskLevel::Low);
    }

    // ── Git ──

    #[test]
    fn test_plan_git_resolves_two_ports() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_git("repo", Some(10500), Some(30100)).unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert_eq!(plan.ports[0].port, 10500);
        assert_eq!(plan.ports[1].port, 30100);
    }

    #[test]
    fn test_plan_git_risk_medium_ssh() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_git("repo", None, None).unwrap();
        assert_eq!(plan.risk, crate::domain::plan::RiskLevel::Medium);
    }

    // ── Tor ──

    #[test]
    fn test_plan_tor_virtual_port_not_validated() {
        // virtual_port 80 is .onion — should NOT trigger privileged-port rejection.
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(15000)).unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert_eq!(plan.ports[0].label, "virtual-port");
        assert_eq!(plan.ports[0].port, 80);
        assert_eq!(plan.ports[1].label, "target-port");
        assert_eq!(plan.ports[1].port, 15000);
    }

    #[test]
    fn test_plan_tor_no_containers() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(15000)).unwrap();
        assert!(plan.containers.is_empty());
    }

    #[test]
    fn test_plan_tor_risk_low() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(15000)).unwrap();
        assert_eq!(plan.risk, crate::domain::plan::RiskLevel::Low);
    }

    // ── Validation ──

    #[test]
    fn test_plan_invalid_name_rejected() {
        let svc = PlanService::new(Arc::new(free_checker()));
        assert!(svc.plan_wordpress("Bad;Name", None).is_err());
        assert!(svc.plan_git("Bad;Name", None, None).is_err());
        assert!(svc.plan_tor("Bad;Name", "web", 80, None).is_err());
    }

    #[test]
    fn test_plan_empty_name_rejected() {
        let svc = PlanService::new(Arc::new(free_checker()));
        assert!(svc.plan_wordpress("", None).is_err());
    }

    #[test]
    fn test_plan_busy_port_rejected() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| {
            Ok(PortCheckResult {
                port: p,
                free_os: false,
                free_docker: true,
            })
        });
        let svc = PlanService::new(Arc::new(mock));
        assert!(svc.plan_wordpress("foo", Some(8080)).is_err());
    }

    // ── Zero side effects (by construction) ──

    #[test]
    fn test_plan_zero_side_effects() {
        // PlanService has no ContainerPort, FirewallPort, or AppArmorManager.
        // The only dependency is PortValidator (read-only port checks).
        // This test exists to document and guard that invariant: if someone
        // adds a side-effecting dependency, this test should be updated to
        // verify the mock is NOT called for write operations.
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_wordpress("foo", Some(8090)).unwrap();
        // The plan is fully populated but nothing was executed.
        assert!(!plan.containers.is_empty());
        assert!(!plan.firewall_rules.is_empty());
        assert!(!plan.paths.is_empty());
    }
}
