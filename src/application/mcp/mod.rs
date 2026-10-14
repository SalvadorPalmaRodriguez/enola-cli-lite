// Application MCP — ToolRegistry, resources y política del servidor MCP.
pub mod audit; // MCP-007 — log de auditoría de escrituras (stderr/fichero)
pub mod builtin_tools;
pub mod command_catalog;
pub mod confirmation; // MCP-009 — confirmación de dos pasos para destructivas
pub mod policy; // MCP-007 — política read-only/--allow-write + allowlist
pub mod registry;
pub mod resources; // MCP-005 — resources/list + resources/read + docs_search
pub mod run_command;
