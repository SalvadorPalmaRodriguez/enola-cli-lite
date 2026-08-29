> **Documento usuario:** `docs/user/general/config-reference.md`
> **Versión:** 1.0 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de configuración**

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
```

---

## Ver también

- [Comandos simples](commands-simple.md) — `config-show`, `config-validate`
- [Índice de comandos](commands.md) — catálogo completo
- [Conceptos](concepts.md) — arquitectura general

---
