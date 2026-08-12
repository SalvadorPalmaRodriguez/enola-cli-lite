> **Documento usuario:** `docs/user/logs/commands-logs.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 📝 Logs — Comandos `enola-cli logs`

Ver y gestionar logs del sistema.

---

## `logs list`

Lista las fuentes de logs disponibles.

```bash
sudo enola-cli logs list
```

Sin flags ni argumentos.

---

## `logs view`

Muestra logs de una fuente específica.

```bash
sudo enola-cli logs view <FUENTE> [--lines <N>] [--follow]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<FUENTE>` | String | Sí | Fuente de logs: `system`, `tor`, `nginx`, `docker`, etc. |

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--lines` / `-n` | usize | `50` | Número de líneas a mostrar |
| `--follow` / `-f` | Bool | `false` | Sigue la salida de logs en tiempo real |

**Ejemplos:**
```bash
sudo enola-cli logs view nginx
sudo enola-cli logs view tor --lines 100
sudo enola-cli logs view docker --follow
```

---

## `logs install`

Muestra los logs de instalación.

```bash
sudo enola-cli logs install
```

Sin flags ni argumentos.

---

## `logs smoke-test`

Muestra los logs del smoke test.

```bash
sudo enola-cli logs smoke-test
```

Sin flags ni argumentos.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
