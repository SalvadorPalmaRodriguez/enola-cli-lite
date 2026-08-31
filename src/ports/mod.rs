// Ports Layer
// Traits (Interfaces) describing what the application needs from the outside world.

pub mod apparmor; // AA-002 (202) — AppArmor sandboxing port
pub mod cert;
pub mod cms;
pub mod connector;
pub mod container;
pub mod dependencies; // DEP-001..003 — System dependency port
pub mod document;
pub mod file;
pub mod firewall; // Gestión de firewall UFW (UFW-002)
pub mod git;
pub mod hardware;
pub mod manifest; // UNINSTALL-MANIFEST-001 — manifest-based uninstall port
pub mod pipeline;
pub mod port_checker; // PORTS-002 (176) — validación libre de puertos OS+Docker
pub mod port_config;
pub mod service;
pub mod test_runner;
pub mod tor;
pub mod vpn; // Tarea 162 — WireGuard VPN port trait
pub mod vpn_bridge; // VPN over Tor — socat UDP→TCP bridge port
pub mod web; // DRUPAL-001 — CmsAdapter + CmsLifecycle traits
