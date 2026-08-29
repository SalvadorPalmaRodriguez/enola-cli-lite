> **Documento usuario:** `docs/user/general/commands-simple.md`
> **Versión:** 2.1 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🛠️ Comandos simples — Setup, Doctor, Config, Test, Docs y más

Comandos top-level que no pertenecen a una familia de servicios específica.
Incluye instalación de dependencias, verificación del sistema, configuración,
licencia, verificación de descargas y desinstalación.

---

## `setup`

Instala dependencias del sistema (Docker, Nginx, Tor, WireGuard, UFW, AppArmor).

```bash
sudo enola-cli setup [--all] [--vpn] [--security] [--pqc-tls]
```

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--all` | Bool | `false` | Instala TODAS las dependencias (core + vpn + security) |
| `--vpn` | Bool | `false` | Instala solo dependencias VPN (wireguard-tools) |
| `--security` | Bool | `false` | Instala solo herramientas de seguridad (UFW, AppArmor) |
| `--pqc-tls` | Bool | `false` | Instala OpenSSL 3.5.x + Nginx con stack PQC |

**Ejemplos:**
```bash
sudo enola-cli setup              # dependencias core
sudo enola-cli setup --all        # todo
sudo enola-cli setup --vpn        # solo VPN
sudo enola-cli setup --security   # solo seguridad
sudo enola-cli setup --pqc-tls    # stack PQC TLS
```

---

## `doctor`

Verifica las dependencias del sistema — muestra qué está instalado y qué falta.

```bash
sudo enola-cli doctor [--security]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--security` | Bool | Ejecuta auditoría de seguridad: hardening de contenedores, configs Nginx, AppArmor, UFW, secrets en env vars |

---

## `config-show`

Muestra la configuración centralizada resuelta con su fuente (flag > env > file > default).
Los valores sensibles se muestran como `[REDACTED]`.

```bash
enola-cli config-show [--json]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--json` | Bool | Salida en formato JSON |

---

## `config-validate`

Valida la configuración centralizada. Ejecuta comprobaciones: TOML parseable,
permisos 0600, sintaxis de URLs, Tor disponible si hay `.onion`, y opcionalmente
alcanzabilidad HTTP.

```bash
enola-cli config-validate [--reachable] [--json]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--reachable` | Bool | Comprueba alcanzabilidad HTTP de URLs (más lento) |
| `--json` | Bool | Salida en formato JSON |

> Devuelve exit code 1 si hay errores. Los warnings no bloquean.

---

## `test run`

Ejecuta tests del sistema.

```bash
sudo enola-cli test run [--filter <FILTRO>]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--filter` / `-f` | String | Filtro de test (opcional) |

---

## `test list`

Lista los tests disponibles.

```bash
sudo enola-cli test list
```

---

## `test benchmark`

Ejecuta benchmarks.

```bash
sudo enola-cli test benchmark
```

---

## `test results`

Muestra los últimos resultados de tests.

```bash
sudo enola-cli test results
```

---

## `test clean`

Limpia artefactos de tests.

```bash
sudo enola-cli test clean
```

---

## `docs`

Consulta la documentación de uso directamente en el terminal. Funciona offline.

```bash
enola-cli docs <SUBCOMANDO>
```

**Subcomandos disponibles:**

| Subcomando | Descripción |
|------------|-------------|
| `quickstart` | Guía de inicio rápido |
| `commands [GRUPO]` | Referencia de comandos |
| `concepts [TEMA]` | Conceptos clave |
| `faq [TÉRMINO]` | Preguntas frecuentes |
| `examples [CASO]` | Ejemplos de uso |
| `search TÉRMINO` | Buscar en toda la documentación |
| `quantum-security` | Guía PQC |
| `verify-downloads` | Verificación de descargas |
| `security` | Modelo de seguridad orientado al usuario |
| `install-from-iso` | Guía de instalación desde ISO |

---

## `quickref`

Muestra referencia rápida: comandos Docker vs equivalentes Enola CLI.

```bash
enola-cli quickref
```

Sin flags ni argumentos.

---

## `license`

Muestra el texto completo de la licencia del software. Funciona offline.

```bash
enola-cli license
```

Sin flags ni argumentos.

---

## `verify`

Verifica que una descarga de Enola es legítima. Comprueba la firma
post-cuántica ML-DSA-65 (FIPS 204) con la clave pública embebida en el binario.
No requiere red ni herramientas externas.

```bash
enola-cli verify <ARCHIVO> [--pqsig <PATH>] [--pubkey <PATH>] [--json]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<ARCHIVO>` | String | Sí | Path al archivo descargado a verificar |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--pqsig` | String | Path a la firma `.pqsig` (default: `<FILE>.pqsig`) |
| `--pubkey` | String | Clave pública ML-DSA alternativa (default: embebida) |
| `--json` | Bool | Salida en formato JSON |

**Ejemplos:**
```bash
enola-cli verify enola-cli-v0.1.2-alpha-x86_64-linux.tar.gz
enola-cli verify mybinary --pqsig mybinary.pqsig --json
```

---

## `uninstall`

Desinstala Enola CLI del sistema. Borra binario, servicios, contenedores,
configs de Tor/Nginx/AppArmor/UFW/systemd y datos (`/srv/enola-*`, `/opt/enola/`).
Por defecto ejecuta en modo dry-run.

```bash
sudo enola-cli uninstall [--yes] [--keep-data] [--only <SECCIONES>] [--force]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--yes` | Bool | Confirmar y ejecutar el borrado (sin esto es dry-run) |
| `--keep-data` | Bool | Conserva datos de servicios (`/srv/enola-*` y `config.toml`) |
| `--only` | String | Solo secciones indicadas (coma-separadas): `binary,config,services,tor,nginx,systemd,apparmor,docker,ufw,data,deps` |
| `--force` | Bool | Continuar ante errores no críticos (servicios no instalados) |
| `--remove-deps` | Bool | Desinstalar dependencias de terceros que Enola instaló (según manifiesto) |

**Ejemplos:**
```bash
sudo enola-cli uninstall                        # dry-run (no borra)
sudo enola-cli uninstall --yes                  # borrar todo
sudo enola-cli uninstall --yes --keep-data      # preserva /srv y config
sudo enola-cli uninstall --yes --only tor,nginx # solo estas secciones
sudo enola-cli uninstall --yes --remove-deps        # borrar todo + dependencias
```

---

## Ver también

- [Referencia de comandos](commands.md) — catálogo completo de comandos.
- [Conceptos](concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.
- [Referencia de configuración](config-reference.md) — formato de `config.toml`.

---
