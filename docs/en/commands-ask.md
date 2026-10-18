> **User document:** `docs/en/commands-ask.md`
> **Version:** 1.0 | **Updated:** 2026-10-18
> **Status:** ✅ **CURRENT — User guide**
> **Spanish original:** [`docs/user/ai/commands-ask.md`](../user/ai/commands-ask.md)
> **References:** commands.md, commands-mcp.md

# 🤖 `ask` — Local AI assistant (Ollama)

`enola-cli ask` lets you inspect and operate your Enola infrastructure in
natural language using a local LLM running on **Ollama**, without sending data
to external cloud providers.

It shares the same `ToolRegistry` as the MCP server (`enola-cli mcp`), replacing
unattended policy checks with an **interactive `[s/N]` confirmation gate on your
terminal (TTY)** for any mutating or destructive operation.

---

## Syntax

```bash
enola-cli ask <PROMPT> [--model MODEL] [--endpoint URL]
              [--instance NAME] [--no-exec] [--yes]
```

| Argument / Flag | Description |
|-----------------|-------------|
| `<PROMPT>` | Natural-language question or instruction (e.g. `"list my tor services"`). |
| `--model <MODEL>` | Ollama model name (overrides `ENOLA_AI_MODEL` / `[ai].model`; default: `llama3.1:8b`). |
| `--endpoint <URL>` | Ollama HTTP endpoint URL (overrides `ENOLA_AI_ENDPOINT` / `[ai].endpoint`; default: `http://127.0.0.1:11434`). Automatically routes `.onion` endpoints through Tor `socks5h://`. |
| `--instance <NAME>` | Managed AI instance name (`enola-ai-<NAME>-ollama`); resolves its local host port automatically. |
| `--no-exec` | Simulation mode: mutating and destructive tool calls run via `try_run` (`ToolRegistry::try_call`), returning the plan or `argv` with zero side effects. |
| `-y`, `--yes` | Approve mutating/destructive operations without an interactive TTY prompt (for scripts). |

---

## Configuration (`[ai]`)

Resolution precedence (high → low):

1. **CLI flags**: `--endpoint`, `--model`, `--instance`
2. **Environment variables**: `ENOLA_AI_ENDPOINT`, `ENOLA_AI_MODEL`, `ENOLA_AI_TIMEOUT_SECS`
3. **`~/.enola/config.toml`**: `[ai]` section
4. **Built-in defaults**: `http://127.0.0.1:11434`, `llama3.1:8b`, `120` seconds

```toml
[ai]
endpoint = "http://127.0.0.1:11434"
model = "llama3.1:8b"
timeout_secs = 120
```

Inspect effective values with:

```bash
enola-cli config-show --json
```

---

## Security and interactive gate

1. **Read-only operations (`ReadOnly`)**: execute directly (`tor_list`,
   `wp_list`, `wp_status`, `ports_list`, `diag_summary`, `verify_onion`,
   `plan_wp_create`, `docs_search`, etc.).
2. **Mutating (`Mutating`) and destructive (`Destructive`) operations**:
   - With `--no-exec`: **never** modify the system; they invoke `try_run` and
     return the simulated plan or exact `argv`.
   - Without `--no-exec`: require an **interactive terminal (TTY)** (or explicit
     `--yes`; without a TTY and without `--yes` the command fails closed).
     Before running, `ask` displays the tool name and secret-redacted arguments
     (`sanitize_args` / `redact_argv`), prints a reinforced warning for
     `Destructive` operations, and prompts `[s/N]`.
3. **Anti-recursion**: `ask` is in `FORBIDDEN_FAMILIES`, so `enola_run_command`
   never dispatches `ask` as a subprocess.
4. **Clean schemas for local models**: MCP-only meta-properties
   (`confirm_token`, `try_run`) are stripped from schemas sent to Ollama so
   smaller 8B models are not confused.

---

## Examples

```bash
# Query active Tor services (read-only)
enola-cli ask "list my tor services and check their status"

# Simulate creating a WordPress site without side effects (--no-exec)
sudo enola-cli ask "create a wordpress site named myblog on port 8090" --no-exec

# Execute with interactive [s/N] confirmation on TTY
sudo enola-cli ask "create a wordpress site named myblog and publish it on tor"

# Use a specific model or instance and JSON output
enola-cli --format json ask "diagnostics summary" --model qwen2.5:7b --instance dev
```

---

## Cross-references

- [Command Index](commands.md)
- [MCP server (`enola-cli mcp`)](commands-mcp.md)
