> **Documento usuario:** `docs/user/update/commands-update.md`
> **Versión:** 3.0 | **Actualizado:** 2026-08-02
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🔄 Update — Comandos `enola-cli update`

Feed de advisories y actualizaciones. Verifica la disponibilidad de updates
y aplica nuevas versiones del binario.

> Verificación de updates: SHA256 obligatorio (integridad) + **minisign obligatorio** (anti-compromiso del servidor).
> Instalación de minisign vía `sudo apt install minisign` (o equivalente), NO desde la release propia.
>
> **Importante:** La firma minisign es ahora **obligatoria** para descargar y aplicar updates.
> Si minisign no está instalado o la firma no verifica, la descarga/actualización se **rechaza**.
> Para override en entornos de testing: `--allow-unsigned` o `ENOLA_ALLOW_UNSIGNED_UPDATE=1`.

---

## `update check`

Comprueba si hay actualizaciones y security advisories.

Fetcha el advisory feed (`ENOLA_UPDATE_FEED_URL` o `[update].feed_url` en config.toml),
compara con la versión actual, y muestra si hay updates o advisories.

```bash
sudo enola-cli update check [--json] [--force]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--json` | Bool | Salida en formato JSON |
| `--force` | Bool | Ignora caché y fuerza una comprobación fresca |

**Exit codes estables para scripts/CI:**

| Code | Significado |
|------|-------------|
| `0` | OK (incluye update disponible sin advisory crítico) |
| `11` | Advisory crítico afecta a la versión actual |
| `12` | Versión actual por debajo de `min_supported` |
| `20` | Feed inválido/no parseable/no alcanzable |
| `21` | Firma minisign inválida o ausente |

---

## `update schema`

Muestra el schema del advisory feed (para operadores que crean su propio feed).

```bash
sudo enola-cli update schema [--json]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--json` | Bool | Salida en formato JSON |

---

## `update verify-feed`

Verifica un advisory feed firmado manualmente desde URL o path local.

```bash
sudo enola-cli update verify-feed <FUENTE> [--signature <URL>] [--json]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<FUENTE>` | String | Sí | URL http/https o path local del feed |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--signature` | String | Path/URL de la firma (default: `<source>.minisig`) |
| `--json` | Bool | Salida en formato JSON |

**Ejemplos:**
```bash
sudo enola-cli update verify-feed web/releases/advisories.json
sudo enola-cli update verify-feed https://host/advisories.json --json
```

---

## `update download`

Descarga el último binario del update feed.

Verifica SHA256 y **firma minisign obligatoria**. El binario se guarda en un directorio temporal.

> **Importante:** Si la firma minisign no verifica (o minisign no está instalado),
> la descarga se **rechaza** con error. El binario se guarda para inspección
> pero no se aplica automáticamente. Usar `--allow-unsigned` solo en testing.

```bash
sudo enola-cli update download [--yes] [--dry-run] [--json] [--force] [--allow-unsigned]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--yes` | Bool | También aplica el update (reemplaza el binario actual) |
| `--dry-run` | Bool | Muestra qué pasaría sin descargar ni aplicar |
| `--json` | Bool | Salida en formato JSON |
| `--force` | Bool | Fuerza comprobación fresca del feed (ignora caché) |
| `--allow-unsigned` | Bool | Permite descargar sin verificación minisign (**peligroso**, solo testing) |

---

## `update apply`

Aplica un update descargado previamente.

Reemplaza el binario actual atómicamente: hace backup del binario antiguo a
`/usr/local/share/enola/enola-cli.bak`, mueve el nuevo binario, y actualiza
`cli.sha256`. Requiere root.

> **Importante:** Si el binario descargado no fue verificado con minisign
> (metadata `signature_verified: false`), `apply` se **rechaza**.
> Usar `--allow-unsigned` solo en entornos de testing.

```bash
sudo enola-cli update apply [--binary <PATH>] [--json] [--allow-unsigned]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--binary` | String | Path al binario descargado (si se omite, usa la última descarga de `update download`) |
| `--json` | Bool | Salida en formato JSON |
| `--allow-unsigned` | Bool | Permite aplicar sin verificación minisign (**peligroso**, solo testing) |

---

## Rotación de clave minisign

El feed de advisories puede anunciar una **nueva clave minisign** mediante el campo
opcional `next_pubkey`. Esto permite al operador rotar la clave de firma sin romper
clientes existentes.

**Cómo funciona:**

1. El operador genera una nueva clave minisign y la firma con la clave **actual**.
2. Publica el par `next_pubkey: { key, signature }` en el feed.
3. El cliente verifica la firma de la nueva clave con la clave actual.
4. Si es válida, persiste la nueva clave en `~/.enola/trusted_minisign_keys.json` (0600).
5. Las verificaciones posteriores usan la nueva clave (prioridad: env var > config.toml > claves persistidas > embebida).

**Si la firma de la nueva clave no verifica:** el cliente ignora la rotación y muestra un warning.

**Prioridad de resolución de clave minisign:**

1. `ENOLA_UPDATE_MINISIGN_PUBKEY` (env var)
2. `[update].minisign_pubkey` en `~/.enola/config.toml`
3. `~/.enola/trusted_minisign_keys.json` (claves persistidas vía rotación)
4. Clave embebida en el binario (trust anchor inicial)

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Verificar descargas](../verify/verify-downloads.md) — verificación de firmas de releases.

---

## Variables de entorno del instalador (`install.sh`)

| Variable | Descripción |
|----------|-------------|
| `ENOLA_INSTALL_BASE_URL` | URL base donde están los artefactos de release |
| `ENOLA_INSTALL_VERSION` | Versión a instalar (default: `latest`) |
| `ENOLA_INSTALL_PREFIX` | Prefix de instalación (default: `/usr/local`) |
| `ENOLA_INSTALL_PUBKEY` | Clave pública minisign (override del trust anchor — solo desarrollo/testing) |
| `ENOLA_INSTALL_STRICT_PUBKEY` | Si `1`, aborta cuando `ENOLA_INSTALL_PUBKEY` difiere del default |
| `ENOLA_INSTALL_NO_VERIFY` | Si `1`, salta verificación minisign (DESACONSEJADO) |
| `ENOLA_MINISIGN_BIN` | Path al binario minisign (default: `minisign` en PATH). Si es un path absoluto, se valida que el binario exista y sea minisign de verdad |

> **Nota**: Si se establece `ENOLA_INSTALL_PUBKEY` con un valor distinto del default,
> el instalador emite un warning visible. En producción, no usar esta variable;
> instalar minisign via `sudo apt install minisign` y usar el trust anchor del proyecto.

---

## Variables de entorno del mecanismo de update

| Variable | Descripción |
|----------|-------------|
| `ENOLA_UPDATE_FEED_URL` | URL del advisory feed (override del default) |
| `ENOLA_UPDATE_SIGNATURE_URL` | URL de la firma del feed (default: `<feed>.minisig`) |
| `ENOLA_UPDATE_MINISIGN_PUBKEY` | Clave pública minisign (override, máxima prioridad) |
| `ENOLA_ALLOW_UNSIGNED_UPDATE` | Si `1`, permite descargar/aplicar sin firma minisign (**peligroso**) |
| `ENOLA_HTTP_DOWNLOAD_TIMEOUT` | Timeout en segundos para descargas grandes (default: 300) |
| `ENOLA_TOR_SOCKS_PROXY` | Proxy Tor SOCKS5h para URLs .onion (default: `socks5h://127.0.0.1:9050`) |

---

## Seguridad del mecanismo de actualización

### Validación de `ENOLA_MINISIGN_BIN`

Si se establece `ENOLA_MINISIGN_BIN` con un path absoluto o reluto (contiene `/`),
el CLI valida antes de usarlo:

1. **Existencia**: el binario debe existir en el path especificado.
2. **Ejecutable**: el binario debe ser ejecutable.
3. **Identity check**: ejecuta `<bin> -V` y verifica que el output contenga
   la palabra "minisign" (confirma que es minisign de verdad, no un binario
   malicioso con el mismo nombre).

Si cualquiera de las validaciones falla, el CLI emite un warning y fallback
a `minisign` del PATH (instalado via `apt install minisign`).

Si `ENOLA_MINISIGN_BIN` es un nombre simple (sin `/`), se usa directamente
via PATH lookup (sin validación adicional).

### Trust anchor del instalador

El instalador (`install.sh`) usa `ENOLA_INSTALL_PUBKEY` como trust anchor para
verificar la firma minisign del binario descargado. El valor default es la
clave pública del proyecto embebida en el script.

- Si `ENOLA_INSTALL_PUBKEY` se establece con un valor distinto del default,
  el instalador emite un **warning visible** a stderr.
- `ENOLA_INSTALL_STRICT_PUBKEY=1` hace que el instalador **aborte** si se
  usa un pubkey custom (modo CI/producción).
- En producción: no usar `ENOLA_INSTALL_PUBKEY`. Instalar minisign via
  `sudo apt install minisign` y confiar en el trust anchor del proyecto.

---
