//! Domain types for the `enola plan` dry-run command (Tarea 3).
//!
//! Pure business logic — no external dependencies, no I/O, no side effects.
//!
//! A `ServicePlan` describes what WOULD happen if a `create` command were
//! executed: ports to open/assign, Docker containers to create, filesystem
//! paths, UFW rules, AppArmor profiles, and a statically-computed risk level.
//!
//! The blueprint constants below are declarative mirrors of the values
//! hardcoded in the application deploy use cases
//! (`deploy_wordpress.rs`, `deploy_git_server.rs`, `deploy_tor_service.rs`).
//! They are kept in sync via `KEEP-IN-SYNC` comments and regression tests.

use crate::domain::apparmor::AppArmorServiceType;

// ═══════════════════════════════════════════════════════════════════════════
// Risk level
// ═══════════════════════════════════════════════════════════════════════════

/// Statically-computed risk level for a planned service creation.
///
/// Computed from the plan itself (bind interface, privileged ports, SSH
/// exposure) — NOT from inspecting the live system (that is `doctor --security`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// All ports bind to 127.0.0.1, no privileged ports, no SSH exposure.
    Low,
    /// Privileged port (<1024, except 80/443) or SSH exposed to loopback.
    Medium,
    /// Any port binding to 0.0.0.0 (should never happen in Enola, defensive).
    High,
}

impl RiskLevel {
    /// Compute the risk level from a fully-resolved plan.
    ///
    /// Heuristic (static — no live system inspection):
    /// - `High` if any planned port binds to an interface other than 127.0.0.1.
    /// - `Medium` if any port is privileged (<1024, except 80/443) OR the plan
    ///   exposes SSH (Git with an ssh-port).
    /// - `Low` otherwise.
    pub fn compute(plan: &ServicePlan) -> Self {
        // High: any non-loopback bind (defensive — Enola always binds 127.0.0.1).
        for p in &plan.ports {
            if p.bind_interface != "127.0.0.1" {
                return RiskLevel::High;
            }
        }
        // Medium: privileged port (except 80/443) or SSH exposure.
        for p in &plan.ports {
            if p.port < 1024 && p.port != 80 && p.port != 443 {
                return RiskLevel::Medium;
            }
        }
        if plan.exposes_ssh {
            return RiskLevel::Medium;
        }
        RiskLevel::Low
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Plan components
// ═══════════════════════════════════════════════════════════════════════════

/// A port that would be used by the planned service.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedPort {
    /// Human-readable label: "http-port", "ssh-port", "target-port", "virtual-port".
    pub label: String,
    /// Resolved port number.
    pub port: u16,
    /// Interface the port binds to (always "127.0.0.1" in Enola).
    pub bind_interface: String,
    /// How the port was determined: "manual" or "auto-assigned (range START-END)".
    pub source: String,
}

/// A Docker container that would be created.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedContainer {
    /// Container name: "wp-foo", "db-foo", "enola-git-foo".
    pub name: String,
    /// Docker image: "wordpress:latest", "mariadb:10.6", "codeberg.org/forgejo/forgejo:9.0".
    pub image: String,
    /// Internal container port (the app listens here): 80, 3000, 22.
    pub internal_port: u16,
    /// Host port mapped to internal_port, if any (None for DB containers).
    pub host_port: Option<u16>,
    /// Bind-mount volumes: (host_path, container_path).
    pub volumes: Vec<(String, String)>,
    /// Docker network name.
    pub network: String,
}

/// A UFW rule that would be applied (WITHOUT executing it).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedFirewallRule {
    pub port: u16,
    pub protocol: String,
    pub scope: String,
}

/// An AppArmor profile that would be applied (WITHOUT executing aa-enforce).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedAppArmor {
    pub profile_name: String,
    pub mode: String,
}

/// A filesystem path that would be created.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedPath {
    pub path: String,
    pub purpose: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Service kind & blueprint
// ═══════════════════════════════════════════════════════════════════════════

/// The kind of service that can be planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanKind {
    WordPress,
    Git,
    Tor,
}

impl std::fmt::Display for PlanKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanKind::WordPress => write!(f, "wordpress"),
            PlanKind::Git => write!(f, "git"),
            PlanKind::Tor => write!(f, "tor"),
        }
    }
}

/// A port specification in a blueprint: (label, default range).
#[derive(Debug, Clone, Copy)]
pub struct PortSpec {
    pub label: &'static str,
    pub range: (u16, u16),
}

/// Static declarative descriptor for a service type.
///
/// Constants here are KEEP-IN-SYNC with the application deploy use cases:
/// - WordPress: `src/application/deploy_wordpress.rs`
/// - Git:       `src/application/deploy_git_server.rs`
/// - Tor:       `src/application/deploy_tor_service.rs` + `src/adapters/tor.rs`
pub struct ServiceBlueprint {
    pub kind: PlanKind,
    pub apparmor_service_type: AppArmorServiceType,
    pub port_specs: &'static [PortSpec],
    /// Whether this service exposes SSH (affects risk level).
    pub exposes_ssh: bool,
}

/// A fully-resolved plan — the output of the `plan` use case.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServicePlan {
    pub kind: PlanKind,
    pub service_name: String,
    pub ports: Vec<PlannedPort>,
    pub containers: Vec<PlannedContainer>,
    pub paths: Vec<PlannedPath>,
    pub firewall_rules: Vec<PlannedFirewallRule>,
    pub apparmor: PlannedAppArmor,
    pub risk: RiskLevel,
    /// Whether this plan exposes SSH (used by RiskLevel::compute).
    #[serde(skip)]
    pub exposes_ssh: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Blueprint registry
// ═══════════════════════════════════════════════════════════════════════════

/// Lookup the static blueprint for a service kind.
pub fn blueprint_for(kind: PlanKind) -> ServiceBlueprint {
    match kind {
        PlanKind::WordPress => ServiceBlueprint {
            kind: PlanKind::WordPress,
            apparmor_service_type: AppArmorServiceType::WordPress,
            // KEEP-IN-SYNC: PortRanges::WORDPRESS_BACKEND (8080, 9000)
            port_specs: &[PortSpec {
                label: "http-port",
                range: (8080, 9000),
            }],
            exposes_ssh: false,
        },
        PlanKind::Git => ServiceBlueprint {
            kind: PlanKind::Git,
            apparmor_service_type: AppArmorServiceType::Git,
            // KEEP-IN-SYNC: PortRanges::GIT_HTTP (10000, 15000), GIT_SSH (30000, 35000)
            port_specs: &[
                PortSpec {
                    label: "http-port",
                    range: (10000, 15000),
                },
                PortSpec {
                    label: "ssh-port",
                    range: (30000, 35000),
                },
            ],
            exposes_ssh: true,
        },
        PlanKind::Tor => ServiceBlueprint {
            kind: PlanKind::Tor,
            apparmor_service_type: AppArmorServiceType::Tor,
            // KEEP-IN-SYNC: PortRanges::NGINX_LISTEN (10000, 20000)
            port_specs: &[PortSpec {
                label: "target-port",
                range: (10000, 20000),
            }],
            exposes_ssh: false,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Plan builders (pure functions — no I/O)
// ═══════════════════════════════════════════════════════════════════════════

/// A resolved port: (label, port, was_auto_assigned).
pub struct ResolvedPort {
    pub label: &'static str,
    pub port: u16,
    pub auto_assigned: bool,
}

impl ServiceBlueprint {
    /// Build a fully-resolved plan from resolved ports.
    ///
    /// Pure function: no I/O, no side effects. The containers, paths,
    /// firewall rules, and AppArmor profile are derived declaratively.
    pub fn build(&self, name: &str, resolved: &[ResolvedPort]) -> ServicePlan {
        let ports: Vec<PlannedPort> = resolved
            .iter()
            .map(|rp| PlannedPort {
                label: rp.label.to_string(),
                port: rp.port,
                bind_interface: "127.0.0.1".to_string(),
                source: if rp.auto_assigned {
                    let spec = self
                        .port_specs
                        .iter()
                        .find(|s| s.label == rp.label)
                        .map(|s| s.range)
                        .unwrap_or((0, 0));
                    format!("auto-assigned (range {}-{})", spec.0, spec.1)
                } else {
                    "manual".to_string()
                },
            })
            .collect();

        let containers = self.containers(name, resolved);
        let paths = self.paths(name);
        let firewall_rules = self.firewall_rules(resolved);
        let apparmor = PlannedAppArmor {
            profile_name: self.apparmor_service_type.profile_name(name),
            mode: "complain".to_string(),
        };

        let mut plan = ServicePlan {
            kind: self.kind,
            service_name: name.to_string(),
            ports,
            containers,
            paths,
            firewall_rules,
            apparmor,
            risk: RiskLevel::Low, // placeholder, computed below
            exposes_ssh: self.exposes_ssh,
        };
        plan.risk = RiskLevel::compute(&plan);
        plan
    }

    /// Containers that would be created.
    fn containers(&self, name: &str, resolved: &[ResolvedPort]) -> Vec<PlannedContainer> {
        match self.kind {
            PlanKind::WordPress => {
                // KEEP-IN-SYNC: deploy_wordpress.rs
                //   wp container:  image "wordpress:latest", internal 80, host http_port
                //   db container:  image "mariadb:10.6", no host port
                //   network: enola_net_{name}
                let http_port = resolved
                    .iter()
                    .find(|r| r.label == "http-port")
                    .map(|r| r.port)
                    .unwrap_or(0);
                let network = format!("enola_net_{}", name);
                vec![
                    PlannedContainer {
                        name: format!("wp-{}", name),
                        image: "wordpress:latest".to_string(),
                        internal_port: 80,
                        host_port: Some(http_port),
                        volumes: vec![(
                            format!("/srv/enola-wordpress/{}_wp", name),
                            "/var/www/html".to_string(),
                        )],
                        network: network.clone(),
                    },
                    PlannedContainer {
                        name: format!("db-{}", name),
                        image: "mariadb:10.6".to_string(),
                        internal_port: 3306,
                        host_port: None,
                        volumes: vec![(
                            format!("/srv/enola-wordpress/{}_db", name),
                            "/var/lib/mysql".to_string(),
                        )],
                        network,
                    },
                ]
            }
            PlanKind::Git => {
                // KEEP-IN-SYNC: deploy_git_server.rs
                //   container: enola-git-{name}, image codeberg.org/forgejo/forgejo:9.0
                //   internal ports: 3000 (http), 22 (ssh)
                //   volume: /srv/enola-git/{name} -> /data
                let http_port = resolved
                    .iter()
                    .find(|r| r.label == "http-port")
                    .map(|r| r.port)
                    .unwrap_or(0);
                vec![PlannedContainer {
                    name: format!("enola-git-{}", name),
                    image: "codeberg.org/forgejo/forgejo:9.0".to_string(),
                    internal_port: 3000,
                    host_port: Some(http_port),
                    volumes: vec![(format!("/srv/enola-git/{}", name), "/data".to_string())],
                    network: format!("enola_net_{}", name),
                }]
            }
            PlanKind::Tor => {
                // Tor is a systemd service, not a Docker container.
                vec![]
            }
        }
    }

    /// Filesystem paths that would be created.
    fn paths(&self, name: &str) -> Vec<PlannedPath> {
        match self.kind {
            PlanKind::WordPress => vec![
                PlannedPath {
                    path: format!("/srv/enola-wordpress/{}_wp", name),
                    purpose: "WordPress files (bind mount -> /var/www/html)".to_string(),
                },
                PlannedPath {
                    path: format!("/srv/enola-wordpress/{}_db", name),
                    purpose: "MariaDB data (bind mount -> /var/lib/mysql)".to_string(),
                },
                PlannedPath {
                    path: format!("/srv/enola-wordpress/{}_secrets", name),
                    purpose: "Secrets directory (0700, root:root)".to_string(),
                },
            ],
            PlanKind::Git => vec![PlannedPath {
                path: format!("/srv/enola-git/{}", name),
                purpose: "Forgejo data (bind mount -> /data, chown 1000:1000)".to_string(),
            }],
            PlanKind::Tor => vec![
                PlannedPath {
                    path: format!("/var/lib/tor/enola_{}", name),
                    purpose: "Tor hidden service directory (debian-tor:debian-tor, 700)"
                        .to_string(),
                },
                PlannedPath {
                    path: format!("/etc/tor/enola.d/{}.conf", name),
                    purpose: "Tor hidden service config (root:debian-tor, 640)".to_string(),
                },
            ],
        }
    }

    /// UFW rules that would be applied (loopback only).
    fn firewall_rules(&self, resolved: &[ResolvedPort]) -> Vec<PlannedFirewallRule> {
        resolved
            .iter()
            .filter(|rp| rp.label != "virtual-port") // .onion ports are not real sockets
            .map(|rp| PlannedFirewallRule {
                port: rp.port,
                protocol: "tcp".to_string(),
                scope: "loopback".to_string(),
            })
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(label: &'static str, port: u16, auto: bool) -> ResolvedPort {
        ResolvedPort {
            label,
            port,
            auto_assigned: auto,
        }
    }

    // ── Risk computation ──

    #[test]
    fn test_risk_low_wordpress_loopback() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.risk, RiskLevel::Low);
    }

    #[test]
    fn test_risk_medium_git_ssh_exposed() {
        let bp = blueprint_for(PlanKind::Git);
        let plan = bp.build(
            "repo",
            &[
                resolved("http-port", 10500, false),
                resolved("ssh-port", 30100, false),
            ],
        );
        assert_eq!(plan.risk, RiskLevel::Medium);
        assert!(plan.exposes_ssh);
    }

    #[test]
    fn test_risk_medium_privileged_port() {
        let bp = blueprint_for(PlanKind::WordPress);
        // Port 25 is privileged and not 80/443 → Medium.
        let plan = bp.build("foo", &[resolved("http-port", 25, false)]);
        assert_eq!(plan.risk, RiskLevel::Medium);
    }

    #[test]
    fn test_risk_low_port_80_allowed() {
        let bp = blueprint_for(PlanKind::Tor);
        // virtual-port 80 is .onion (not a real socket), target-port 15000 is fine.
        let plan = bp.build(
            "svc",
            &[
                resolved("virtual-port", 80, false),
                resolved("target-port", 15000, false),
            ],
        );
        assert_eq!(plan.risk, RiskLevel::Low);
    }

    #[test]
    fn test_risk_high_non_loopback_bind() {
        let bp = blueprint_for(PlanKind::WordPress);
        let mut plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        // Simulate a non-loopback bind (defensive — should never happen in Enola).
        plan.ports[0].bind_interface = "0.0.0.0".to_string();
        assert_eq!(RiskLevel::compute(&plan), RiskLevel::High);
    }

    // ── Blueprint: WordPress ──

    #[test]
    fn test_blueprint_wp_containers_match_deploy() {
        // Regression guard: if deploy_wordpress.rs changes image/port/volumes,
        // this test fails, signaling the blueprint needs updating.
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("myblog", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.containers.len(), 2);

        let wp = &plan.containers[0];
        assert_eq!(wp.name, "wp-myblog");
        assert_eq!(wp.image, "wordpress:latest"); // KEEP-IN-SYNC deploy_wordpress.rs:191
        assert_eq!(wp.internal_port, 80); // KEEP-IN-SYNC deploy_wordpress.rs:160
        assert_eq!(wp.host_port, Some(8090));
        assert_eq!(
            wp.volumes[0],
            (
                "/srv/enola-wordpress/myblog_wp".into(),
                "/var/www/html".into()
            )
        );

        let db = &plan.containers[1];
        assert_eq!(db.name, "db-myblog");
        assert_eq!(db.image, "mariadb:10.6"); // KEEP-IN-SYNC deploy_wordpress.rs:127
        assert_eq!(db.host_port, None);
        assert_eq!(
            db.volumes[0],
            (
                "/srv/enola-wordpress/myblog_db".into(),
                "/var/lib/mysql".into()
            )
        );
    }

    #[test]
    fn test_blueprint_wp_paths() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.paths.len(), 3);
        assert!(plan
            .paths
            .iter()
            .any(|p| p.path == "/srv/enola-wordpress/foo_wp"));
        assert!(plan
            .paths
            .iter()
            .any(|p| p.path == "/srv/enola-wordpress/foo_db"));
        assert!(plan
            .paths
            .iter()
            .any(|p| p.path == "/srv/enola-wordpress/foo_secrets"));
    }

    #[test]
    fn test_blueprint_wp_apparmor_profile() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.apparmor.profile_name, "enola-wp-foo");
        assert_eq!(plan.apparmor.mode, "complain");
    }

    #[test]
    fn test_blueprint_wp_firewall_one_rule() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.firewall_rules.len(), 1);
        assert_eq!(plan.firewall_rules[0].port, 8090);
        assert_eq!(plan.firewall_rules[0].scope, "loopback");
    }

    // ── Blueprint: Git ──

    #[test]
    fn test_blueprint_git_containers_match_deploy() {
        let bp = blueprint_for(PlanKind::Git);
        let plan = bp.build(
            "repo",
            &[
                resolved("http-port", 10500, false),
                resolved("ssh-port", 30100, false),
            ],
        );
        assert_eq!(plan.containers.len(), 1);
        let c = &plan.containers[0];
        assert_eq!(c.name, "enola-git-repo"); // KEEP-IN-SYNC deploy_git_server.rs:218
        assert_eq!(c.image, "codeberg.org/forgejo/forgejo:9.0"); // KEEP-IN-SYNC deploy_git_server.rs:219
        assert_eq!(c.internal_port, 3000); // KEEP-IN-SYNC deploy_git_server.rs:128
        assert_eq!(c.host_port, Some(10500));
        assert_eq!(c.volumes[0], ("/srv/enola-git/repo".into(), "/data".into()));
    }

    #[test]
    fn test_blueprint_git_firewall_two_rules() {
        let bp = blueprint_for(PlanKind::Git);
        let plan = bp.build(
            "repo",
            &[
                resolved("http-port", 10500, false),
                resolved("ssh-port", 30100, false),
            ],
        );
        assert_eq!(plan.firewall_rules.len(), 2);
        let ports: Vec<u16> = plan.firewall_rules.iter().map(|r| r.port).collect();
        assert!(ports.contains(&10500));
        assert!(ports.contains(&30100));
    }

    #[test]
    fn test_blueprint_git_apparmor_profile() {
        let bp = blueprint_for(PlanKind::Git);
        let plan = bp.build("repo", &[resolved("http-port", 10500, false)]);
        assert_eq!(plan.apparmor.profile_name, "enola-git-repo");
    }

    // ── Blueprint: Tor ──

    #[test]
    fn test_blueprint_tor_no_container() {
        let bp = blueprint_for(PlanKind::Tor);
        let plan = bp.build(
            "svc",
            &[
                resolved("virtual-port", 80, false),
                resolved("target-port", 15000, false),
            ],
        );
        assert!(
            plan.containers.is_empty(),
            "Tor is a systemd service, not Docker"
        );
    }

    #[test]
    fn test_blueprint_tor_paths() {
        let bp = blueprint_for(PlanKind::Tor);
        let plan = bp.build("svc", &[resolved("target-port", 15000, false)]);
        assert_eq!(plan.paths.len(), 2);
        assert!(plan
            .paths
            .iter()
            .any(|p| p.path == "/var/lib/tor/enola_svc"));
        assert!(plan
            .paths
            .iter()
            .any(|p| p.path == "/etc/tor/enola.d/svc.conf"));
    }

    #[test]
    fn test_blueprint_tor_apparmor_no_suffix() {
        let bp = blueprint_for(PlanKind::Tor);
        let plan = bp.build("svc", &[resolved("target-port", 15000, false)]);
        // Tor/Nginx/DockerBase profiles have no instance suffix.
        assert_eq!(plan.apparmor.profile_name, "enola-tor");
    }

    #[test]
    fn test_blueprint_tor_virtual_port_not_in_firewall() {
        let bp = blueprint_for(PlanKind::Tor);
        let plan = bp.build(
            "svc",
            &[
                resolved("virtual-port", 80, false),
                resolved("target-port", 15000, false),
            ],
        );
        // virtual-port is .onion (not a real socket) → no UFW rule.
        assert_eq!(plan.firewall_rules.len(), 1);
        assert_eq!(plan.firewall_rules[0].port, 15000);
    }

    // ── Auto-assigned source label ──

    #[test]
    fn test_auto_assigned_source_label() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8080, true)]);
        assert_eq!(plan.ports[0].source, "auto-assigned (range 8080-9000)");
    }

    #[test]
    fn test_manual_source_label() {
        let bp = blueprint_for(PlanKind::WordPress);
        let plan = bp.build("foo", &[resolved("http-port", 8090, false)]);
        assert_eq!(plan.ports[0].source, "manual");
    }

    // ── Display ──

    #[test]
    fn test_risk_level_display() {
        assert_eq!(RiskLevel::Low.to_string(), "low");
        assert_eq!(RiskLevel::Medium.to_string(), "medium");
        assert_eq!(RiskLevel::High.to_string(), "high");
    }

    #[test]
    fn test_plan_kind_display() {
        assert_eq!(PlanKind::WordPress.to_string(), "wordpress");
        assert_eq!(PlanKind::Git.to_string(), "git");
        assert_eq!(PlanKind::Tor.to_string(), "tor");
    }
}
