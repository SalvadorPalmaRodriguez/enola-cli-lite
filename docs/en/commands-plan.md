> **User document:** `docs/en/commands-plan.md`
> **Version:** 1.2 | **Updated:** 2026-09-20
> **Status:** ✅ **CURRENT — `plan` command reference**
> **Spanish original:** [`docs/user/general/commands-plan.md`](../user/general/commands-plan.md)
> **References:** [commands.md](commands.md), [wp/commands-wp.md](../user/wp/commands-wp.md), [git/commands-git.md](../user/git/commands-git.md), [tor/commands-tor.md](../user/tor/commands-tor.md)

# `enola plan` — Service creation dry-run

```
sudo enola-cli plan <wp|tor|git> create [OPTIONS]
sudo enola-cli --format json plan <wp|tor|git> create [OPTIONS]
```

## Description

`plan` is a **dry-run**: it shows the change plan that **would** be applied
if you ran the corresponding `create` command, **WITHOUT modifying the system**
(0 side effects).

It shows:

- **Ports** to use (validated/auto-assigned via `PortValidator`).
- **Docker containers** to create (image, internal port, volumes).
- **Filesystem paths** to create.
- **UFW rules** that would be added (WITHOUT executing them).
- **AppArmor profile** that would be applied (WITHOUT running `aa-enforce`).
  Only `wp create` and `git create` create a per-service profile; `tor create`
  creates none and the plan reports `(none)`.
- **Risk level** (low/medium/high) computed statically from the plan.
- **Notes**: warnings about flags that `create` would ignore — the plan
  reports them in a `📝 Notes` section instead of pretending they take effect.

> **Requires root** (`sudo`): although it is a dry-run, port validation
> performs temporary TCP binds and the command is a configuration function
> that only operates once Enola is initialized.

## Subcommands

### `plan wp create`

Plans the creation of a WordPress site.

```bash
sudo enola-cli plan wp create --name <name> [--http-port <PORT>]
```

| Flag | Description | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Site name (lowercase, digits, hyphens) | (required) |
| `--http-port <PORT>` | Internal HTTP port (range 8080-9000) | auto |

**Example:**

```bash
sudo enola-cli plan wp create --name foo --http-port 8090
```

### `plan git create`

Plans the creation of a Git server (Forgejo).

```bash
sudo enola-cli plan git create --name <name> [--ssl] [--http-port <PORT>] [--ssh-port <PORT>]
```

| Flag | Description | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Server name | (required) |
| `--ssl` | Enable HTTPS with self-signed certificate | false |
| `--http-port <PORT>` | Internal HTTP port (range 10000-15000) | auto |
| `--ssh-port <PORT>` | Internal SSH port (range 30000-35000) | auto |

With `--ssl` the plan adds:

- an `https-port` auto-assigned in the **15001-20000** range;
- the Nginx site `/etc/nginx/sites-available/proxy_<name>` and the pair
  `/etc/nginx/ssl/<name>.crt` + `.key` (self-signed certificate);
- a **Note** reminding that `git create --ssl` does **NOT register the
  HTTPS port in UFW** (only `http-port` and `ssh-port` are synced).

The Forgejo container uses Docker's **default bridge** (the output shows
`network: (default bridge)`), not a dedicated network.

> **Note:** `--admin-user`/`--admin-password` (present in `git create`)
> are omitted in `plan` because they do not affect ports, container,
> firewall, or AppArmor.

**Example:**

```bash
sudo enola-cli plan git create --name repo --ssl
```

The risk level is **medium** (🟡) because the plan exposes SSH (even though
only on loopback).

### `plan tor create`

Plans the creation of a Tor hidden service.

```bash
sudo enola-cli plan tor create --name <name> [-s <type>] [-p <virtual-port>] [-t <target-port>] [--ssl]
```

| Flag | Description | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Service name | (required) |
| `-s, --service-type <TYPE>` | `raw`/`tcp`, `web`/`proxy`/`http`, `static`, `files`/`fileserver` | `web` |
| `-p, --virtual-port <PORT>` | Public .onion port (only honored by `raw`) | `80` |
| `-t, --target-port <PORT>` | Local port of your app (assumed already listening; not validated or auto-assigned) | see table |
| `--ssl` | HTTPS with self-signed certificate (`web` only) | false |

An unknown `--service-type` is an error (same message as `tor create`).

**Per-type behavior** (mirrors `tor create` exactly):

| Type | Planned ports | Paths |
|------|---------------|-------|
| `raw`/`tcp` | `virtual-port` (the one you pass) + `target-port` (`--target-port`, or the virtual port if omitted) | `/var/lib/tor/enola_<name>` + `/etc/tor/enola.d/<name>.conf` |
| `web`/`proxy`/`http` | `virtual-port` 80 + `nginx-port` auto (10000-20000) + `backend-port` (`--target-port`, default 8080). With `--ssl`: `virtual-port` 80 + `virtual-port-https` 443 + `nginx-http-port` auto (10000-15000) + `nginx-https-port` auto (15001-20000) + `backend-port` | same with `proxy_` prefix (`/var/lib/tor/enola_proxy_<name>`, `/etc/tor/enola.d/proxy_<name>.conf`) + `/etc/nginx/sites-available/proxy_<name>`; with `--ssl` also `/etc/nginx/ssl/<name>.crt` + `.key` |
| `static` | `virtual-port` 80 + `nginx-port` auto (20000-30000) | `/var/lib/tor/enola_<name>`, `/etc/tor/enola.d/<name>.conf`, `/etc/nginx/sites-available/<name>`, `/var/www/<name>` |
| `files`/`fileserver` | `virtual-port` 80 + `nginx-port` auto (20000-30000) | `fileserver_` prefix in Tor and Nginx + `/srv/enola-files/<name>` |

Real `tor create` limitations that the plan reports in `📝 Notes`:

- `--virtual-port` is only honored by `raw`; for `web`/`static`/`files` the
  `.onion` is always published on `:80` (and `:443` with `--ssl`).
- `static` and `files` ignore `--target-port` (the Nginx port is
  auto-assigned in 20000-30000) and also `--ssl` (only `web` supports HTTPS).
- UFW: `tor create` only registers `--target-port` when passed explicitly;
  Nginx ports are never registered.

**Example:**

```bash
sudo enola-cli plan tor create --name svc --service-type files --target-port 1234
```

**Output (text):**

```
📋 Plan: tor service 'svc'
──────────────────────────────────────────────────────────

🔌 Ports:
  • virtual-port → 127.0.0.1:80 (manual)
  • nginx-port → 127.0.0.1:20000 (auto-assigned (range 20000-30000))

📦 Containers: (none — systemd service)

📂 Filesystem:
  • /var/lib/tor/enola_fileserver_svc — Tor hidden service directory (debian-tor:debian-tor, 700)
  • /etc/tor/enola.d/fileserver_svc.conf — Tor hidden service config (root:debian-tor, 640)
  • /etc/nginx/sites-available/fileserver_svc — Nginx file-server config (autoindex)
  • /srv/enola-files/svc — Shared folder (root:www-data, 0750)

🛡 Firewall: (no rules)

🔒 AppArmor: (none — this command does not create a per-service profile)

⚠️  Risk: 🟢 low

📝 Notes:
  • `--target-port` is ignored for service type 'files': the Nginx port is auto-assigned in range 20000-30000.

💡 This is a dry-run — nothing was executed.
```

Tor is a systemd service (not a Docker container), so the "Containers"
section shows "(none — systemd service)". The `virtual-port` is a .onion
port (not a real socket) and is neither validated nor generates a UFW rule.

## JSON output

With `--format json`, the output is a serializable `ServicePlan` object.
It includes a `notes` array (warnings about ignored flags), and
`containers[].network` is `null` when the service uses Docker's default
bridge (e.g. Git/Forgejo) instead of a dedicated network. `apparmor` is
`null` for Tor services (`tor create` creates no profile).

## Risk level

Computed **statically** from the plan (does not inspect the live system;
for runtime auditing use `doctor --security`):

| Level | Condition |
|-------|-----------|
| 🟢 **low** | All on 127.0.0.1, ports ≥1024, no SSH exposed |
| 🟡 **medium** | Privileged port (<1024, except 80/443) or SSH exposed (Git) |
| 🔴 **high** | Any bind to 0.0.0.0 (defensive — never happens in Enola) |

## Difference from `doctor --security`

| | `plan` | `doctor --security` |
|---|-------|---------------------|
| When | Before creating a service | After, on existing services |
| What | Declarative plan (dry-run) | Live system audit |
| Side effects | 0 | 0 (read-only) |
| Risk | Static from plan | From real containers/configs |

## Web access

The local dashboard (`enola-cli web`, `127.0.0.1` only, token-protected)
exposes the same dry-runs as REST endpoints:

| Method | Path | Body |
|--------|------|------|
| POST | `/api/plan/wp/create` | `{ "name": "blog", "http_port": 8090? }` |
| POST | `/api/plan/git/create` | `{ "name": "repo", "ssl": true?, "http_port": ..., "ssh_port": ...? }` |
| POST | `/api/plan/tor/create` | `{ "name": "svc", "service_type": "web"?, "virtual_port": 80?, "target_port": ...?, "ssl": false? }` |

Fields marked `?` are optional and use the same defaults as the CLI
(`service_type = "web"`, `virtual_port = 80`, `ssl = false`). The response is
the serialized `ServicePlan` object — **the same JSON as `--format json`**
(see [JSON output](#json-output)), not a text rendering.

## See also

- [Command index](commands.md)
- [`wp create`](../user/wp/commands-wp.md) · [`git create`](../user/git/commands-git.md) · [`tor create`](../user/tor/commands-tor.md)
- [`doctor --security`](../user/general/commands-simple.md#doctor)
