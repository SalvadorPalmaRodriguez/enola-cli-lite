> **User document:** `docs/en/commands-mcp.md`
> **Version:** 1.5 | **Updated:** 2026-10-28
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
| `--allow-tools` | CSV allowlist of tool names (`wp_create`) or command paths (`wp:create`; bare `family` for leaf commands like `doctor`). |
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

**Mutating** (require `--allow-write` + allowlist): `wp_create`.

**Destructive** (require `--allow-write` + allowlist, `force: true`
where the leaf declares it, and **two-step confirmation**): `wp_publish`,
`wp_hide`, `tor_create`, `tor_remove`, `files_create`, and via
`enola_run_command` the paths in the impact table below.

**Always vetoed** (`enola_run_command` never dispatches them under any
policy): the `mcp`, `ask`, `ssh`, `uninstall`, `web` and `setup` families, and
the `update apply`, `test run` and `test benchmark` paths, plus the ones that
print a generated secret (`tor auth generate`, `vpn peer add`,
`git user create`): those keys and passwords would end up in the agent's
context. Run them from the terminal or with the web dashboard buttons. The `update`
flags `--binary`/`--allow-unsigned` are not expressible either, nor is
any flag value starting with `-`.

**Meta-tools**: `enola_list_commands` (command family catalog) and
`enola_run_command` (`{family, subcommand, args}` validated against the
generated command tree — full subcommand coverage, always forcing
`--format json`; mutating commands are still subject to the policy).
`enola_run_command` additionally accepts `try_run` (simulation) and
`confirm_token` (second step of destructive calls).

### Impact table (for sizing the allowlist)

`--allow-tools` entries are tool names (`wp_create`) or `family:sub`
paths (bare `family` for leaf commands; level-3 paths use a space:
`tor:auth revoke`).

| Entry | Class | Impact |
|-------|-------|--------|
| `wp:create`, `*:start|stop|restart|edit|build-image` | Mutating | Creates/manages Docker containers |
| `update:verify-feed` | Mutating | Reads a local file or fetches an arbitrary http(s) URL (oracle/SSRF) |
| `tor:create`, `tor:edit`, `tor:rotate` | Destructive | Exposes or modifies `.onion` hidden services (publishes a local port to the network) |
| `tor:auth add|generate|disable` | Destructive | Grants or removes `.onion` client credentials |
| `tor:remove`, `tor:auth revoke`, `tor:auth rotate` | Destructive | Deletes hidden services / credentials (the `.onion` is lost) |
| `vpn:create`, `vpn:peer add`, `vpn:peer add-pubkey` | Destructive | Opens the VPN server / grants network access to third parties |
| `files:create`, `*:publish` | Destructive | Publishes files or sites to the network in one step |
| `*:delete`, `*:hide`, `git:user delete`, `vpn:peer remove` | Destructive | Irreversible data/resource destruction |
| `firewall:setup`, `firewall:allow`, `firewall:deny` | Destructive | Opens/closes host firewall ports |
| `apparmor:setup`, `apparmor:mode` | Destructive | Changes kernel confinement (weakens defense) |
| `git:create`, `git:registration`, `git:user create` | Destructive | Creates repos/users, grants access (incl. `--admin-password`) |
| `maintenance:backup` | Destructive | Exports data (backup tarball) |
| `maintenance:disable-checks`, `maintenance:ssh-harden-pqc`, `maintenance:cleanup` | Destructive | Disables periodic checks / modifies sshd / deletes files |
| `update:download` | Destructive | Downloads a candidate binary (`update apply` is vetoed) |

## Simulation (`try_run`)

Every mutating/destructive tool accepts `"try_run": true`. The server
validates **the same parameters and permissions** as a real execution and
returns a structured result **with no side effects**:

```json
{"simulated": true, "reliable": true, "operation": "wp_create",
 "params": {...}, "plan": {...}, "effects": [...], "limitations": ""}
```

- `reliable: true` → `plan` comes from the real planner (`wp_create`,
  `tor_create`, `enola_run_command` return the exact `argv`).
- `reliable: false` → limited simulation; `limitations` explains what
  cannot be predicted (`wp_publish`, `wp_hide`, `tor_remove`,
  `files_create`).
- `{"simulated": false, "reason": "..."}` → the tool cannot be simulated.
- A simulation **never** creates confirmation tokens nor executes anything.

## Destructive operation confirmation

`force: true` alone does **not** authorize a destructive call. Flow:

1. First call with the real parameters → response
   `{"confirmation_required": true, "confirm_token": "<hex>", "params": {...},
   "expires_in_secs": 300}` (nothing is executed).
2. Second call with **the same parameters** + `confirm_token` → executes.

The token is **single-use**, **expires** (300 s), is **bound** to the tool
and the exact arguments (changing any parameter invalidates it) and does
**not** survive a server restart. Real human confirmation is provided by
the MCP client approving each `tools/call`; the token guarantees the
approved operation is exactly the one that was shown.

> ⚠️ The `confirm_token` is **not a boundary against a hostile agent**:
> if the caller can issue two `tools/call`s, it can complete both steps.
> The real security boundary is `--allow-write` + the allowlist (and the
> structural vetoes of `enola_run_command`); the token only guarantees
> that what executes is exactly what the operator saw in the challenge.

**Resources**: embedded user documentation is exposed as
`enola://docs/<domain>/<file>` (text/markdown) via `resources/list` and
`resources/read`.

## Security

- **Read-only by default**: without `--allow-write` no tool can mutate the
  system — rejection happens *before* the use case runs.
- Empty `allowed_tools` + `--allow-write` → **all** mutating tools are
  allowed (explicit decision, use with care).
- **Strict schemas**: every tool declares `additionalProperties: false`;
  unknown parameters, wrong types and out-of-range values are rejected
  (`maxLength`, `minimum`/`maximum` on ports).
- **Destructive = two steps** (see above): `force` alone is not enough; a
  single-use `confirm_token` bound to the operation is required.
- `enola_run_command` **never** dispatches the `mcp`, `ask`, `ssh`, `uninstall`,
  `web`, `setup` families nor the `update apply`, `test run`,
  `test benchmark`, `tor auth generate`, `vpn peer add`, `git user create` paths; the `--binary`/`--allow-unsigned` flags of
  `update` are not expressible, and any flag value starting with `-` is
  rejected. It never builds shell strings: typed argv + tree validation.
- **Reduced child environment**: dispatched subprocesses run with a
  cleared environment (`env_clear`) + allowlist (`PATH`, `HOME`, `USER`,
  `SUDO_USER`, `LANG`, `TERM`, `LC_*`, `ENOLA_ACCEPT_LICENSE`) — the
  server's `ENOLA_*` vars are not inherited.
- `files_create` requires `share_path` under `/srv/enola-files/` (the
  interactive CLI allows other paths; the agent does not).
- Every mutating call lands in the **audit log** (timestamp, tool,
  secret-redacted arguments, state: `rejected`/`simulated`/
  `confirmation_requested`/`authorized`/`ok`/`error`, operation id) on
  `--audit-log`/stderr. The file is created `0600` and its directory
  `0700` (O_NOFOLLOW).
- **Privileged policy**: with euid 0 the `[mcp]` section is read only
  from `/etc/enola/config.toml` (root-owned) — the user's
  `~/.enola/config.toml` is ignored.
- The server does **not** escalate privileges on its own: it runs with the
  privileges of the launching process (if launched with `sudo`, mutating
  tools and `enola_run_command` subprocesses inherit root — scope the
  allowlist accordingly).

## Cancellation

If the client dies mid-mutation, the operation may be left half-done: the
partial state is recorded in the audit log and there is **no automatic
rollback** unless the use case guarantees it. Subprocesses dispatched by
`enola_run_command` are killed on timeout or when the server dies.

## Privileged deployment (recommended)

If the server must run as root, pin the policy **outside user control**:
argv in a root-owned launcher, `[mcp]` in `/etc/enola/config.toml` (the
only config honored at euid 0), a **specific** (non-empty) allowlist and
an audit log in a root-owned directory.

```ini
# /etc/systemd/system/enola-mcp.service
[Service]
ExecStart=/usr/local/bin/enola-cli mcp --allow-write \
          --allow-tools wp_create,wp:publish \
          --audit-log /var/log/enola/mcp-audit.log
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=read-only
RestrictAddressFamilies=AF_UNIX AF_INET AF_NETLINK
```

- **Never** a wildcard sudoers rule (`enola-cli mcp *`): it lets the user
  choose `--allow-write`/`--allow-tools` and therefore the whole policy.
- Honest note: this is **not** a real privilege sandbox — the Docker
  daemon is equivalent to root. Its value is pinning argv/env (the
  policy does not depend on launcher-chosen flags) and centralizing the
  audit trail in journald.
- If the `web` dashboard also runs on this host, beware: its session
  token goes to stderr and is readable via `logs view`/journald.

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
