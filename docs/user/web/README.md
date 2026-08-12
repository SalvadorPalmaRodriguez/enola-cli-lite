> **Documento usuario:** `docs/user/web/README.md`
> **Versión:** 2.0 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# Enola Web Dashboard

The web dashboard is a local GUI embedded in `enola-cli` for managing all your
services from a browser.

## Starting the Dashboard

```bash
sudo enola-cli web --port 8090
```

The server binds to `127.0.0.1` only (no remote access). A random token is
generated and printed in the terminal. Open `http://127.0.0.1:8090` in your
browser and enter the token to connect.

## Security Model

- **Token-based auth**: A 32-character token is generated at server start.
  All API requests must include it in the `Authorization` header.
- **Localhost only**: The server never binds to `0.0.0.0`.
- **Root required**: The server requires root privileges (same as CLI commands).

## Features

### Services
- View all services (Git, WordPress, Tor, CMS) in a unified table.
- See status, ports, and onion addresses at a glance.

### Tor Hidden Services
- List all Tor hidden services.
- Create, start, stop, remove, edit, rotate identities.
- Publish/hide services by type (git, wordpress, drupal, ghost, etc.).
- Manage client authorization (add, revoke, generate, rotate).

### Console (Universal CLI Access)
- Get help for any CLI command.
- Run arbitrary CLI commands from the web UI.

### Git Servers
- Create, start, stop, delete Git servers.
- Publish on Tor (with optional SSL).
- Hide from Tor.
- Manage registration, edit ports.
- Manage users (list, create, delete).
- Run pipeline watcher.

### WordPress Sites
- Create, start, stop, restart, delete WordPress sites.
- Publish on Tor, hide from Tor.
- Update, view config, view status.
- Edit ports and SSL.

### CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail)
- List, create, start, stop, delete, status, edit.
- Publish/hide on Tor.
- Strapi: build custom image.

### File Shares
- Create authenticated/SSL file shares.
- Delete, edit shares.
- Fix permissions.

### Ports
- View all ports in use by Enola services.

### Security
- View firewall status (UFW rules, Docker-User chain).
- Setup secure firewall defaults.
- Allow/deny specific ports.
- View AppArmor status (profiles, violations).
- Setup AppArmor base profiles.
- Change AppArmor mode (enforce/complain/disable).

### VPN (WireGuard)
- List, create, start, stop, delete VPN interfaces.
- View interface status (peers, transfer stats).
- Manage peers (add, add-pubkey, remove).

### Logs
- View system, Tor, Nginx, and Docker logs.
- View install and smoke-test logs.
- Configurable line count.

### Maintenance
- System status, smoke tests.
- Enable/disable health checks.
- Timer status, SSH config.
- SSH hardening (PQC).
- Backup and cleanup.

### Diagnostics
- Summary, Nginx, Tor, SSH, WordPress diagnostics.
- Nginx config test, resource monitoring.

### Test
- Run, list, benchmark, results, clean.

### Setup
- Install dependencies.
- PQC-TLS stack installation (SSE progress stream).

### System
- Quickref, license, config show/validate.
- Verify downloads, uninstall.
- Embedded docs access.

---

## API Endpoints

All endpoints under `/api/` require the `Authorization: <token>` header.

### Status & Services

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/status` | Server version and status |
| GET | `/api/services` | All services (aggregated) |

### Tor

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/tor` | List Tor hidden services |
| POST | `/api/tor/create` | Create Tor hidden service |
| POST | `/api/tor/{name}/start` | Start Tor service |
| POST | `/api/tor/{name}/stop` | Stop Tor service |
| POST | `/api/tor/{name}/remove` | Remove Tor service |
| POST | `/api/tor/{name}/edit` | Edit Tor service ports |
| GET | `/api/tor/{name}/detail` | Get Tor service details |
| POST | `/api/tor/{name}/rotate` | Rotate Tor identity |
| POST | `/api/tor/publish/{service_type}/{name}` | Publish service on Tor |
| POST | `/api/tor/hide/{service_type}/{name}` | Hide service from Tor |

### Tor Auth

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/tor/auth/{service}/list` | List authorized clients |
| POST | `/api/tor/auth/{service}/enable` | Enable client auth |
| POST | `/api/tor/auth/{service}/disable` | Disable client auth |
| POST | `/api/tor/auth/{service}/add` | Add authorized client |
| POST | `/api/tor/auth/{service}/revoke` | Revoke client |
| POST | `/api/tor/auth/generate` | Generate key pair |
| POST | `/api/tor/auth/{service}/rotate` | Rotate client keys |

### Console

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/console/help` | List all CLI commands |
| GET | `/api/console/help/{command}` | Help for specific command |
| POST | `/api/console/run` | Run CLI command |

### Git

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/git` | List Git servers |
| POST | `/api/git/create` | Create Git server |
| POST | `/api/git/{name}/start` | Start Git server |
| POST | `/api/git/{name}/stop` | Stop Git server |
| GET | `/api/git/{name}/status` | Git server status |
| POST | `/api/git/{name}/delete` | Delete Git server |
| POST | `/api/git/{name}/publish` | Publish Git on Tor |
| POST | `/api/git/{name}/hide` | Hide Git from Tor |
| POST | `/api/git/{name}/registration` | Toggle registration |
| GET | `/api/git/{name}/registration/status` | Registration status |
| POST | `/api/git/{name}/edit` | Edit Git ports |
| POST | `/api/git/user/list` | List Git users |
| POST | `/api/git/user/create` | Create Git user |
| POST | `/api/git/user/delete` | Delete Git user |
| POST | `/api/git/watcher` | Run pipeline watcher |

### WordPress

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/wp` | List WordPress sites |
| POST | `/api/wp/create` | Create WordPress site |
| POST | `/api/wp/{name}/start` | Start WordPress site |
| POST | `/api/wp/{name}/stop` | Stop WordPress site |
| POST | `/api/wp/{name}/restart` | Restart WordPress site |
| POST | `/api/wp/{name}/delete` | Delete WordPress site |
| POST | `/api/wp/{name}/publish` | Publish WordPress on Tor |
| POST | `/api/wp/{name}/hide` | Hide WordPress from Tor |
| POST | `/api/wp/{name}/update` | Update WordPress |
| GET | `/api/wp/{name}/config` | WordPress config |
| GET | `/api/wp/{name}/status` | WordPress status |
| POST | `/api/wp/{name}/edit` | Edit WordPress ports/SSL |

### CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/cms/{cms_type}/list` | List CMS instances |
| POST | `/api/cms/{cms_type}/create` | Create CMS instance |
| POST | `/api/cms/{cms_type}/{name}/start` | Start CMS instance |
| POST | `/api/cms/{cms_type}/{name}/stop` | Stop CMS instance |
| POST | `/api/cms/{cms_type}/{name}/delete` | Delete CMS instance |
| GET | `/api/cms/{cms_type}/{name}/status` | CMS instance status |
| POST | `/api/cms/{cms_type}/{name}/edit` | Edit CMS instance |
| POST | `/api/cms/{cms_type}/{name}/publish` | Publish CMS on Tor |
| POST | `/api/cms/{cms_type}/{name}/hide` | Hide CMS from Tor |
| POST | `/api/cms/strapi/build-image` | Build Strapi custom image |

### Files

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/files` | List file shares |
| POST | `/api/files/create` | Create file share |
| POST | `/api/files/{name}/delete` | Delete file share |
| POST | `/api/files/{name}/edit` | Edit file share |
| POST | `/api/files/{name}/fix-perms` | Fix file permissions |

### Ports

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/ports` | List all port usage |

### Doctor

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/doctor` | Run system diagnostics |

### Firewall

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/firewall/status` | Firewall status |
| POST | `/api/firewall/setup` | Setup secure firewall |
| POST | `/api/firewall/allow` | Allow port |
| POST | `/api/firewall/deny` | Deny port |

### AppArmor

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/apparmor/status` | AppArmor status |
| POST | `/api/apparmor/setup` | Setup AppArmor profiles |
| POST | `/api/apparmor/mode` | Change AppArmor mode |

### VPN

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/vpn/list` | List VPN interfaces |
| GET | `/api/vpn/status/{interface}` | VPN interface status |
| POST | `/api/vpn/create` | Create VPN interface |
| POST | `/api/vpn/{interface}/start` | Start VPN |
| POST | `/api/vpn/{interface}/stop` | Stop VPN |
| POST | `/api/vpn/{interface}/delete` | Delete VPN |
| POST | `/api/vpn/peer/add` | Add VPN peer |
| POST | `/api/vpn/peer/add-pubkey` | Add VPN peer by pubkey |
| POST | `/api/vpn/peer/remove` | Remove VPN peer |

### Logs

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/logs/sources` | List log sources |
| GET | `/api/logs/view?source={s}&lines={n}` | View logs |
| GET | `/api/logs/install` | View install logs |
| GET | `/api/logs/smoke-test` | View smoke test logs |

### Maintenance

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/maintenance/status` | System status |
| POST | `/api/maintenance/smoke-test` | Run smoke test |
| POST | `/api/maintenance/enable-checks` | Enable health checks |
| POST | `/api/maintenance/disable-checks` | Disable health checks |
| GET | `/api/maintenance/timer-status` | Timer status |
| GET | `/api/maintenance/ssh-config` | SSH config |
| POST | `/api/maintenance/ssh-harden-pqc` | SSH PQC hardening |
| POST | `/api/maintenance/backup` | Create backup |
| POST | `/api/maintenance/cleanup` | Cleanup system |

### Diagnostics

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/diag/summary` | Diagnostic summary |
| GET | `/api/diag/nginx` | Nginx diagnostics |
| GET | `/api/diag/tor` | Tor diagnostics |
| GET | `/api/diag/ssh` | SSH diagnostics |
| GET | `/api/diag/wordpress` | WordPress diagnostics |
| GET | `/api/diag/wp-sync` | WordPress sync status |
| GET | `/api/diag/nginx-test` | Nginx config test |
| GET | `/api/diag/resources` | Resource monitoring |

### Test

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/test/run` | Run tests |
| GET | `/api/test/list` | List available tests |
| POST | `/api/test/benchmark` | Run benchmarks |
| GET | `/api/test/results` | Last test results |
| POST | `/api/test/clean` | Clean test artifacts |

### Setup

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/setup` | Install dependencies |
| GET | `/api/setup/pqc-tls` | PQC-TLS install (SSE stream) |

### System

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/quickref` | Quick reference |
| GET | `/api/license` | License text |
| GET | `/api/config/show` | Show configuration |
| POST | `/api/config/validate` | Validate configuration |
| POST | `/api/verify` | Verify download |
| POST | `/api/uninstall` | Uninstall Enola |
| GET | `/api/docs` | Embedded docs |

---

## Request Schemas (POST endpoints)

### `POST /api/tor/create`
```json
{
  "name": "my-service",
  "service_type": "web",
  "virtual_port": 80,
  "target_port": 8080,
  "ssl": false
}
```

### `POST /api/tor/{name}/edit`
```json
{
  "virtual_port": 80,
  "nginx_port": 15000,
  "target_port": 9000,
  "auto_ports": false
}
```

### `POST /api/tor/auth/{service}/add`
```json
{
  "client": "alice",
  "pubkey": "descriptor:x25519:..."
}
```

### `POST /api/tor/auth/generate`
```json
{
  "client": "alice"
}
```

### `POST /api/git/create`
```json
{
  "name": "my-repo",
  "ssl": false,
  "admin_user": "admin",
  "admin_password": "secret123"
}
```

### `POST /api/git/{name}/edit`
```json
{
  "http_port": 10001,
  "https_port": 10443,
  "ssh_port": 30001
}
```

### `POST /api/git/user/create`
```json
{
  "server": "my-repo",
  "username": "dev1",
  "email": "dev1@example.onion",
  "admin_user": "admin",
  "admin_password": "secret123"
}
```

### `POST /api/wp/create`
```json
{
  "name": "my-blog",
  "http_port": 8080
}
```

### `POST /api/wp/{name}/edit`
```json
{
  "http_port": 8081,
  "https_port": 8443,
  "ssl": true
}
```

### `POST /api/cms/{cms_type}/create`
```json
{
  "name": "my-site",
  "http_port": 8090
}
```

### `POST /api/cms/strapi/build-image`
```json
{
  "force": false
}
```

### `POST /api/files/create`
```json
{
  "name": "my-files",
  "auth": false,
  "ssl": false
}
```

### `POST /api/files/{name}/edit`
```json
{
  "port": 8091
}
```

### `POST /api/firewall/setup`
```json
{
  "ssh_port": 22,
  "force": false
}
```

### `POST /api/firewall/allow`
```json
{
  "port": 8080,
  "proto": "tcp",
  "from": "192.168.1.0/24"
}
```

### `POST /api/firewall/deny`
```json
{
  "port": 3306,
  "proto": "tcp"
}
```

### `POST /api/apparmor/setup`
```json
{
  "mode": "enforce",
  "force": false
}
```

### `POST /api/apparmor/mode`
```json
{
  "mode": "enforce",
  "profile": "enola-wordpress"
}
```

### `POST /api/vpn/create`
```json
{
  "interface": "wg0",
  "port": 51820,
  "subnet": "10.0.0.0/24",
  "autostart": false,
  "sync_firewall": true
}
```

### `POST /api/vpn/{interface}/delete`
```json
{
  "sync_firewall": true
}
```

### `POST /api/vpn/peer/add`
```json
{
  "interface": "wg0",
  "peer_name": "laptop",
  "endpoint": "203.0.113.1:51820",
  "allowed_ips": "10.0.0.2/32"
}
```

### `POST /api/vpn/peer/add-pubkey`
```json
{
  "interface": "wg0",
  "peer_name": "server2",
  "public_key": "abc123...",
  "endpoint": "10.0.0.5:51820",
  "allowed_ips": "10.0.0.3/32"
}
```

### `POST /api/vpn/peer/remove`
```json
{
  "interface": "wg0",
  "public_key": "abc123..."
}
```

### `POST /api/console/run`
```json
{
  "args": ["git", "list"],
  "timeout_secs": 30
}
```

### `POST /api/maintenance/ssh-harden-pqc`
```json
{
  "force": false,
  "dry_run": true
}
```

### `POST /api/maintenance/cleanup`
```json
{
  "target": "all",
  "dry_run": true,
  "force": false
}
```

### `POST /api/test/run`
```json
{
  "filter": "tor"
}
```

---

## Response Schemas (key endpoints)

### `GET /api/status`
```json
{
  "version": "1.4.0",
  "status": "ok"
}
```

### `POST /api/tor/auth/generate`
```json
{
  "public_key": "descriptor:x25519:ABC123...",
  "private_key": "descriptor:x25519:DEF456...",
  "message": "Client 'alice' key pair generated"
}
```

### `POST /api/tor/{name}/edit`
```json
{
  "message": "Ports updated for 'my-service'",
  "warning": null,
  "applied": true
}
```

### `POST /api/console/run`
```json
{
  "stdout": "Service list...",
  "stderr": "",
  "exit_code": 0
}
```

### `GET /api/console/help`
```json
{
  "commands": ["tor", "git", "wp", "drupal", "ghost", "..."],
  "help": "Enola CLI - Manage Tor hidden services..."
}
```

---

## HTTP Error Codes

| Code | Meaning |
|------|---------|
| 200 | OK — request succeeded |
| 401 | Unauthorized — token missing or invalid |
| 404 | Not found — resource doesn't exist |
| 500 | Internal server error — message in response body |

---

## SSE Endpoint

### `GET /api/setup/pqc-tls`

This endpoint returns Server-Sent Events (SSE) for real-time progress of the
PQC-TLS stack installation. Each event contains a JSON object with progress
updates, build logs, and completion status.

---

## Ver también

- [Índice de comandos](../general/commands.md) — catálogo completo de comandos CLI.
- [Conceptos](../general/concepts.md) — arquitectura general.
- [Referencia de configuración](../general/config-reference.md) — formato de `config.toml`.

---
