> **User document:** `docs/en/commands-ai.md`
> **Version:** 1.14 | **Updated:** 2026-10-24
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

# RAG index (level 3)
sudo enola-cli ai rag status <NAME>
sudo enola-cli ai rag ingest <NAME> <PATH> [--embedding-fallback hash]
sudo enola-cli ai rag ingest-docs <NAME> [--embedding-fallback hash]
sudo enola-cli ai rag bind <NAME> --source-type <git|filesystem|wordpress> --path <P> --force
sudo enola-cli ai rag sync <NAME> [--embedding-fallback hash]
sudo enola-cli ai rag config <NAME> [--chunk-size N] [--chunk-overlap N]
                                [--remove-code] [--preset docs|code|faq|large]
                                [--vector-store chroma|qdrant] [--show]

# Local benchmark (level 2)
sudo enola-cli ai benchmark <NAME> [--model <M>] [--prompt <P>] [--runs <N>]

# BYOK cloud providers (level 3)
enola-cli ai provider list
enola-cli ai provider status <PROVIDER>
enola-cli ai provider set <PROVIDER> [--model <M>] [--endpoint <URL>]
                                    [--key-stdin] [--disable]
enola-cli ai provider remove <PROVIDER> --force
enola-cli ai provider test <PROVIDER> [--accept-risks] [--tor]

# ask against an external provider (BYOK — data leaves your machine)
enola-cli ask "question" --provider openai [--model <M>] [--accept-risks] [--tor]

# Fine-tuning LoRA (level 3)
enola-cli ai finetune prepare <FILE.jsonl> [--format <F>] [--out <DIR>] [--val-ratio <N>]
enola-cli ai finetune validate <FILE.jsonl> [--base-model <M>]
sudo enola-cli ai finetune start <NAME> --dataset <FILE.jsonl> [--base-model <M>]
                                [--tokenizer <ORG/REPO>] [--low-memory] [--no-register] --force
sudo enola-cli ai finetune status <NAME>
sudo enola-cli ai finetune register <NAME> <JOB_ID>
enola-cli ai finetune wizard
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
| `ai model download <N> <SRC>` | `Mutating` | Downloads a `.gguf` from HuggingFace (`org/repo` + `--file`) or a direct URL, streaming to `<file>.part` (resumable with `Range`), with SHA-256 and GGUF header verification before publishing it (a previous model with the same name is left untouched if anything fails); registers it via `/api/create` unless `--no-register`. Defaults to **clearnet**; `--tor` or `[ai].use_tor_for_downloads` routes via SOCKS5h and the egress route is always reported. |
| `ai model import <N> <PATH>` | `Mutating` | Copies a local `.gguf` into `/srv/enola-ai/<N>/ollama/models-gguf/` after header validation, with `0640` permissions, and registers it unless `--no-register`. |
| `ai model add <N> <MODEL>` | `Mutating` | If `MODEL` ends in `.gguf` it registers the library file (uploads the `.gguf` as an Ollama blob and creates it with `/api/create`); otherwise it pulls it from the Ollama registry (`/api/pull`, e.g. `llama3.1:8b` or `hf.co/org/repo:tag`). |
| `ai model delete <N> <M> --force` | `Destructive` | Removes a `.gguf` from the library (`.gguf` suffix) or a model from the Ollama registry (`DELETE /api/delete`). Requires `--force`. |
| `ai model validate <N> <F>` | `ReadOnly` | Parses the GGUF header (magic + v2/v3 version + counters) and reports inferred family/quantization. |
| `ai model refresh <N>` | `Mutating` | Re-scans the instance library and registry. |
| `ai rag status <N>` | `ReadOnly` | RAG index status: vector store, indexed documents and bound sources. |
| `ai rag ingest <N> <PATH>` | `Mutating` | Ingests a file or directory into the instance vector index (canonicalizes the path, rejects system dirs, skips dotfiles and symlink escapes). If Ollama returns no embeddings it fails without indexing anything (a TTY offers retry / hash / cancel); `--embedding-fallback hash` indexes with hash embeddings by explicit choice. |
| `ai rag ingest-docs <N>` | `Mutating` | Indexes the enola-cli documentation embedded in the binary into the instance's `enola_docs` collection (the one `docs_lookup` queries). Rebuilds the collection on every run (nothing stale after an upgrade) and never touches your own index (`ingest`/`sync`). Same embedding policy as `ingest`, including `--embedding-fallback hash`. |
| `ai rag bind <N> -s <T> -p <P> --force` | `Destructive` | Binds a source (`git`, `filesystem`, `wordpress`) to the index for `sync`. Requires `--force`; paths are canonicalized and system prefixes (`/etc`, `/root`, `/proc`…) are rejected. |
| `ai rag sync <N>` | `Mutating` | Re-ingests all bound sources (`git pull --ff-only` as the invoking user for git; REST `/wp-json/wp/v2/posts` for wordpress). Accepts `--embedding-fallback hash` like `ingest`. |
| `ai rag config <N> [--show]` | `Mutating` | Shows or edits `rag_config.json` (chunking, presets `docs|code|faq|large`, backend `chroma|qdrant`). |
| `ai benchmark <N>` | `Mutating` | Measures tokens/s (Ollama eval_duration), average latency and model size on the instance (`--runs`, `--model`, `--prompt`). |
| `ai provider list` | `ReadOnly` | Lists BYOK providers and their status (the key only shows as `(set)`/`(not set)`). |
| `ai provider status <P>` | `ReadOnly` | One provider's configuration status (never reveals the key). |
| `ai provider set <P>` | `Mutating` | Configures key/model/endpoint. The key is read via secure TTY prompt or `--key-stdin` — **never via argv**. |
| `ai provider remove <P> --force` | `Destructive` | Deletes the provider's entry from `~/.enola/providers.env`. Requires `--force`. |
| `ai provider test <P>` | `Mutating` | Minimal ping to the provider: sends data off-machine → requires consent (`--accept-risks` or TTY `[s/N]` prompt; fails closed without TTY). |
| `ask --provider <P>` | — | Single-shot chat via the external provider **without the tool registry** (cloud cannot run enola commands). Same privacy gate. |
| `ai finetune prepare <F>` | `Mutating` | Validates a JSONL dataset (`alpaca\|chatml\|sharegpt\|text`, auto-detected), converts to canonical ChatML and writes `train.jsonl`/`val.jsonl` with deterministic split (`--val-ratio`). |
| `ai finetune validate <F>` | `Mutating` | Pre-flight: schema + size + VRAM (`HardwareProbePort`) + base model — before spending compute hours. |
| `ai finetune start <N> --dataset <F> --force` | `Destructive` | Launches the `enola-ai-<n>-finetune-<job>` container (`enola-finetune:latest`, 16 GiB, GPU if present, read-only rootfs, no ports/network) with the embedded LoRA script. Dataset must be ChatML. Requires `--force`. |
| `ai finetune status <N>` | `ReadOnly` | Lists the instance jobs and reconciles `Running` ones with the real container exit (`Completed`/`Failed` + `finished_at`/`error` persisted to `finetune/jobs.json`). |
| `ai finetune register <N> <JOB_ID>` | `Mutating` | Materializes the model of a `Completed` job: imports the exported `.gguf` into the instance library (SHA-256 + header) and registers it in Ollama (`/api/create`) as `<name>-ft`. |
| `ai finetune wizard` | `Mutating` | Interactive TTY wizard (forbidden for `enola_run_command`). |

---

## Fine-tuning (`ai finetune`)

The pipeline converts JSONL datasets to canonical ChatML (`prepare`), validates
the job before spending compute (`validate` — schema, size, VRAM via
`HardwareProbePort`, base model) and launches training in an isolated container
(`start` — `enola-ai-<n>-finetune-<job>` via `ContainerPort`, no ports/network,
GPU if available, `--low-memory` mode for <10 GB VRAM).

The LoRA script (`train_lora.py` with transformers+peft) is **embedded** in the
binary (`include_str!`) and deployed to the private dir
`/srv/enola-ai/<n>/finetune/<job>/scripts/` — never to predictable `/tmp` paths
(SEC-001/N-01). Jobs are registered in `finetune/jobs.json` (`0640`).

**Base model and tokenizer (GGUF):** `start` resolves the base model against
the instance GGUF library (`ai model`). By default it uses the instance model
(`inst.model`) matched by the index `name` field; `--base-model` accepts that
`name` or a `.gguf` file from the library. If the model is not in the library,
`start` fails with a clear error ("importa primero un GGUF de ese modelo").
Before launching, the GGUF header is pre-validated (`general.architecture` +
`general.file_type`): an unsupported architecture or quantization aborts listing
the supported quantizations. The GGUF and tokenizer are mounted as volumes
(`/model.gguf` and `/tokenizer`) and the script loads the model natively from
the GGUF (`gguf_file`) and the tokenizer from the mounted directory.
`--tokenizer <org/repo>` overrides the model's tokenizer repo; if it is not
downloaded yet, it is fetched on the host on demand.

**Resulting model (GGUF export + registration):** on exit 0, the container
merges the LoRA adapter into the base weights and exports an F16 GGUF to
`/srv/enola-ai/<n>/finetune/<job>/out/<n>-ft-<job>.gguf` — unless `start` was
given `--no-register`, in which case only the PEFT adapter is saved under
`/out/adapter` (no merge, no conversion). The job becomes `Completed` (see
`ai finetune status`), but the model is **not materialized automatically**: run
`ai finetune register <n> <job>` to import it into the GGUF library (SHA-256 +
validated header) and register it in Ollama under the name `<n>-ft`. Registration
is an explicit step so that `status` stays `ReadOnly` (it must not mutate anything
under the MCP server).

> **⚠️ Real training requirements:** `ai finetune start` needs the
> `enola-finetune:latest` image (~8 GB: PyTorch CUDA + transformers + peft +
> datasets) built/available locally — recipe at
> `assets/finetune/Dockerfile` (`docker build -t enola-finetune:latest -f
> assets/finetune/Dockerfile assets/finetune`; CPU by default, NVIDIA GPU via
> `--build-arg TORCH_INDEX=.../cu126`; the recipe pins the base image by
> digest and every Python dependency, transitive ones included, with
> `requirements.lock` and, on GPU, `requirements-cu126.lock`) —, and an NVIDIA GPU with ≥10 GB free
> VRAM (or `--low-memory`). The rest of the pipeline (`prepare`, `validate`,
> `status`, the `--force`/ChatML gates) works without it. Real end-to-end
> training verification is manual and slow — see
> `docs/dev/test/test_finetune.sh` (excluded from the fast suite by design).

---

## External BYOK providers (`ai provider` / `ask --provider`)

Cloud providers (`openai`, `anthropic`, `grok`, `groq`, `openrouter`) are
**the only place in the CLI where user data leaves the machine**. They are
**disabled by default** and enforce strict guarantees:

- **Consent before any send**: explicit TTY `[s/N]` prompt; scripts/CI must
  pass `--accept-risks` (fails closed without TTY).
- **Keys live in `~/.enola/providers.env`** (honors `SUDO_USER`), atomic
  write + `0600` permissions, validated against control characters.
  Never shown in output, logs, errors or `config show`.
- **The key never travels via argv** — TTY prompt (no echo) or `--key-stdin`.
- **Route is reported**: clearnet by default; `--tor`, `ENOLA_AI_TOR_PROXY`
  or `[ai].tor_proxy = true` forces SOCKS5h (output states the route).
- **`ask --provider` is single-shot without tools**: the cloud cannot run
  `enola-cli` commands (minimal leak surface by design).

> **⚠️ Real verification:** actual provider calls require the user's own API
> key (BYOK by design — enola-cli ships no credentials). What IS covered by
> tests and E2E is the **safe behavior without a key**: `ai provider test` /
> `ask --provider` fail closed without consent (no TTY + no `--accept-risks`),
> and the key never appears in argv, output, logs or `config show`.

---

## RAG index (per-instance vector store)

Each instance may run its own vector store container
(`enola-ai-<name>-chromadb` or `enola-ai-<name>-qdrant`), lazily provisioned
via `ContainerPort` on `enola_net_ai_<name>`, bound to `127.0.0.1`, with a
`flock`-reserved host port in `12001–12500`, persistent volume
`/srv/enola-ai/<name>/<backend>/` and calibrated limits (1 GiB, 512 pids).

Embeddings are generated by the instance's Ollama (`/api/embed`,
`nomic-embed-text` by default). If Ollama does not answer, ingestion **fails
without indexing anything**: an interactive terminal offers `[r]etry` (up to 3
times), `[h]ash` or `[C]ancel`; without a terminal (scripts, MCP) it fails
directly. Deterministic hash embeddings (**non-semantic** search) are only used
if you choose them (`h` or `--embedding-fallback hash`). Each collection records
how it was indexed (`ollama:<model>` or `hash`, in `rag_config.json` →
`embedding_kinds`) and refuses mixing methods or models; queries use the same
method and fail if it is Ollama and it does not answer. Per-instance state lives in
`/srv/enola-ai/<name>/rag_config.json` and `rag_sources.json` (`0640`).

The `docs_lookup` MCP tool offers semantic search over the enola-cli
documentation (optional alternative to `docs_search`). It only queries the
`enola_docs` collection, filled by `ai rag ingest-docs <NAME>`, never your own
documents. If no instance has the docs indexed (or the given instance does
not), it returns an empty result list with a note suggesting
`ai rag ingest-docs` or `docs_search`, without an error. After upgrading
enola-cli, run `ingest-docs` again to reindex.

---

## GGUF model library

Each instance keeps its library at `/srv/enola-ai/<name>/ollama/models-gguf/`
(inside the Ollama container's `/root/.ollama` volume, so files appear as
`/root/.ollama/models-gguf/<file>`). File names are validated against path
traversal; on registration the `.gguf` is uploaded to Ollama as a blob
(identified by its SHA-256, not re-uploaded if already present) and the model
is created with `/api/create`, so `ask --instance <name>` can use them
directly. Ollama keeps its own copy in its blob store, so a registered model
takes twice the size of the library file on disk.

**Network privacy (R22):** downloads use **clearnet** by default. Use `--tor`
(or `[ai].use_tor_for_downloads = true` / `ENOLA_AI_USE_TOR_DOWNLOADS`) to force
the `socks5h://` proxy from `[http].tor_socks_proxy`; the CLI always reports
the egress route.

**HF tokenizer (offline fine-tuning):** when downloading from a HuggingFace
repository (`org/repo`), `ai model download` records the source repository
(`hf_repo`) in `index.json` and also downloads the tokenizer files on the host
to `/srv/enola-ai/<name>/ollama/models-gguf/tokenizer/<org__repo>/`. That way
`ai finetune start` can mount them as a volume without the (networkless)
training container downloading anything. If the tokenizer download fails, the
command warns but the GGUF download still counts as successful; you can supply
another tokenizer with `ai finetune start --tokenizer <org/repo>`.

---

## Models: `ai model` vs the Open WebUI panel

Each instance runs the official Open WebUI image, which ships its own model
manager (Admin → Settings → Models → *Pull a model from Ollama.com*) — the
visual equivalent of `ollama pull`. Both paths coexist, but they are **not
equivalent**:

| Path | Sources | Destination | Traceability in enola |
|------|---------|-------------|----------------------|
| Open WebUI panel (pull) | Ollama.com registry only | Ollama's internal store (`/root/.ollama/models`) | Only shown under `registered` in `ai model list` |
| `ai model download` / `import` / `add` | HuggingFace (`org/repo` + `--file`), direct URL, local `.gguf`, or the registry via `add` | Instance `models-gguf/` library + `/api/create` registration | `library` + `registered`: file, size, status and SHA-256 |

> **⚠️ Important nuance:** a model pulled from the Open WebUI panel
> **bypasses enola's library**. It works with `ask --instance` and shows up
> under `registered` in `ai model list`, but it is **not** in `library`: no
> file in `models-gguf/`, no recorded SHA-256, no documented origin. For full
> provenance always use `ai model download`/`import`/`add`.

The `ai model` path adds what the panel cannot offer: downloading any `.gguf`
from HuggingFace or a direct URL (not just models published on ollama.com),
`--sha256` verification, GGUF header validation before publishing the file,
`Range` resume from `<file>.part`, optional Tor egress (`--tor`) and `--name` aliases. Each operation is
also a scriptable, auditable command (exposed via MCP), rather than a manual
click.

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
