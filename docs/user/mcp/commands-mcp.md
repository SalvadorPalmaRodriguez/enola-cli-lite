> **Documento usuario:** `docs/user/mcp/commands-mcp.md`
> **Versión:** 1.1 | **Actualizado:** 2026-10-12
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md, config-reference.md

# 🔌 MCP — Servidor stdio para agentes IA

`enola-cli mcp` arranca un servidor **Model Context Protocol** (MCP) por
**stdio**. Permite que un agente IA externo (Claude Desktop, Cursor, Devin…)
opere la infraestructura del usuario usando **exclusivamente** los comandos de
`enola-cli` y su documentación de usuario — sin shell arbitraria, sin acceso
libre al filesystem y sin red sin restricciones.

## Sintaxis

```bash
enola-cli mcp [--allow-write] [--allow-tools TOOLS]
              [--max-output-bytes BYTES] [--audit-log PATH]
              [--request-timeout-secs SECS]
```

| Flag | Descripción |
|------|-------------|
| `--allow-write` | Permite tools que mutan el sistema. Por defecto: **read-only**. |
| `--allow-tools` | Lista CSV de tools permitidas (`wp_create`) o rutas de comando (`wp:create`). |
| `--max-output-bytes` | Límite de stdout/stderr devuelto por llamada (def: 65536). |
| `--audit-log` | Fichero de auditoría para llamadas mutantes (def: `stderr`; nunca stdout). |
| `--request-timeout-secs` | Timeout por llamada a tool (def: 60). |

Todo se puede fijar también por entorno (`ENOLA_MCP_*`) o por `config.toml`
`[mcp]` — precedencia: **flag > env > fichero > default**. Ver
`config-reference.md` § `[mcp]`.

## Protocolo

- **JSON-RPC 2.0** por stdio: stdout transporta **exclusivamente** JSON-RPC
  (una línea NDJSON por mensaje); los logs van a stderr.
- Métodos: `initialize`, `notifications/initialized`, `ping`, `tools/list`,
  `tools/call`, `resources/list`, `resources/read`.
- `protocolVersion`: `2026-07-28`.

## Tools

**Lectura** (siempre disponibles): `tor_list`, `diag_tor`, `diag_nginx`,
`diag_summary`, `config_show`, `doctor`, `update_check`, `wp_list`,
`wp_status`, `ports_list`, `verify_onion`, `plan_wp_create`,
`plan_tor_create`, `docs_search`.

**Mutantes** (requieren `--allow-write` + allowlist): `wp_create`,
`wp_publish`, `wp_hide`, `tor_create`, `tor_remove`, `files_create`.
Las destructivas (`tor_remove`, `wp_hide`) exigen además `force: true`
explícito en los argumentos.

**Meta-tools**: `enola_list_commands` (catálogo de familias/subcomandos) y
`enola_run_command` (`{family, subcommand, args}` validado contra el árbol
generado — cobertura de todos los subcomandos, forzando `--format json`;
los comandos mutantes quedan igualmente sujetos a la política).

**Resources**: la documentación de usuario embebida se expone como
`enola://docs/<domain>/<file>` (text/markdown) vía `resources/list` y
`resources/read`.

## Seguridad

- **Read-only por defecto**: sin `--allow-write` ninguna tool puede mutar
  el sistema — el rechazo ocurre *antes* de invocar el caso de uso.
- `allowed_tools` vacía + `--allow-write` → se permiten **todas** las
  tools mutantes (decisión explícita, úsese con cuidado).
- Cada llamada mutante queda en el **log de auditoría** (timestamp, tool,
  argumentos con secretos redactados, resultado) en `--audit-log`/stderr.
- El servidor **no** eleva privilegios por su cuenta: opera con los
  privilegios del proceso que lo lanza (normalmente `sudo`).

## Cancelación

Si el cliente muere a mitad de una mutación, la operación puede quedar a
medias: el estado parcial queda registrado en el log de auditoría y **no
hay rollback automático** salvo que el caso de uso lo garantice. Los
subprocesos despachados por `enola_run_command` se matan al expirar el
timeout o al morir el servidor.

## Ejemplo de configuración de cliente

```jsonc
// claude_desktop_config.json
{
  "mcpServers": {
    "enola": {
      "command": "sudo",
      "args": ["enola-cli", "mcp", "--allow-write",
               "--allow-tools", "wp_create,wp_publish"]
    }
  }
}
```

## Ejemplos

```bash
# Servidor read-only (solo consultas)
enola-cli mcp

# Permitir mutaciones acotadas con auditoría en fichero
enola-cli mcp --allow-write --allow-tools wp_create,wp_publish \
              --audit-log ~/.enola/mcp-audit.log
```
