> **User document:** `docs/en/commands-ai.md`
> **Version:** 1.1 | **Updated:** 2026-10-18
> **Status:** ✅ **CURRENT — User guide**
> **Spanish original:** [`docs/user/ai/commands-ai.md`](../user/ai/commands-ai.md)
> **References:** commands-ask.md, commands.md, commands-mcp.md

# 🧠 `ai` — Local AI instances (Ollama + Open WebUI)

`enola-cli ai` deploys and manages isolated local AI stacks composed of
**Ollama** (`enola-ai-<name>-ollama`) and **Open WebUI** (`enola-ai-<name>-webui`),
bound exclusively to `127.0.0.1`, with optional NVIDIA GPU passthrough and
Tor (`.onion`) publication through Nginx.

Instances created with `ai create` are automatically discovered by
`enola-cli ask --instance <name>`.

---

## Subcommands

```bash
sudo enola-cli ai create --name <NAME> [--model <MODEL>] [--api-port <PORT>]
                         [--webui-port <PORT>] [--no-gpu] [--memory-gb <GIB>] [--no-pull]
sudo enola-cli ai list
sudo enola-cli ai status <NAME>
sudo enola-cli ai start <NAME>
sudo enola-cli ai stop <NAME>
sudo enola-cli ai delete <NAME> --force
sudo enola-cli ai publish <NAME> [--verify]
sudo enola-cli ai hide <NAME>
sudo enola-cli ai logs <NAME> [--lines <N>]

# Model library (level 3)
sudo enola-cli ai model list [--instance <NAME>]
sudo enola-cli ai model download <NAME> <org/repo|URL> [--file <F.gguf>]
                                 [--sha256 <HEX>] [--name <ALIAS>] [--tor] [--no-register]
sudo enola-cli ai model import <NAME> <PATH.gguf> [--name <ALIAS>] [--no-register]
sudo enola-cli ai model add <NAME> <model|file.gguf> [--name <ALIAS>]
sudo enola-cli ai model delete <NAME> <model|file.gguf> --force
sudo enola-cli ai model validate <NAME> <file.gguf>
sudo enola-cli ai model refresh <NAME>
```

| Subcommand | MCP Class | Description |
|------------|-----------|-------------|
| `ai create` | `Mutating` | Creates the isolated network `enola_net_ai_<name>` and starts `enola-ai-<name>-ollama` and `enola-ai-<name>-webui` on `127.0.0.1`. |
| `ai list` | `ReadOnly` | Lists all managed AI instances, their local ports, GPU mode, and `.onion` address. |
| `ai status <NAME>` | `ReadOnly` | Displays detailed status for the Ollama and Open WebUI containers of the instance. |
| `ai start <NAME>` | `Mutating` | Starts the containers of a stopped AI instance. |
| `ai stop <NAME>` | `Mutating` | Stops the containers of a running AI instance. |
| `ai delete <NAME> --force` | `Destructive` | Removes containers, Docker network, data in `/srv/enola-ai/<name>`, and any `.onion` service. Requires `--force`. |
| `ai publish <NAME> [--verify]` | `Destructive` | Publishes Open WebUI as a Tor `.onion` hidden service via Nginx + Tor (and verifies SOCKS5h reachability with `--verify`). |
| `ai hide <NAME>` | `Destructive` | Removes the `.onion` hidden service and Nginx config while keeping local containers intact. |
| `ai logs <NAME> [--lines N]` | `ReadOnly` | Displays the last `N` log lines from Ollama and Open WebUI (also available via `enola-cli logs view ai-<NAME>`). |
| `ai model list [--instance N]` | `ReadOnly` | Lists `.gguf` files in each instance library and models registered in its Ollama (`/api/tags`). |
| `ai model download <N> <SRC>` | `Mutating` | Downloads a `.gguf` from HuggingFace (`org/repo` + `--file`) or a direct URL, with streaming, `Range` resume and SHA-256 verification; registers it via `/api/create` unless `--no-register`. Defaults to **clearnet**; `--tor` or `[ai].use_tor_for_downloads` routes via SOCKS5h and the egress route is always reported. |
| `ai model import <N> <PATH>` | `Mutating` | Copies a local `.gguf` into `/srv/enola-ai/<N>/ollama/models-gguf/` after header validation, with `0640` permissions, and registers it unless `--no-register`. |
| `ai model add <N> <MODEL>` | `Mutating` | If `MODEL` ends in `.gguf` it registers the library file (`/api/create` + `FROM` Modelfile); otherwise it pulls it from the Ollama registry (`/api/pull`, e.g. `llama3.1:8b` or `hf.co/org/repo:tag`). |
| `ai model delete <N> <M> --force` | `Destructive` | Removes a `.gguf` from the library (`.gguf` suffix) or a model from the Ollama registry (`DELETE /api/delete`). Requires `--force`. |
| `ai model validate <N> <F>` | `ReadOnly` | Parses the GGUF header (magic + v2/v3 version + counters) and reports inferred family/quantization. |
| `ai model refresh <N>` | `Mutating` | Re-scans the instance library and registry. |

---

## GGUF model library

Each instance keeps its library at `/srv/enola-ai/<name>/ollama/models-gguf/`
(inside the Ollama container's `/root/.ollama` volume, so files appear as
`/root/.ollama/models-gguf/<file>`). File names are validated against path
traversal and models are registered in Ollama with a `FROM` Modelfile, so
`ask --instance <name>` can use them directly.

**Network privacy (R22):** downloads use **clearnet** by default. Use `--tor`
(or `[ai].use_tor_for_downloads = true` / `ENOLA_AI_USE_TOR_DOWNLOADS`) to force
the `socks5h://` proxy from `[http].tor_socks_proxy`; the CLI always reports
the egress route.

---

## `ai create` flags

| Flag | Description |
|------|-------------|
| `-n`, `--name <NAME>` | Instance name (lowercase letters, digits, and hyphens; max 63 chars). |
| `-m`, `--model <MODEL>` | Initial Ollama model (default: `llama3.1:8b`). |
| `--api-port <PORT>` | Host port on `127.0.0.1` for the Ollama HTTP API (default: auto `11435–12000`). |
| `--webui-port <PORT>` | Host port on `127.0.0.1` for Open WebUI (default: auto `8081–8200`). |
| `--no-gpu` | Force CPU mode even when an NVIDIA GPU is detected. |
| `--memory-gb <GIB>` | Memory limit in GiB for the Ollama container (minimum and default: `8` GiB). |
| `--no-pull` | Skip initial model download (`/api/pull`) during creation. |

---

## Hardening and isolation

- **Exclusive `127.0.0.1` binding**: both the Ollama API port and the Open WebUI port bind strictly to localhost.
- **Calibrated resource limits (R20)**: the Ollama container is configured with at least `8 GiB` of RAM and `pids_limit >= 4096` (BLAS/CUDA threads) to prevent OOM kills (`exit 137`), while enforcing `no-new-privileges:true` and `cap_drop: ["ALL"]`.
- **Anti-TOCTOU port locking**: ports are locked with `acquire_port_lock` (`flock`) before container creation, with full rollback if any step fails.
- **Safe GPU probing**: reuses `EnolaHardwareProbe` (NVML + privilege-dropped `nvidia-smi` fallback) and gracefully degrades to CPU mode with a clear warning when no NVIDIA GPU is available.

---

## Examples

```bash
# Create a local AI instance with default model (llama3.1:8b)
sudo enola-cli ai create --name dev --model llama3.1:8b

# Inspect instance status as JSON
sudo enola-cli --format json ai status dev

# Query the CLI assistant against the created instance
enola-cli ask "list my tor services" --instance dev

# Publish Open WebUI on Tor and verify .onion reachability
sudo enola-cli ai publish dev --verify

# View instance logs (via ai logs or logs view)
sudo enola-cli ai logs dev --lines 100
sudo enola-cli logs view ai-dev --lines 100

# Hide from Tor and delete the instance
sudo enola-cli ai hide dev
sudo enola-cli ai delete dev --force
```

---

## Cross-references

- [Local AI assistant `enola-cli ask`](commands-ask.md)
- [Command Index](commands.md)
- [MCP server (`enola-cli mcp`)](commands-mcp.md)
