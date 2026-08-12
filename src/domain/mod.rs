// Domain Layer
// Pure business logic and types. No external dependencies (if possible).

pub mod app_config; // CONFIG-001 + CONFIG-002 — distribution & web URLs
pub mod apparmor; // AA-001 (201) — AppArmor sandboxing domain types
pub mod cms;
pub mod connector;
pub mod dependencies; // DEP-001..003 — System dependency management
pub mod error;
pub mod firewall;
pub mod git;
pub mod git_flattening;
pub(crate) mod license_acceptance; // LIC-002 — License acceptance on first run
pub mod naming;
pub mod port_config;
pub mod system;
pub mod tests;
pub mod tor;
pub mod user;
pub mod vpn; // Tarea 162 — WireGuard VPN domain types
pub mod wordpress; // DRUPAL-001 — Catálogo CMS (DbStack, CmsKind, CmsDescriptor)
