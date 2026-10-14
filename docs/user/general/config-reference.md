> **Documento usuario:** `docs/user/general/config-reference.md`
> **Versión:** 1.4 | **Actualizado:** 2026-10-14
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
| `allowed_tools` | Array/CSV | `[]` | `ENOLA_MCP_ALLOWED_TOOLS` / `--allow-tools` | Vacía = todas las mutantes; si no, nombres (`wp_create`) o rutas (`wp:create`; `family` pelado para hojas como `doctor`) |
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
> 🚫 `enola_run_command` nunca despacha las familias `mcp`, `ssh`,
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
