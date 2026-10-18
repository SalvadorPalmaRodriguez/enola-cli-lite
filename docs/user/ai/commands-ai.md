> **Documento usuario:** `docs/user/ai/commands-ai.md`
> **Versión:** 1.0 | **Actualizado:** 2026-10-18
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
