> **Documento usuario:** `docs/user/general/commands.md`
> **Versión:** 3.3 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Índice de comandos**
> **English:** [`docs/en/commands.md`](../../en/commands.md)

# Enola CLI — Índice de Comandos

```
sudo enola-cli [--format text|json] [--verbose] <COMANDO> <SUBCOMANDO> [OPCIONES]
```

Cada grupo tiene su propia referencia detallada con sintaxis, flags, argumentos y ejemplos.
Este documento es el **índice maestro** — usa los enlaces para acceder al detalle.

**Desde el terminal** (offline, sin abrir archivos) el detalle de cada familia se lee con:

```bash
sudo enola-cli docs commands <GRUPO>     # ej: tor, git, wp, vpn, firewall
sudo enola-cli docs commands             # este índice
sudo enola-cli docs search <TÉRMINO>     # buscar en toda la documentación
```

---

## Servicios

| Grupo | Prefijo | Descripción | Referencia |
|-------|---------|-------------|-----------|
| Tor | `tor` | Servicios ocultos Tor (create, list, edit, rotate, auth) | [tor/commands-tor.md](../tor/commands-tor.md) |
| Git | `git` | Servidores Git Forgejo (create, users, registration) | [git/commands-git.md](../git/commands-git.md) |
| WordPress | `wp` | Sitios WordPress + MariaDB | [wp/commands-wp.md](../wp/commands-wp.md) |
| Drupal | `drupal` | Sitios Drupal + MariaDB | [drupal/commands-drupal.md](../drupal/commands-drupal.md) |
| Ghost | `ghost` | Blogs Ghost + SQLite (1 contenedor, ~256 MB) | [ghost/commands-ghost.md](../ghost/commands-ghost.md) |
| Magnolia | `magnolia` | CMS Java Magnolia (Tomcat, ≥4 GB RAM) | [magnolia/commands-magnolia.md](../magnolia/commands-magnolia.md) |
| Strapi | `strapi` | Headless CMS Strapi (Node + Postgres) | [strapi/commands-strapi.md](../strapi/commands-strapi.md) |
| Wagtail | `wagtail` | CMS Wagtail (Python/Django + Postgres) | [wagtail/commands-wagtail.md](../wagtail/commands-wagtail.md) |
| Files | `files` | Servidores de archivos anónimos via Tor | [files/commands-files.md](../files/commands-files.md) |

---

## Seguridad y red

| Grupo | Prefijo | Descripción | Referencia |
|-------|---------|-------------|-----------|
| Firewall | `firewall` | Gestión UFW (setup, status, allow, deny) | [firewall/commands-firewall.md](../firewall/commands-firewall.md) |
| AppArmor | `apparmor` | Sandboxing de procesos (setup, status, mode) | [apparmor/commands-apparmor.md](../apparmor/commands-apparmor.md) |
| VPN | `vpn` | Túneles WireGuard (create, peer, status) | [vpn/commands-vpn.md](../vpn/commands-vpn.md) |
| Ports | `ports` | Ver puertos usados por todos los servicios | [ports/commands-ports.md](../ports/commands-ports.md) |

---

## Sistema

| Grupo | Prefijo | Descripción | Referencia |
|-------|---------|-------------|-----------|
| Setup | `setup` | Instalar dependencias del sistema | [setup/commands-setup.md](../setup/commands-setup.md) |
| Doctor | `doctor` | Verificar dependencias y seguridad | [commands-simple.md](commands-simple.md#doctor) |
| Config | `config-show`/`config-validate` | Inspeccionar y validar configuración | [commands-simple.md](commands-simple.md#config-show) |
| Maintenance | `maintenance` | Mantenimiento, backups, cleanup, SSH hardening | [maintenance/commands-maintenance.md](../maintenance/commands-maintenance.md) |
| Diagnostics | `diag` | Diagnósticos y salud (nginx, tor, ssh, resources) | [diag/commands-diag.md](../diag/commands-diag.md) |
| Logs | `logs` | Ver y gestionar logs del sistema | [logs/commands-logs.md](../logs/commands-logs.md) |
| Update | `update` | Feed de advisories y actualizaciones del binario | [update/commands-update.md](../update/commands-update.md) |
| Test | `test` | Tests del sistema (run, list, benchmark) | [test/commands-test.md](../test/commands-test.md) |
| Docs | `docs` | Documentación embebida en el binario (offline) | [commands-simple.md](commands-simple.md#docs) |

---

## Utilidades

| Grupo | Prefijo | Descripción | Referencia |
|-------|---------|-------------|-----------|
| Quickref | `quickref` | Referencia rápida Docker ↔ Enola | [commands-simple.md](commands-simple.md#quickref) |
| License | `license` | Texto completo de la licencia | [commands-simple.md](commands-simple.md#license) |
| Verify | `verify` | Verificar autenticidad de descargas (PQC) | [commands-simple.md](commands-simple.md#verify) |
| Uninstall | `uninstall` | Desinstalación segura del CLI | [commands-simple.md](commands-simple.md#uninstall) |
| Web | `web` | Dashboard web local (GUI) | [web/README.md](../web/README.md) |

---

## Ver también

- [Conceptos](concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.
- [Comandos simples](commands-simple.md) — doctor, config, docs, quickref, license, verify, uninstall.

---
