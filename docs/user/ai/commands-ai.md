> **Documento usuario:** `docs/user/ai/commands-ai.md`
> **Versión:** 1.4 | **Actualizado:** 2026-10-18
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands-ask.md, ../general/commands.md, ../general/config-reference.md, ../mcp/commands-mcp.md

# 🧠 `ai` — Instancias de IA local (Ollama + Open WebUI)

`enola-cli ai` despliega y gestiona instancias aisladas de IA local compuestas por
**Ollama** (`enola-ai-<name>-ollama`) y **Open WebUI** (`enola-ai-<name>-webui`),
bindeadas exclusivamente a `127.0.0.1`, con passthrough opcional de GPU NVIDIA y
publicación en la red Tor (`.onion`) a través de Nginx.

Las instancias creadas con `ai create` son descubiertas automáticamente por
`enola-cli ask --instance <name>`.

---

## Subcomandos

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

# Biblioteca de modelos (nivel 3)
sudo enola-cli ai model list [--instance <NAME>]
sudo enola-cli ai model download <NAME> <org/repo|--url> [--file <F.gguf>]
                                 [--sha256 <HEX>] [--name <ALIAS>] [--tor] [--no-register]
sudo enola-cli ai model import <NAME> <PATH.gguf> [--name <ALIAS>] [--no-register]
sudo enola-cli ai model add <NAME> <model|file.gguf> [--name <ALIAS>]
sudo enola-cli ai model delete <NAME> <model|file.gguf> --force
sudo enola-cli ai model validate <NAME> <file.gguf>
sudo enola-cli ai model refresh <NAME>

# Índice RAG (nivel 3)
sudo enola-cli ai rag status <NAME>
sudo enola-cli ai rag ingest <NAME> <PATH>
sudo enola-cli ai rag bind <NAME> --source-type <git|filesystem|wordpress> --path <P> --force
sudo enola-cli ai rag sync <NAME>
sudo enola-cli ai rag config <NAME> [--chunk-size N] [--chunk-overlap N]
                                [--remove-code] [--preset docs|code|faq|large]
                                [--vector-store chroma|qdrant] [--show]

# Benchmark local (nivel 2)
sudo enola-cli ai benchmark <NAME> [--model <M>] [--prompt <P>] [--runs <N>]

# Providers cloud BYOK (nivel 3)
enola-cli ai provider list
enola-cli ai provider status <PROVIDER>
enola-cli ai provider set <PROVIDER> [--model <M>] [--endpoint <URL>]
                                    [--key-stdin] [--disable]
enola-cli ai provider remove <PROVIDER> --force
enola-cli ai provider test <PROVIDER> [--accept-risks] [--tor]

# ask contra provider externo (BYOK — los datos salen de tu máquina)
enola-cli ask "pregunta" --provider openai [--model <M>] [--accept-risks] [--tor]

# Fine-tuning LoRA (nivel 3)
enola-cli ai finetune prepare <FILE.jsonl> [--format <F>] [--out <DIR>] [--val-ratio <N>]
enola-cli ai finetune validate <FILE.jsonl> [--base-model <M>]
sudo enola-cli ai finetune start <NAME> --dataset <FILE.jsonl> [--base-model <M>]
                                [--low-memory] [--no-register] --force
sudo enola-cli ai finetune status <NAME>
enola-cli ai finetune wizard
```

| Subcomando | Clase MCP | Descripción |
|------------|-----------|-------------|
| `ai create` | `Mutating` | Crea la red aislada `enola_net_ai_<name>` y levanta `enola-ai-<name>-ollama` y `enola-ai-<name>-webui` en `127.0.0.1`. |
| `ai list` | `ReadOnly` | Lista todas las instancias IA gestionadas, sus puertos locales, estado GPU y dirección `.onion`. |
| `ai status <NAME>` | `ReadOnly` | Muestra el estado detallado de los contenedores Ollama y Open WebUI de la instancia. |
| `ai start <NAME>` | `Mutating` | Inicia los contenedores de una instancia detenida. |
| `ai stop <NAME>` | `Mutating` | Detiene los contenedores de una instancia en ejecución. |
| `ai delete <NAME> --force` | `Destructive` | Elimina contenedores, red Docker, datos en `/srv/enola-ai/<name>` y servicio `.onion`. Exige `--force`. |
| `ai publish <NAME> [--verify]` | `Destructive` | Publica la interfaz Open WebUI como servicio oculto `.onion` vía Nginx + Tor (y verifica alcanzabilidad SOCKS5h con `--verify`). |
| `ai hide <NAME>` | `Destructive` | Retira el servicio `.onion` y la configuración Nginx de la instancia, conservando los contenedores locales. |
| `ai logs <NAME> [--lines N]` | `ReadOnly` | Muestra las últimas `N` líneas de logs de Ollama y Open WebUI (también accesible mediante `enola-cli logs view ai-<NAME>`). |
| `ai model list [--instance N]` | `ReadOnly` | Lista los `.gguf` de la biblioteca de cada instancia y los modelos registrados en su Ollama (`/api/tags`). |
| `ai model download <N> <SRC>` | `Mutating` | Descarga un `.gguf` desde HuggingFace (`org/repo` + `--file`) o una URL directa, con streaming, reanudación `Range` y verificación SHA-256; registra el modelo vía `/api/create` salvo `--no-register`. Por defecto sale por **clearnet**; `--tor` o `[ai].use_tor_for_downloads` lo enrutan por SOCKS5h y la ruta se informa explícitamente. |
| `ai model import <N> <PATH>` | `Mutating` | Copia un `.gguf` local a `/srv/enola-ai/<N>/ollama/models-gguf/` tras validar su header, con permisos `0640`, y lo registra salvo `--no-register`. |
| `ai model add <N> <MODEL>` | `Mutating` | Si `MODEL` termina en `.gguf` lo registra desde la biblioteca (`/api/create` + Modelfile `FROM`); si no, lo descarga del registro Ollama (`/api/pull`, p.ej. `llama3.1:8b` o `hf.co/org/repo:tag`). |
| `ai model delete <N> <M> --force` | `Destructive` | Elimina un `.gguf` de la biblioteca (sufijo `.gguf`) o un modelo del registro Ollama (`DELETE /api/delete`). Exige `--force`. |
| `ai model validate <N> <F>` | `ReadOnly` | Parsea el header GGUF (magic + versión v2/v3 + contadores) y reporta familia/cuantización inferidas. |
| `ai model refresh <N>` | `Mutating` | Re-escanea la biblioteca y el registro de la instancia. |
| `ai rag status <N>` | `ReadOnly` | Estado del índice RAG: vector store, documentos indexados y fuentes ligadas. |
| `ai rag ingest <N> <PATH>` | `Mutating` | Ingere un fichero o directorio en el índice vectorial de la instancia (canonicaliza la ruta, rechaza rutas del sistema, salta dotfiles y escapes por symlink). |
| `ai rag bind <N> -s <T> -p <P> --force` | `Destructive` | Liga una fuente (`git`, `filesystem`, `wordpress`) al índice para `sync`. Exige `--force`; las rutas se canonicalizan y se rechazan prefijos del sistema (`/etc`, `/root`, `/proc`…). |
| `ai rag sync <N>` | `Mutating` | Re-ingiere todas las fuentes ligadas (`git pull --ff-only` como usuario invocador para fuentes git; REST `/wp-json/wp/v2/posts` para wordpress). |
| `ai rag config <N> [--show]` | `Mutating` | Muestra o edita `rag_config.json` (chunking, presets `docs|code|faq|large`, backend `chroma|qdrant`). |
| `ai benchmark <N>` | `Mutating` | Mide tokens/s (eval_duration de Ollama), latencia media y tamaño del modelo sobre la instancia (`--runs`, `--model`, `--prompt`). |
| `ai provider list` | `ReadOnly` | Lista providers BYOK y su estado (la key solo aparece como `(set)`/`(not set)`). |
| `ai provider status <P>` | `ReadOnly` | Estado de un provider concreto (sin revelar la key). |
| `ai provider set <P>` | `Mutating` | Configura clave/modelo/endpoint. La key se lee por prompt TTY seguro o `--key-stdin` — **nunca por argv**. |
| `ai provider remove <P> --force` | `Destructive` | Borra la entrada del provider de `~/.enola/providers.env`. Exige `--force`. |
| `ai provider test <P>` | `Mutating` | Ping mínimo al provider: envía datos fuera → exige consentimiento (`--accept-risks` o prompt `[s/N]` en TTY; sin TTY falla cerrado). |
| `ask --provider <P>` | — | Chat single-shot por el provider externo **sin tool registry** (el cloud no ejecuta comandos enola). Mismo gate de privacidad. |
| `ai finetune prepare <F>` | `Mutating` | Valida un dataset JSONL (`alpaca\|chatml\|sharegpt\|text`, auto-detectado), convierte a ChatML canónico y genera `train.jsonl`/`val.jsonl` con split determinista (`--val-ratio`). |
| `ai finetune validate <F>` | `Mutating` | Pre-flight: esquema + tamaño + VRAM (`HardwareProbePort`) + modelo base — antes de gastar horas de cómputo. |
| `ai finetune start <N> --dataset <F> --force` | `Destructive` | Lanza el contenedor `enola-ai-<n>-finetune-<job>` (`enola-finetune:latest`, 16 GiB, GPU si hay, read-only rootfs, sin puertos/red) con el script LoRA embebido. Dataset debe ser ChatML. Exige `--force`. |
| `ai finetune status <N>` | `ReadOnly` | Lista los jobs registrados de la instancia (`finetune/jobs.json`). |
| `ai finetune wizard` | `Mutating` | Asistente interactivo TTY (vetado para `enola_run_command`). |

---

## Fine-tuning (`ai finetune`)

El pipeline convierte datasets JSONL a ChatML canónico (`prepare`), valida el
job antes de gastar cómputo (`validate` — esquema, tamaño, VRAM vía
`HardwareProbePort`, modelo base) y lanza el entrenamiento en un contenedor
aislado (`start` — `enola-ai-<n>-finetune-<job>` sobre `ContainerPort`, sin
puertos ni red, GPU si está disponible, modo `--low-memory` para <10 GB VRAM).

El script LoRA (`train_lora.py` con transformers+peft) va **embebido** en el
binario (`include_str!`) y se despliega al directorio privado
`/srv/enola-ai/<n>/finetune/<job>/scripts/` — nunca a rutas predecibles de
`/tmp` (SEC-001/N-01). Los jobs se registran en `finetune/jobs.json` (`0640`).

---

## Providers externos BYOK (`ai provider` / `ask --provider`)

Los providers cloud (`openai`, `anthropic`, `grok`, `groq`, `openrouter`) son
**el único punto del CLI donde los datos del usuario salen de su máquina**.
Por eso están **desactivados por defecto** y cumplen garantías estrictas:

- **Consentimiento previo** a cualquier envío: prompt TTY `[s/N]` explícito;
  en scripts/CI se requiere `--accept-risks` (sin TTY falla cerrado).
- **Claves en `~/.enola/providers.env`** (respeta `SUDO_USER`), escritura
  atómica + permisos `0600`, validadas contra caracteres de control.
  Jamás se muestran en salidas, logs, errores ni `config show`.
- **La key nunca viaja por argv** — prompt TTY (sin eco) o `--key-stdin`.
- **Ruta informada**: clearnet por defecto; `--tor`, `ENOLA_AI_TOR_PROXY` o
  `[ai].tor_proxy = true` la fuerza por SOCKS5h (y la salida lo indica).
- **`ask --provider` es single-shot sin tools**: el cloud no puede ejecutar
  comandos `enola-cli` (superficie de fuga mínima por diseño).

---

## Índice RAG (vector store por instancia)

Cada instancia puede tener un contenedor de vector store propio
(`enola-ai-<name>-chromadb` o `enola-ai-<name>-qdrant`), aprovisionado de forma
perezosa por `ContainerPort` en `enola_net_ai_<name>` con bind `127.0.0.1`,
puerto del rango `12001–12500` reservado con `flock`, volumen persistente
`/srv/enola-ai/<name>/<backend>/` y límites calibrados (1 GiB, 512 pids).

Los embeddings se generan en el Ollama de la instancia (`/api/embed`, modelo
`nomic-embed-text` por defecto); si Ollama no responde se usa un embedding
determinista por hash para no bloquear la ingesta. El estado por instancia
vive en `/srv/enola-ai/<name>/rag_config.json` y `rag_sources.json` (`0640`).

La tool MCP `docs_lookup` ofrece búsqueda semántica sobre la colección
`enola_docs` de una instancia con RAG aprovisionado (alternativa opcional a
`docs_search`).

---

## Biblioteca de modelos GGUF

Cada instancia mantiene su biblioteca en `/srv/enola-ai/<name>/ollama/models-gguf/`
(dentro del volumen `/root/.ollama` del contenedor Ollama: los ficheros se ven
como `/root/.ollama/models-gguf/<file>`). Los nombres de fichero se validan
contra path traversal y los modelos quedan registrados en Ollama con un
`Modelfile` `FROM` apuntando al fichero, por lo que `ask --instance <name>`
puede usarlos directamente.

**Privacidad de red (R22):** las descargas salen por **clearnet** por defecto.
Usa `--tor` (o `[ai].use_tor_for_downloads = true` / `ENOLA_AI_USE_TOR_DOWNLOADS`)
para forzar el proxy `socks5h://` de `[http].tor_socks_proxy`; el CLI informa
siempre de la ruta de salida.

---

## Flags de `ai create`

| Flag | Descripción |
|------|-------------|
| `-n`, `--name <NAME>` | Nombre de la instancia (minúsculas, dígitos y guiones; máx. 63 caracteres). |
| `-m`, `--model <MODEL>` | Modelo inicial de Ollama (por defecto: `llama3.1:8b`). |
| `--api-port <PORT>` | Puerto en `127.0.0.1` para la API HTTP de Ollama (por defecto: auto `11435–12000`). |
| `--webui-port <PORT>` | Puerto en `127.0.0.1` para Open WebUI (por defecto: auto `8081–8200`). |
| `--no-gpu` | Fuerza ejecución en modo CPU aunque exista GPU NVIDIA disponible. |
| `--memory-gb <GIB>` | Límite de RAM en GiB para el contenedor Ollama (mínimo y por defecto: `8` GiB). |
| `--no-pull` | Omite la descarga inicial (`/api/pull`) del modelo al crear la instancia. |

---

## Hardening y aislamiento

- **Bind exclusivo a `127.0.0.1`**: tanto el puerto API de Ollama como el de Open WebUI escuchan únicamente en loopback local.
- **Límites calibrados (R20)**: el contenedor Ollama se configura con al menos `8 GiB` de RAM y `pids_limit >= 4096` (hilos BLAS/CUDA) para evitar muertes por OOM (`exit 137`), manteniendo `no-new-privileges:true` y `cap_drop: ["ALL"]`.
- **Reserva de puertos con `flock`**: los puertos se reservan mediante `acquire_port_lock` antes de crear contenedores y cuentan con rollback completo si falla algún paso intermedio.
- **Detección GPU segura**: reutiliza `EnolaHardwareProbe` (NVML + fallback `nvidia-smi` con reducción de privilegios) y degrada automáticamente a CPU con advertencia si no hay GPU NVIDIA disponible.

---

## Ejemplos

```bash
# Crear instancia IA local con modelo por defecto (llama3.1:8b)
sudo enola-cli ai create --name dev --model llama3.1:8b

# Consultar estado en formato JSON
sudo enola-cli --format json ai status dev

# Usar el asistente CLI contra la instancia creada
enola-cli ask "lista mis servicios tor" --instance dev

# Publicar Open WebUI en Tor y verificar que la dirección .onion responde
sudo enola-cli ai publish dev --verify

# Ver logs de la instancia (vía subcomando ai o vía logs view)
sudo enola-cli ai logs dev --lines 100
sudo enola-cli logs view ai-dev --lines 100

# Ocultar de Tor y eliminar la instancia
sudo enola-cli ai hide dev
sudo enola-cli ai delete dev --force
```

---

## Referencias cruzadas

- [Asistente local `enola-cli ask`](commands-ask.md)
- [Índice de comandos](../general/commands.md)
- [Referencia de configuración (`config.toml`)](../general/config-reference.md)
- [Servidor MCP (`enola-cli mcp`)](../mcp/commands-mcp.md)
