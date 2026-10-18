// Adapters Layer — Implementaciones concretas organizadas por dominio
//
// Estructura:
//   infra/     → Docker, Nginx, Systemd, Filesystem, Logging, Security, MachineId
//   git/       → Git client, Pipeline watcher
//   web/       → HTTP auth, Cert, Session store
//   hardware/  → Hardware probe
//   testing/   → Cargo test runner
//   (flat)     → tor.rs

pub mod ai; // Fase 9 — adapter HTTP contra Ollama (/api/chat, /api/tags, /api/pull)
pub mod cms; // DRUPAL-001 — Catálogo de adapters CMS (WordPress, Drupal, …)
pub mod git;
pub mod hardware;
pub mod http_probe; // MCP-006 — sonda reqwest+SOCKS5h (verify_onion)
pub mod infra;
pub mod mcp; // MCP-002 — transporte stdio NDJSON + guardia de stdout
pub mod process_runner; // MCP-004 — enola-cli como subproceso (argv, sin shell)
pub mod testing;
pub mod web;

// Flat modules (pequeños o transversales)
pub mod tor;
