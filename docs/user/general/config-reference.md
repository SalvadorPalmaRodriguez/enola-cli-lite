> **Documento usuario:** `docs/user/general/config-reference.md`
> **Versión:** 1.2 | **Actualizado:** 2026-10-12
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

---

## Sección `[http]`

Proxy SOCKS5 para peticiones a URLs `.onion`.

| Clave | Tipo | Default | Env var | Descripción |
|-------|------|---------|---------|-------------|
| `tor_socks_proxy` | String | `socks5h://127.0.0.1:9050` | `ENOLA_TOR_SOCKS_PROXY` | Proxy SOCKS5h para Tor |

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
| `allowed_tools` | Array/CSV | `[]` | `ENOLA_MCP_ALLOWED_TOOLS` / `--allow-tools` | Vacía = todas las mutantes; si no, nombres (`wp_create`) o rutas (`wp:create`) |
| `max_output_bytes` | Integer | `65536` | `ENOLA_MCP_MAX_OUTPUT_BYTES` / `--max-output-bytes` | Límite de stdout/stderr por llamada |
| `audit_log` | String | `stderr` | `ENOLA_MCP_AUDIT_LOG` / `--audit-log` | `"stderr"` o path de fichero (nunca stdout) |
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

---

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
```

---

## Ver también

- [Comandos simples](commands-simple.md) — `config-show`, `config-validate`
- [Índice de comandos](commands.md) — catálogo completo
- [Conceptos](concepts.md) — arquitectura general

---
