// Domain Layer
// Pure business logic and types. No external dependencies (if possible).

pub mod ai; // Fase 10 — Tipos puros del stack IA local (Ollama + Open WebUI)
pub mod ai_benchmark; // Fase 13 — métricas de benchmark local (`ai benchmark`)
pub mod ai_finetune; // Fase 14 — datasets JSONL y fine-tuning (`ai finetune`)
pub mod ai_model; // Fase 11 — Biblioteca de modelos GGUF (ai model)
pub mod ai_providers; // Fase 13 — providers IA externos BYOK (`ai provider`)
pub mod app_config; // CONFIG-001 + CONFIG-002 — distribution & web URLs
pub mod apparmor; // AA-001 (201) — AppArmor sandboxing domain types
pub mod cms;
pub mod connector;
pub mod dependencies; // DEP-001..003 — System dependency management
pub mod docs; // MCP-005 — documentación de usuario embebida + catálogo MCP
pub mod error;
pub mod firewall;
pub mod git;
pub mod git_flattening;
pub(crate) mod license_acceptance; // LIC-002 — License acceptance on first run
pub mod mcp; // MCP-001 — Tipos JSON-RPC 2.0 + MCP (servidor stdio)
pub mod naming;
pub mod plan; // Tarea 3 — `enola plan` dry-run domain types
pub mod port_config;
pub mod rag; // Fase 12 — Índice RAG (chunking, colecciones, fuentes)
pub mod system;
pub mod tests;
pub mod tool; // MCP-004 — ToolSpec, ToolKind, clasificación read/write
pub mod tor;
pub mod user;
pub mod vpn; // Tarea 162 — WireGuard VPN domain types
pub mod wordpress; // DRUPAL-001 — Catálogo CMS (DbStack, CmsKind, CmsDescriptor)
