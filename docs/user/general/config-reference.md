> **Documento usuario:** `docs/user/general/config-reference.md`
> **Versión:** 1.8 | **Actualizado:** 2026-10-25
> **Estado:** ✅ **VIGENTE — Guía de configuración**
> **Referencias:** commands.md

# ⚙️ Referencia de Configuración — `config.toml`

Guía de formato del archivo de configuración centralizada de Enola CLI.

**Ubicación:** `~/.enola/config.toml`
**Permisos obligatorios:** `chmod 0600 ~/.enola/config.toml`
**Plantilla:** `config.example.toml` (en la raíz del repositorio)

---

## Prioridad de resolución

Los valores se resuelven en este orden (alta → baja):

1. **Flags CLI** — ej. `--web-url`, `--binary-base-url`, `--tor-socks`
2. **Variables de entorno** — ej. `ENOLA_WEB_URL`, `ENOLA_BINARY_BASE_URL`
3. **`~/.enola/config.toml`** — este archivo
4. **Defaults del binario** — valores hardcodeados en el CLI

Verificar con: `enola-cli config-show` (muestra el valor efectivo y su fuente).

---

## Como root (`sudo`)

Cuando el CLI corre como root, el entorno del usuario no se considera de
confianza (`sudo -E` lo entrega entero):

- **Variables de entorno:** solo se conservan `ENOLA_ACCEPT_LICENSE`,
  `ENOLA_MCP_CHILD` y `ENOLA_AUTO_SETUP`. Cualquier otra `ENOLA_*` heredada se
  ignora con **un** aviso que lista sus nombres (nunca sus valores). `PATH` se fija
  a `/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`.
- **Flags:** se respetan siempre (los teclea el operador). `--tor-socks` con un
  proxy que no está en `127.0.0.1`/`::1`/`localhost` muestra un aviso.
- **`~/.enola/config.toml`:** se sigue leyendo para claves de comodidad (`[ai].model`,
  `[ai].timeout_secs`, `[backup]`, `[web]`, `[http].download_timeout_secs`…). Las
  claves que deciden **a dónde se conecta root, qué verifica o por dónde enruta**
  solo se leen de `/etc/enola/config.toml` (root-owned, no escribible por grupo/otros):
  `[ai].endpoint`, `[ai].tor_proxy`, `[distribution].*`, `[update].*`, `[mcp].*` y un
  `[http].tor_socks_proxy` que no sea loopback. Si están en la config del usuario se
  ignoran con aviso.
- Para cambiar como root algo que antes ponías en una variable: usa el flag
  equivalente o `/etc/enola/config.toml`. Los overrides de límites de contenedor
  por variable de entorno no tienen equivalente: como root se usan los límites por
  defecto.

---

## Sección `[web]`

URLs públicas del proyecto. Se usan en mensajes del CLI (quickref, errores).

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `web_public_url` | String | (vacío) | `ENOLA_WEB_URL` | URL pública de la web del proyecto |
| `docs_url` | String | (vacío) | `ENOLA_DOCS_URL` | URL de documentación (opcional) |

```toml
[web]
web_public_url = "https://github.com/user/enola-cli-lite"
# docs_url = "https://github.com/user/enola-cli-lite#docs"
```

> Si se dejan vacías, el CLI no muestra enlaces en mensajes.

---

## Sección `[distribution]`

Configuración de descarga de binarios para `update download`.

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `binary_base_url` | String | (relativo a web) | `ENOLA_BINARY_BASE_URL` | URL base para descargar releases |
| `minisign_pubkey_url` | String | (embebida) | `ENOLA_MINISIGN_PUBKEY_URL` | URL de clave pública minisign |

```toml
[distribution]
binary_base_url = "https://github.com/user/enola-cli-lite/releases/latest/download"
minisign_pubkey_url = "https://github.com/user/enola-cli-lite/releases/latest/download/minisign.pub"
```

---

## Sección `[update]`

Feed de advisories y actualizaciones de seguridad.

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `feed_url` | String | (vacío) | `ENOLA_UPDATE_FEED_URL` | URL del feed JSON de advisories |
| `signature_url` | String | `{feed_url}.minisig` | — | URL de la firma minisign del feed |
| `minisign_pubkey` | String | (embebida) | — | Clave pública minisign para verificar el feed |

```toml
[update]
feed_url = "https://example.com/releases/advisories.json"
# signature_url = "https://example.com/releases/advisories.json.minisig"
# minisign_pubkey = "RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e"
```

> El feed puede vivir en clearweb o en `.onion`. Si usas `.onion`, el CLI
> lo enruta automáticamente por Tor vía `[http].tor_socks_proxy`.
>
> 🔒 Con euid 0 (`sudo`) la sección `[update]` solo se lee de
> `/etc/enola/config.toml` (root-owned, no escribible por grupo/otros) —
> `~/.enola/config.toml` se ignora para evitar que un fichero de usuario
> altere el feed o el trust anchor de un proceso root. Lo mismo aplica a
> `trusted_minisign_keys.json`.

---

## Sección `[http]`

Proxy SOCKS5 para peticiones a URLs `.onion`.

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `tor_socks_proxy` | String | `socks5h://127.0.0.1:9050` | `ENOLA_TOR_SOCKS_PROXY` | Proxy SOCKS5h para Tor |

> **Bajo `sudo`:** `ENOLA_TOR_SOCKS_PROXY` se ignora (ver "Como root"); el `config.toml` del
> usuario solo admite `socks5h://` en `127.0.0.1`/`::1`/`localhost` (si no, se ignora con
> aviso). Un proxy en otro host va en `/etc/enola/config.toml` (root-owned) o con `--tor-socks`.

```toml
[http]
tor_socks_proxy = "socks5h://127.0.0.1:9050"
# Tor Browser local: socks5h://127.0.0.1:9150
# Tor remoto LAN:    socks5h://10.0.0.5:9050
```

> El esquema `socks5h://` (con "h") delega la resolución DNS a Tor — crítico
> para no filtrar DNS fuera del circuito.

---

## Sección `[backup]`

Política de retención de backups del sistema (`maintenance backup`, backups
previos a `wp update`, etc.).

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `max_backups` | Integer | `5` | `ENOLA_MAX_BACKUPS` | Backups conservados por servicio (mínimo 1) |

```toml
[backup]
max_backups = 5
```

> También se puede fijar sin editar el archivo:
> `sudo enola-cli maintenance backup-config --max-backups 3`
> o sobrescribir por ejecución: `maintenance backup --keep N`.

---

## Sección `[mcp]`

Política de escritura y límites del servidor MCP (`enola-cli mcp`).

| Clave | Tipo | Default | Env var / Flag | Descripción |
|-------|------|---------|----------------|-------------|
| `allow_write` | Bool | `false` | `ENOLA_MCP_ALLOW_WRITE` / `--allow-write` | Habilita tools mutantes |
| `allowed_tools` | Array/CSV | `[]` | `ENOLA_MCP_ALLOWED_TOOLS` / `--allow-tools` | Vacía = todas las mutantes; si no, nombres (`wp_create`) o rutas (`wp:create`; `family` pelado para hojas como `doctor`) |
| `max_output_bytes` | Integer | `65536` | `ENOLA_MCP_MAX_OUTPUT_BYTES` / `--max-output-bytes` | Límite de stdout/stderr por llamada |
| `audit_log` | String | `stderr` | `ENOLA_MCP_AUDIT_LOG` / `--audit-log` | `"stderr"` o path absoluto de fichero (nunca stdout). Como root solo directamente en `/var/log/enola/` o `/var/lib/enola/` (si no, `mcp` no arranca) |
| `request_timeout_secs` | Integer | `60` | `ENOLA_MCP_REQUEST_TIMEOUT_SECS` / `--request-timeout-secs` | Timeout por llamada a tool |

```toml
[mcp]
# allow_write = false          # solo lectura (recomendado)
# allowed_tools = []           # vacía = TODAS las mutantes con allow_write
# max_output_bytes = 65536
# audit_log = "stderr"
# request_timeout_secs = 60
```

> ⚠️ `allowed_tools = []` con `allow_write = true` habilita **todas** las
> tools mutantes — es una decisión explícita. El audit log registra cada
> llamada mutante con args saneados (secretos redactados).
>
> ℹ️ Independientemente de la allowlist, las tools **destructivas**
> (`wp_publish`, `wp_hide`, `tor_create`, `tor_remove`, `files_create`, y
> vía `enola_run_command` todo `* delete|hide`, `tor create|edit|rotate`,
> `tor auth add|generate|disable|revoke|rotate`, `vpn create`,
> `vpn peer add|add-pubkey|remove`, `* publish`, `firewall setup|allow|deny`,
> `apparmor setup|mode`, `git create|registration`, `git user create|delete`,
> `maintenance backup|cleanup|disable-checks|ssh-harden-pqc`,
> `update download`) exigen una **confirmación de dos pasos**
> (`confirm_token` monouso, 300 s, vinculado a los args) — no es
> configurable. Y `try_run: true` simula cualquier mutante sin efectos,
> con la misma política aplicada.
>
> 🚫 `enola_run_command` nunca despacha las familias `mcp`, `ask`, `ssh`,
> `uninstall`, `web`, `setup` ni las rutas `update apply`, `test run`,
> `test benchmark` — con ninguna política ni allowlist.
>
> 🔒 Con euid 0 la sección `[mcp]` solo se lee de
> `/etc/enola/config.toml` (root-owned, no escribible por grupo/otros) —
> el `~/.enola/config.toml` del usuario se ignora para la política
> privilegiada. Flags y env vars mantienen precedencia sobre el fichero,
> así que un servicio root debe fijar su argv en un lanzador root-owned
> (unit de systemd o sudoers sin comodines): una regla
> `enola-cli mcp *` no fija política alguna.

---

## Sección `[ai]`

Configuración del asistente de IA local (`enola-cli ask`) conectado a Ollama.

| Clave | Tipo | Default | Env var / Flag | Descripción |
|-------|------|---------|----------------|-------------|
| `endpoint` | String | `http://127.0.0.1:11434` | `ENOLA_AI_ENDPOINT` / `--endpoint` | URL base del servidor Ollama (local o `.onion` vía `socks5h://`) |
| `model` | String | `llama3.1:8b` | `ENOLA_AI_MODEL` / `--model` | Modelo de Ollama usado en `enola-cli ask` |
| `timeout_secs` | Integer | `120` | `ENOLA_AI_TIMEOUT_SECS` | Timeout por petición a Ollama en segundos |
| `use_tor_for_downloads` | Boolean | `false` | `ENOLA_AI_USE_TOR_DOWNLOADS` / `--tor` | Si es `true`, `ai model download` sale por el proxy Tor SOCKS5h en lugar de clearnet |
| `tor_proxy` | Boolean | `false` | `ENOLA_AI_TOR_PROXY` / `--tor` | Si es `true`, las peticiones a providers externos (`ai provider test`, `ask --provider`) salen por Tor SOCKS5h |

```toml
[ai]
# endpoint = "http://127.0.0.1:11434"
# model = "llama3.1:8b"
# timeout_secs = 120
# use_tor_for_downloads = false
```

> **Bajo `sudo`:** `endpoint` y `tor_proxy` solo se leen de `/etc/enola/config.toml`
> (root-owned) o del flag; `ENOLA_AI_ENDPOINT` y `ENOLA_AI_TOR_PROXY` se ignoran. `enola-cli ask` imprime siempre el
> endpoint efectivo y su origen (`--endpoint`, `instance <n>`, `/etc/enola/config.toml`,
> `default`) y avisa si no está en esta máquina.

> Si se pasa `--instance <name>` en `enola-cli ask`, el CLI resuelve el puerto
> local del contenedor Ollama correspondiente (`enola-ai-<name>-ollama`). Si
> `endpoint` es una dirección `.onion`, las peticiones se enrutan automáticamente
> por Tor (`[http].tor_socks_proxy`).

---

## Variables solo-desarrollo / escape hatches

> **Solo para desarrollo y depuración.** Como root ninguna de estas variables tiene
> efecto salvo `ENOLA_AUTO_SETUP` (ver "Como root").

| Variable / flag | Efecto | Riesgo |
|---|---|---|
| `ENOLA_AUTO_SETUP=1` | En modo no-TTY (script/servicio) permite que el preflight instale dependencias (`apt-get`) automáticamente | Mutación del sistema sin consentimiento interactivo |
| `update download/apply --allow-unsigned` | Salta la verificación minisign del binario (solo el flag; la antigua `ENOLA_ALLOW_UNSIGNED_UPDATE` ya no existe) | Instala binario arbitrario como root |
| `ENOLA_SKIP_ROOT_CHECK=1` | Omite la exigencia de root (solo en builds `--features testing`) | Ausente en release |
| `ENOLA_MINISIGN_BIN` | Path alternativo al binario `minisign` (sin root) | Como root se ignora: se usa `minisign` del `PATH` seguro |
| `ENOLA_UPDATE_FEED_URL` / `ENOLA_UPDATE_SIGNATURE_URL` / `ENOLA_UPDATE_MINISIGN_PUBKEY` (+ `ENOLA_UPDATE_ENV_ANCHORS=1`) | Overrides de trust anchors de update (sin root) | Como root se ignoran: usa `[update]` en `/etc/enola/config.toml` |
| `ENOLA_TEST_TOKEN` | Salto de licencia en tests (feature `testing`, ausente en release) | Como root se ignora; los E2E usan `ENOLA_ACCEPT_LICENSE` |

El token del dashboard web ya no se puede fijar (`ENOLA_WEB_TOKEN` se eliminó): se
genera aleatorio en cada arranque y se guarda en `~/.enola/web-token`.

## Ejemplo completo

```toml
[web]
web_public_url = "https://github.com/user/enola-cli-lite"

[distribution]
binary_base_url = "https://github.com/user/enola-cli-lite/releases/latest/download"
minisign_pubkey_url = "https://github.com/user/enola-cli-lite/releases/latest/download/minisign.pub"

[update]
feed_url = "https://example.com/releases/advisories.json"

[http]
tor_socks_proxy = "socks5h://127.0.0.1:9050"

[backup]
max_backups = 5

[mcp]
allow_write = false
# allowed_tools = ["wp_create", "wp:publish"]
# max_output_bytes = 65536
# audit_log = "stderr"
# request_timeout_secs = 60

[ai]
endpoint = "http://127.0.0.1:11434"
model = "llama3.1:8b"
timeout_secs = 120
```

---

## Ver también

- [Comandos simples](commands-simple.md) — `config-show`, `config-validate`
- [Índice de comandos](commands.md) — catálogo completo
- [Conceptos](concepts.md) — arquitectura general

---
