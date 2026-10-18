// Ports Layer
// Traits (Interfaces) describing what the application needs from the outside world.

pub mod ai; // Fase 10 — Puerto de ciclo de vida de instancias IA (Ollama + Open WebUI)
pub mod apparmor; // AA-002 (202) — AppArmor sandboxing port
pub mod cert;
pub mod cms;
pub mod connector;
pub mod container;
pub mod dependencies; // DEP-001..003 — System dependency port
pub mod external_ai; // Fase 13 — providers IA externos BYOK (OpenAI-compat + Anthropic)
pub mod file;
pub mod finetune;
pub mod firewall; // Gestión de firewall UFW (UFW-002)
pub mod hardware;
pub mod http_probe; // MCP-006 — sonda HTTP(S)/.onion (verify_onion)
pub mod llm; // Fase 9 — puerto LlmEngine (chat con tools, modelos Ollama)
pub mod manifest; // UNINSTALL-MANIFEST-001 — manifest-based uninstall port
pub mod mcp_transport; // MCP-002 — transporte de mensajes (recv/send) + mock
pub mod pipeline;
pub mod port_checker; // PORTS-002 (176) — validación libre de puertos OS+Docker
pub mod port_config;
pub mod process_runner; // MCP-004 — ejecución de enola-cli como subproceso (argv)
pub mod rag; // Fase 12 — puertos RAG (vector store, fuentes, aprovisionamiento)
pub mod service;
pub mod test_runner;
pub mod tor;
pub mod vpn; // Tarea 162 — WireGuard VPN port trait
pub mod vpn_bridge; // VPN over Tor — socat UDP→TCP bridge port
pub mod web; // DRUPAL-001 — CmsAdapter + CmsLifecycle traits // Fase 14 — jobs de fine-tuning (`ai finetune`)
