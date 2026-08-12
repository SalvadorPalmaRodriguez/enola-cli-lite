> **Documento usuario:** `docs/user/apparmor/commands-apparmor.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🛡️ AppArmor — Comandos `enola-cli apparmor`

Sandboxing de servicios con AppArmor. Carga perfiles base (nginx, tor, docker)
y perfiles por servicio (creados automáticamente con `git/wp create`).

---

## `apparmor setup`

Carga los perfiles base de AppArmor (nginx, tor, docker-base).

```bash
sudo enola-cli apparmor setup [--mode <MODO>] [--force]
```

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--mode` | String | `complain` | Modo: `complain` (solo log) o `enforce` (bloquear + log) |
| `--force` / `-f` | Bool | `false` | Omite el prompt de confirmación |

> Recomendado: empezar con `complain`, cambiar a `enforce` tras validar.

**Ejemplos:**
```bash
sudo enola-cli apparmor setup
sudo enola-cli apparmor setup --mode enforce
sudo enola-cli apparmor setup --force
```

---

## `apparmor status`

Muestra el estado de AppArmor: instalado, habilitado, perfiles cargados y violaciones.

```bash
sudo enola-cli apparmor status
```

Sin flags ni argumentos.

---

## `apparmor mode`

Cambia el modo de los perfiles AppArmor (enforce/complain/disable).

```bash
sudo enola-cli apparmor mode [--enforce] [--complain] [--disable] [--profile <PERFIL>]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--enforce` | Bool | Bloquear violaciones |
| `--complain` | Bool | Solo log, no bloquear |
| `--disable` | Bool | Descargar (unload) perfil |
| `--profile` | String | Perfil específico (default: todos los perfiles de Enola) |

> Los flags `--enforce`, `--complain` y `--disable` son mutuamente excluyentes.

**Ejemplos:**
```bash
sudo enola-cli apparmor mode --enforce
sudo enola-cli apparmor mode --complain --profile enola-git-myserver
sudo enola-cli apparmor mode --disable --profile enola-git-myserver
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
