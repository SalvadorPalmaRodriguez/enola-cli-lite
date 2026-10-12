> **User document:** `docs/en/commands-mcp.md`
> **Version:** 1.1 | **Updated:** 2026-10-12
> **Status:** ✅ **CURRENT — User guide**
> **Spanish original:** [`docs/user/mcp/commands-mcp.md`](../user/mcp/commands-mcp.md)
> **References:** commands.md, config-reference.md

# 🔌 MCP — stdio server for AI agents

`enola-cli mcp` starts a **Model Context Protocol** (MCP) server over
**stdio**. It lets an external AI agent (Claude Desktop, Cursor, Devin…)
operate the user's infrastructure using **exclusively** `enola-cli` commands
and its embedded user documentation — no arbitrary shell, no free filesystem
access, no unrestricted networking.

## Syntax

```bash
enola-cli mcp [--allow-write] [--allow-tools TOOLS]
              [--max-output-bytes BYTES] [--audit-log PATH]
              [--request-timeout-secs SECS]
```

| Flag | Description |
|------|-------------|
| `--allow-write` | Enables mutating tools. Default: **read-only**. |
| `--allow-tools` | CSV allowlist of tool names (`wp_create`) or command paths (`wp:create`). |
| `--max-output-bytes` | stdout/stderr cap per tool call (def: 65536). |
| `--audit-log` | Audit file for mutating calls (def: `stderr`; never stdout). |
| `--request-timeout-secs` | Per-tool timeout (def: 60). |

Everything can also be set via environment (`ENOLA_MCP_*`) or `config.toml`
`[mcp]` — precedence: **flag > env > file > default**. See
`config-reference.md` § `[mcp]`.

## Protocol

- **JSON-RPC 2.0** over stdio: stdout carries **exclusively** JSON-RPC
  (one NDJSON line per message); logs go to stderr.
- Methods: `initialize`, `notifications/initialized`, `ping`, `tools/list`,
  `tools/call`, `resources/list`, `resources/read`.
- `protocolVersion`: `2026-07-28`.

## Tools

**Read-only** (always available): `tor_list`, `diag_tor`, `diag_nginx`,
`diag_summary`, `config_show`, `doctor`, `update_check`, `wp_list`,
`wp_status`, `ports_list`, `verify_onion`, `plan_wp_create`,
`plan_tor_create`, `docs_search`.

**Mutating** (require `--allow-write` + allowlist): `wp_create`,
`wp_publish`, `wp_hide`, `tor_create`, `tor_remove`, `files_create`.
Destructive tools (`tor_remove`, `wp_hide`) additionally require an explicit
`force: true` argument.

**Meta-tools**: `enola_list_commands` (command family catalog) and
`enola_run_command` (`{family, subcommand, args}` validated against the
generated command tree — full subcommand coverage, always forcing
`--format json`; mutating commands are still subject to the policy).

**Resources**: embedded user documentation is exposed as
`enola://docs/<domain>/<file>` (text/markdown) via `resources/list` and
`resources/read`.

## Security

- **Read-only by default**: without `--allow-write` no tool can mutate the
  system — rejection happens *before* the use case runs.
- Empty `allowed_tools` + `--allow-write` → **all** mutating tools are
  allowed (explicit decision, use with care).
- Every mutating call lands in the **audit log** (timestamp, tool,
  secret-redacted arguments, result) on `--audit-log`/stderr.
- The server does **not** escalate privileges on its own: it runs with the
  privileges of the launching process (typically `sudo`).

## Cancellation

If the client dies mid-mutation, the operation may be left half-done: the
partial state is recorded in the audit log and there is **no automatic
rollback** unless the use case guarantees it. Subprocesses dispatched by
`enola_run_command` are killed on timeout or when the server dies.

## Client configuration example

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

## Examples

```bash
# Read-only server (queries only)
enola-cli mcp

# Scoped mutations with file audit
enola-cli mcp --allow-write --allow-tools wp_create,wp_publish \
              --audit-log ~/.enola/mcp-audit.log
```
