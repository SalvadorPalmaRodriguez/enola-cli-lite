> **User document:** `docs/en/commands.md`
> **Version:** 3.3 | **Updated:** 2026-08-08
> **Status:** ✅ **CURRENT — Command Index**
> **Spanish original:** [`docs/user/general/commands.md`](../user/general/commands.md)

# Enola CLI — Command Index

```
sudo enola-cli [--format text|json] [--verbose] <COMMAND> <SUBCOMMAND> [OPTIONS]
```

Each group has its own detailed reference with syntax, flags, arguments, and examples.
This document is the **master index** — use the links to access details.

**From the terminal** (offline, without opening files) each family's details are available with:

```bash
sudo enola-cli docs commands <GROUP>     # e.g: tor, git, wp, vpn, firewall
sudo enola-cli docs commands             # this index
sudo enola-cli docs search <TERM>        # search all documentation
```

---

## Services

| Group | Prefix | Description | Reference |
|-------|--------|-------------|-----------|
| Tor | `tor` | Tor hidden services (create, list, edit, rotate, auth) | [tor/commands-tor.md](../user/tor/commands-tor.md) |
| Git | `git` | Forgejo Git servers (create, users, registration) | [git/commands-git.md](../user/git/commands-git.md) |
| WordPress | `wp` | WordPress sites + MariaDB | [wp/commands-wp.md](../user/wp/commands-wp.md) |
| Drupal | `drupal` | Drupal sites + MariaDB | [drupal/commands-drupal.md](../user/drupal/commands-drupal.md) |
| Ghost | `ghost` | Ghost blogs + SQLite (1 container, ~256 MB) | [ghost/commands-ghost.md](../user/ghost/commands-ghost.md) |
| Magnolia | `magnolia` | Magnolia Java CMS (Tomcat, ≥4 GB RAM) | [magnolia/commands-magnolia.md](../user/magnolia/commands-magnolia.md) |
| Strapi | `strapi` | Strapi headless CMS (Node + Postgres) | [strapi/commands-strapi.md](../user/strapi/commands-strapi.md) |
| Wagtail | `wagtail` | Wagtail CMS (Python/Django + Postgres) | [wagtail/commands-wagtail.md](../user/wagtail/commands-wagtail.md) |
| Files | `files` | Anonymous file servers via Tor | [files/commands-files.md](../user/files/commands-files.md) |

---

## Security and networking

| Group | Prefix | Description | Reference |
|-------|--------|-------------|-----------|
| Firewall | `firewall` | UFW management (setup, status, allow, deny) | [firewall/commands-firewall.md](../user/firewall/commands-firewall.md) |
| AppArmor | `apparmor` | Process sandboxing (setup, status, mode) | [apparmor/commands-apparmor.md](../user/apparmor/commands-apparmor.md) |
| VPN | `vpn` | WireGuard tunnels (create, peer, status) | [vpn/commands-vpn.md](../user/vpn/commands-vpn.md) |
| Ports | `ports` | View ports used by all services | [ports/commands-ports.md](../user/ports/commands-ports.md) |

---

## System

| Group | Prefix | Description | Reference |
|-------|--------|-------------|-----------|
| Setup | `setup` | Install system dependencies | [setup/commands-setup.md](../user/setup/commands-setup.md) |
| Doctor | `doctor` | Verify dependencies and security | [commands-simple.md](../user/general/commands-simple.md#doctor) |
| Config | `config-show`/`config-validate` | Inspect and validate configuration | [commands-simple.md](../user/general/commands-simple.md#config-show) |
| Maintenance | `maintenance` | Maintenance, backups, cleanup, SSH hardening | [maintenance/commands-maintenance.md](../user/maintenance/commands-maintenance.md) |
| Diagnostics | `diag` | Diagnostics and health (nginx, tor, ssh, resources) | [diag/commands-diag.md](../user/diag/commands-diag.md) |
| Logs | `logs` | View and manage system logs | [logs/commands-logs.md](../user/logs/commands-logs.md) |
| Update | `update` | Advisory feed and binary updates | [update/commands-update.md](../user/update/commands-update.md) |
| Test | `test` | System tests (run, list, benchmark) | [test/commands-test.md](../user/test/commands-test.md) |
| Docs | `docs` | Documentation embedded in binary (offline) | [commands-simple.md](../user/general/commands-simple.md#docs) |

---

## Utilities

| Group | Prefix | Description | Reference |
|-------|--------|-------------|-----------|
| Quickref | `quickref` | Quick reference Docker ↔ Enola | [commands-simple.md](../user/general/commands-simple.md#quickref) |
| License | `license` | Full license text | [commands-simple.md](../user/general/commands-simple.md#license) |
| Verify | `verify` | Verify download authenticity (PQC) | [commands-simple.md](../user/general/commands-simple.md#verify) |
| Uninstall | `uninstall` | Safe CLI uninstall | [commands-simple.md](../user/general/commands-simple.md#uninstall) |
| Web | `web` | Local web dashboard (GUI) | [web/README.md](../user/web/README.md) |

---

## See also

- [Concepts](concepts.md) — general architecture (Tor, Nginx, Docker, secrets).
- [Quick start](quickstart.md) — first site in 5 minutes.
- [FAQ](faq.md) — frequently asked questions.
