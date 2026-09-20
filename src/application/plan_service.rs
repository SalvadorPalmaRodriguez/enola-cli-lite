//! Application use case: `enola plan` — dry-run service planning (Tarea 3).
//!
//! Computes what WOULD happen if a `create` command were executed, WITHOUT
//! any side effects: no Docker, no UFW, no AppArmor, no file writes.
//!
//! Reuses `PortValidator` (application/port_validator.rs) for port resolution
//! — does NOT drop down to the raw `PortCheckerPort` trait.

use crate::application::port_validator::{PortRanges, PortValidator};
use crate::domain::error::{EnolaError, Result};
use crate::domain::naming;
use crate::domain::plan::{
    blueprint_for, PlanKind, PlanOptions, ResolvedPort, ServicePlan, TorServiceType,
};
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
            range: Some(PortRanges::WORDPRESS_BACKEND),
            firewall: true,
        }];
        let blueprint = blueprint_for(PlanKind::WordPress);
        Ok(blueprint.build(name, &resolved, &PlanOptions::default()))
    }

    /// Plan a Git server creation.
    ///
    /// KEEP-IN-SYNC: `git create` (deploy_git_server.rs) — with `--ssl` it
    /// auto-assigns an Nginx HTTPS port but only http-port and ssh-port are
    /// registered in UFW.
    pub fn plan_git(
        &self,
        name: &str,
        ssl: bool,
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
        let mut resolved = vec![
            ResolvedPort {
                label: "http-port",
                port: resolved_http,
                auto_assigned: http_port.is_none(),
                range: Some(PortRanges::GIT_HTTP),
                firewall: true,
            },
            ResolvedPort {
                label: "ssh-port",
                port: resolved_ssh,
                auto_assigned: ssh_port.is_none(),
                range: Some(PortRanges::GIT_SSH),
                firewall: true,
            },
        ];
        if ssl {
            let resolved_https = self
                .port_validator
                .auto_assign(PortRanges::NGINX_HTTPS, "https-port")?;
            resolved.push(ResolvedPort {
                label: "https-port",
                port: resolved_https,
                auto_assigned: true,
                range: Some(PortRanges::NGINX_HTTPS),
                // `git create --ssl` does NOT sync the HTTPS port to UFW.
                firewall: false,
            });
        }
        let blueprint = blueprint_for(PlanKind::Git);
        Ok(blueprint.build(
            name,
            &resolved,
            &PlanOptions {
                ssl,
                tor_type: None,
                extra_notes: vec![],
            },
        ))
    }

    /// Plan a Tor hidden service creation.
    ///
    /// `virtual_port` is the public .onion port (not a real socket — not
    /// validated). `target_port` is the local app port: the user's app is
    /// assumed to be listening there, so it is NOT checked against the port
    /// checker either.
    ///
    /// KEEP-IN-SYNC: `tor::create` (src/cli/commands.rs):
    /// - `virtual_port` is only honored for `raw`; web/static/files always
    ///   publish .onion:80 (and :443 with `--ssl`).
    /// - Only an explicit `--target-port` is registered in UFW; Nginx ports
    ///   are never registered.
    /// - `static`/`files` ignore `--target-port` and `--ssl`.
    pub fn plan_tor(
        &self,
        name: &str,
        service_type: &str,
        virtual_port: u16,
        target_port: Option<u16>,
        ssl: bool,
    ) -> Result<ServicePlan> {
        naming::validate_service_name(name)?;
        let tor_type = TorServiceType::parse(service_type).ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "Unknown service type '{}'. Valid types: raw|tcp, web|proxy|http, static, files|fileserver",
                service_type
            ))
        })?;

        let mut notes = Vec::new();
        if tor_type != TorServiceType::Raw && virtual_port != 80 {
            notes.push(format!(
                "`--virtual-port {}` is ignored for service type '{}': `tor create` always publishes .onion:80 (and :443 with --ssl).",
                virtual_port,
                tor_type.as_str()
            ));
        }
        if matches!(tor_type, TorServiceType::Static | TorServiceType::Files) {
            if target_port.is_some() {
                notes.push(format!(
                    "`--target-port` is ignored for service type '{}': the Nginx port is auto-assigned in range 20000-30000.",
                    tor_type.as_str()
                ));
            }
            if ssl {
                notes.push(format!(
                    "`--ssl` is ignored for service type '{}': only 'web' supports HTTPS in `tor create`.",
                    tor_type.as_str()
                ));
            }
        }

        let nf =
            |label: &'static str, port: u16, auto: bool, range: Option<(u16, u16)>| ResolvedPort {
                label,
                port,
                auto_assigned: auto,
                range,
                firewall: false,
            };

        let resolved: Vec<ResolvedPort> = match tor_type {
            TorServiceType::Raw => vec![
                nf("virtual-port", virtual_port, false, None),
                ResolvedPort {
                    label: "target-port",
                    port: target_port.unwrap_or(virtual_port),
                    auto_assigned: false,
                    range: None,
                    // `tor create` registers target_port in UFW only when the
                    // user passed it explicitly (executor.rs).
                    firewall: target_port.is_some(),
                },
            ],
            TorServiceType::Web => {
                let backend = ResolvedPort {
                    label: "backend-port",
                    port: target_port.unwrap_or(8080),
                    auto_assigned: false,
                    range: None,
                    firewall: target_port.is_some(),
                };
                if ssl {
                    let http = self
                        .port_validator
                        .auto_assign(PortRanges::NGINX_HTTP, "nginx-http-port")?;
                    let https = self
                        .port_validator
                        .auto_assign(PortRanges::NGINX_HTTPS, "nginx-https-port")?;
                    vec![
                        nf("virtual-port", 80, false, None),
                        nf("virtual-port-https", 443, false, None),
                        nf("nginx-http-port", http, true, Some(PortRanges::NGINX_HTTP)),
                        nf(
                            "nginx-https-port",
                            https,
                            true,
                            Some(PortRanges::NGINX_HTTPS),
                        ),
                        backend,
                    ]
                } else {
                    let nginx = self
                        .port_validator
                        .auto_assign(PortRanges::NGINX_LISTEN, "nginx-port")?;
                    vec![
                        nf("virtual-port", 80, false, None),
                        nf("nginx-port", nginx, true, Some(PortRanges::NGINX_LISTEN)),
                        backend,
                    ]
                }
            }
            TorServiceType::Static | TorServiceType::Files => {
                let nginx = self
                    .port_validator
                    .auto_assign(PortRanges::NGINX_STATIC, "nginx-port")?;
                vec![
                    nf("virtual-port", 80, false, None),
                    nf("nginx-port", nginx, true, Some(PortRanges::NGINX_STATIC)),
                ]
            }
        };

        let blueprint = blueprint_for(PlanKind::Tor);
        Ok(blueprint.build(
            name,
            &resolved,
            &PlanOptions {
                ssl,
                tor_type: Some(tor_type),
                extra_notes: notes,
            },
        ))
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
        let plan = svc
            .plan_git("repo", false, Some(10500), Some(30100))
            .unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert_eq!(plan.ports[0].port, 10500);
        assert_eq!(plan.ports[1].port, 30100);
    }

    #[test]
    fn test_plan_git_ssl_adds_https_port_no_firewall() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_git("repo", true, None, None).unwrap();
        assert_eq!(plan.ports.len(), 3);
        let https = &plan.ports[2];
        assert_eq!(https.label, "https-port");
        assert_eq!(https.port, PortRanges::NGINX_HTTPS.0);
        assert_eq!(https.source, "auto-assigned (range 15001-20000)");
        // Only http-port and ssh-port are synced to UFW — NOT https.
        assert_eq!(plan.firewall_rules.len(), 2);
        assert!(plan
            .notes
            .iter()
            .any(|n| n.contains("does NOT register the HTTPS port in UFW")));
    }

    #[test]
    fn test_plan_git_risk_medium_ssh() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_git("repo", false, None, None).unwrap();
        assert_eq!(plan.risk, crate::domain::plan::RiskLevel::Medium);
    }

    // ── Tor ──

    #[test]
    fn test_plan_tor_raw_two_ports() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "raw", 8080, None, false).unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert_eq!(plan.ports[0].label, "virtual-port");
        assert_eq!(plan.ports[0].port, 8080);
        // target defaults to virtual_port
        assert_eq!(plan.ports[1].label, "target-port");
        assert_eq!(plan.ports[1].port, 8080);
        // No explicit --target-port → no UFW rules.
        assert!(plan.firewall_rules.is_empty());
    }

    #[test]
    fn test_plan_tor_raw_explicit_target_one_rule() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "tcp", 80, Some(2222), false).unwrap();
        assert_eq!(plan.ports[1].port, 2222);
        assert_eq!(plan.firewall_rules.len(), 1);
        assert_eq!(plan.firewall_rules[0].port, 2222);
    }

    #[test]
    fn test_plan_tor_web_no_ssl() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, None, false).unwrap();
        assert_eq!(plan.ports.len(), 3);
        assert_eq!(plan.ports[0].label, "virtual-port");
        assert_eq!(plan.ports[0].port, 80);
        assert_eq!(plan.ports[1].label, "nginx-port");
        assert_eq!(plan.ports[1].port, PortRanges::NGINX_LISTEN.0);
        assert_eq!(plan.ports[1].source, "auto-assigned (range 10000-20000)");
        assert_eq!(plan.ports[2].label, "backend-port");
        assert_eq!(plan.ports[2].port, 8080);
        // No explicit --target-port → no UFW rules.
        assert!(plan.firewall_rules.is_empty());
    }

    #[test]
    fn test_plan_tor_web_ssl_five_ports() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(9000), true).unwrap();
        assert_eq!(plan.ports.len(), 5);
        let labels: Vec<&str> = plan.ports.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "virtual-port",
                "virtual-port-https",
                "nginx-http-port",
                "nginx-https-port",
                "backend-port"
            ]
        );
        assert_eq!(plan.ports[2].source, "auto-assigned (range 10000-15000)");
        assert_eq!(plan.ports[3].source, "auto-assigned (range 15001-20000)");
        // Explicit --target-port → exactly one UFW rule (backend only).
        assert_eq!(plan.firewall_rules.len(), 1);
        assert_eq!(plan.firewall_rules[0].port, 9000);
    }

    #[test]
    fn test_plan_tor_static_two_ports_and_notes() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc
            .plan_tor("svc", "static", 80, Some(1234), false)
            .unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert_eq!(plan.ports[0].label, "virtual-port");
        assert_eq!(plan.ports[1].label, "nginx-port");
        assert_eq!(plan.ports[1].port, PortRanges::NGINX_STATIC.0);
        assert!(plan
            .notes
            .iter()
            .any(|n| n.contains("`--target-port` is ignored")));
    }

    #[test]
    fn test_plan_tor_files_ssl_ignored_note() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "files", 80, None, true).unwrap();
        assert_eq!(plan.ports.len(), 2);
        assert!(plan.notes.iter().any(|n| n.contains("`--ssl` is ignored")));
    }

    #[test]
    fn test_plan_tor_unknown_type_error() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let err = svc.plan_tor("svc", "bogus", 80, None, false).unwrap_err();
        assert!(err.to_string().contains("Unknown service type 'bogus'"));
    }

    #[test]
    fn test_plan_tor_web_virtual_port_ignored_note() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 8080, None, false).unwrap();
        // virtual-port is pinned to 80 in the plan.
        assert_eq!(plan.ports[0].port, 80);
        assert!(plan
            .notes
            .iter()
            .any(|n| n.contains("`--virtual-port 8080` is ignored")));
    }

    #[test]
    fn test_plan_tor_no_containers() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(15000), false).unwrap();
        assert!(plan.containers.is_empty());
    }

    #[test]
    fn test_plan_tor_risk_low() {
        let svc = PlanService::new(Arc::new(free_checker()));
        let plan = svc.plan_tor("svc", "web", 80, Some(15000), false).unwrap();
        assert_eq!(plan.risk, crate::domain::plan::RiskLevel::Low);
    }

    // ── Validation ──

    #[test]
    fn test_plan_invalid_name_rejected() {
        let svc = PlanService::new(Arc::new(free_checker()));
        assert!(svc.plan_wordpress("Bad;Name", None).is_err());
        assert!(svc.plan_git("Bad;Name", false, None, None).is_err());
        assert!(svc.plan_tor("Bad;Name", "web", 80, None, false).is_err());
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
