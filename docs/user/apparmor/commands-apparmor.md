> **Documento usuario:** `docs/user/apparmor/commands-apparmor.md`
> **Versión:** 2.1 | **Actualizado:** 2026-09-20
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🛡️ AppArmor — Comandos `enola-cli apparmor`

Sandboxing de servicios con AppArmor. Carga perfiles base (nginx, tor, docker)
y perfiles por servicio (creados automáticamente con `git/wp create`).

---

## Modelo de dos niveles

Enola usa AppArmor en dos niveles: **perfiles base** para los daemons del
sistema (cargados con `apparmor setup`) y **perfiles por servicio** para los
contenedores Docker (creados automáticamente por `wp create` y `git create`).
La cadena de confinamiento de una petición es:

```
Internet/Tor → tor (enola-tor) → nginx (enola-nginx) → contenedor del servicio (perfil por servicio)
```

| Capa | Proceso confinado | Perfil | Quién lo crea |
|------|-------------------|--------|----------------|
| Base | daemon `nginx` | `enola-nginx` | `apparmor setup` |
| Base | daemon `tor` del sistema | `enola-tor` | `apparmor setup` |
| Base | contenedores Docker (base) | `enola-docker-base` | `apparmor setup` |
| Por servicio | contenedor WordPress `wp-<name>` | `enola-wp-<name>` | `wp create` |
| Por servicio | contenedor Forgejo `enola-git-<name>` | `enola-git-<name>` | `git create` |

Notas:

- `tor create` **no** crea perfil por servicio: todas las onion services
  comparten el único daemon `tor`, ya confinado por `enola-tor`. AppArmor
  confina procesos, no configuraciones.
- El contenedor `db-<name>` (MariaDB de WordPress) no lleva perfil propio:
  queda con el perfil `docker-default` de Docker.
- El perfil por servicio solo se inyecta como `security_opt` si está cargado
  en el kernel; si no, el contenedor arranca con `docker-default` (degrada
  silenciosamente, p.ej. en WSL2).
- Los perfiles por servicio se crean siempre en modo `complain` (solo log).

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
