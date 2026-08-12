> **Documento usuario:** `docs/user/maintenance/commands-maintenance.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

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

Crea un backup del sistema.

```bash
sudo enola-cli maintenance backup
```

Sin flags ni argumentos.

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
