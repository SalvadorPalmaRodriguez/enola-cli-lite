> **Documento usuario:** `docs/user/maintenance/commands-maintenance.md`
> **Versión:** 2.1 | **Actualizado:** 2026-09-19
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🔧 Maintenance — Comandos `enola-cli maintenance`

Operaciones de mantenimiento del sistema: estado, smoke tests, health checks,
hardening SSH, backups y limpieza.

---

## `maintenance status`

Muestra el estado general del sistema.

```bash
sudo enola-cli maintenance status
```

Sin flags ni argumentos.

---

## `maintenance smoke-test`

Ejecuta un smoke test del sistema.

```bash
sudo enola-cli maintenance smoke-test
```

Sin flags ni argumentos.

---

## `maintenance enable-checks`

Habilita los health checks automáticos.

```bash
sudo enola-cli maintenance enable-checks
```

Sin flags ni argumentos.

---

## `maintenance disable-checks`

Deshabilita los health checks automáticos.

```bash
sudo enola-cli maintenance disable-checks
```

Sin flags ni argumentos.

---

## `maintenance timer-status`

Muestra el estado del timer de systemd para los health checks automáticos.

```bash
sudo enola-cli maintenance timer-status
```

Sin flags ni argumentos.

---

## `maintenance ssh-config`

Configura el check de SSH.

```bash
sudo enola-cli maintenance ssh-config
```

Sin flags ni argumentos.

---

## `maintenance ssh-harden-pqc`

Endurece la configuración SSH con algoritmos post-cuánticos.
Añade `sntrup761x25519-sha512` KEX (OpenSSH 9.0+) como primer algoritmo preferido.

```bash
sudo enola-cli maintenance ssh-harden-pqc [--force] [--dry-run]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Aplica cambios sin confirmación |
| `--dry-run` | Bool | Muestra qué cambiaría sin aplicar |

> Medida transicional hasta que el PQC completo se estandarice. Ejecuta de nuevo tras actualizar OpenSSH.

---

## `maintenance backup`

Crea un backup del sistema (`/var/lib/tor`, `/etc/nginx/sites-available`,
`/opt/enola`) como `tar.gz` en `/var/backups/enola-server/`, preservando las
rutas absolutas para que la restauración recoloque cada elemento en su sitio.

```bash
sudo enola-cli maintenance backup [--keep <N>]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--keep` | usize | Conserva como máximo N backups por servicio solo en esta ejecución |

Tras crear el backup se rotan los antiguos del mismo identificador según la
política de retención (ver `maintenance backup-config`).

---

## `maintenance backup-config`

Muestra o fija la política de retención de backups (cuántos se conservan por
servicio antes de borrar los más antiguos).

```bash
# Ver el valor efectivo y su origen
sudo enola-cli maintenance backup-config

# Persistir un nuevo valor en ~/.enola/config.toml → [backup].max_backups
sudo enola-cli maintenance backup-config --max-backups 3
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--max-backups` | usize | Nº máximo de backups por servicio (mínimo 1). Omitir para solo consultar |

Prioridad de resolución (de mayor a menor): flag `--keep` de
`maintenance backup` > variable `ENOLA_MAX_BACKUPS` > `config.toml`
`[backup].max_backups` > valor por defecto (5).

> La configuración se guarda en el `~/.enola/config.toml` del usuario que
> invoca `sudo`, no en `/root/.enola/`, y conserva su propiedad.

---

## `maintenance cleanup`

Limpia archivos temporales y datos residuales.

```bash
sudo enola-cli maintenance cleanup [--target <TARGET>] [--dry-run] [--force] [--keep-days <DÍAS>]
```

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--target` / `-t` | String | `all` | Objetivo: `all`, `logs`, `docker` |
| `--dry-run` | Bool | `false` | Muestra qué se borraría sin borrar |
| `--force` / `-f` | Bool | `false` | Limpia sin confirmación |
| `--keep-days` | u32 | `7` | Días de logs a conservar |

**Ejemplos:**
```bash
sudo enola-cli maintenance cleanup --dry-run
sudo enola-cli maintenance cleanup --target logs --keep-days 30
sudo enola-cli maintenance cleanup --force
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
