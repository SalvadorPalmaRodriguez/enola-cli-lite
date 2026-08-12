// Adapters Layer — Implementaciones concretas organizadas por dominio
//
// Estructura:
//   infra/     → Docker, Nginx, Systemd, Filesystem, Logging, Security, MachineId
//   git/       → Git client, Pipeline watcher
//   web/       → HTTP auth, Cert, Session store
//   hardware/  → Hardware probe
//   testing/   → Cargo test runner
//   (flat)     → tor.rs

pub mod cms; // DRUPAL-001 — Catálogo de adapters CMS (WordPress, Drupal, …)
pub mod git;
pub mod hardware;
pub mod infra;
pub mod testing;
pub mod web;

// Flat modules (pequeños o transversales)
pub mod tor;
