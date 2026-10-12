> **Documento usuario:** `docs/user/mcp/commands-mcp.md`
> **Versión:** 1.0 | **Actualizado:** 2026-10-12
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🔌 MCP — Servidor stdio para agentes IA

`enola-cli mcp` arranca un servidor **Model Context Protocol** (MCP) por
**stdio**. Permite que un agente IA externo (Claude Desktop, Cursor, Devin…)
opere la infraestructura del usuario usando **exclusivamente** los comandos de
`enola-cli` y su documentación de usuario.

## Sintaxis

```bash
enola-cli mcp [--allow-write] [--allow-tools TOOLS]
```

| Flag | Descripción |
|------|-------------|
| `--allow-write` | Permite tools que mutan el sistema. Por defecto: **read-only**. |
| `--allow-tools` | Lista separada por comas de nombres de tools permitidas. |

## Protocolo

- **JSON-RPC 2.0** por stdio: stdout transporta **exclusivamente** JSON-RPC
  (una línea NDJSON por mensaje); los logs van a stderr.
- Métodos: `initialize`, `notifications/initialized`, `ping`, `tools/list`,
  `tools/call`, `resources/list`, `resources/read`.
- `protocolVersion`: `2026-07-28`.

## Seguridad

- **Read-only por defecto**: sin `--allow-write` ninguna tool puede mutar el sistema.
- Las tools mutantes exigen `--allow-write` **y** estar en la allowlist
  `[mcp].allowed_tools` de `config.toml`.
- El servidor **no** eleva privilegios por su cuenta.

## Ejemplos

```bash
# Servidor read-only (solo consultas)
enola-cli mcp

# Permitir mutaciones acotadas
enola-cli mcp --allow-write --allow-tools wp_create,wp_publish
```