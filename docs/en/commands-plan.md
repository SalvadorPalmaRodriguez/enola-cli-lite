> **User document:** `docs/en/commands-plan.md`
> **Version:** 1.0 | **Updated:** 2026-09-15
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
- **Risk level** (low/medium/high) computed statically from the plan.

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

> **Note:** `--admin-user`/`--admin-password` (present in `git create`)
> are omitted in `plan` because they do not affect ports, container,
> firewall, or AppArmor.

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
| `-s, --service-type <TYPE>` | `raw`, `web`, `static`, `files` | `web` |
| `-p, --virtual-port <PORT>` | Public .onion port | `80` |
| `-t, --target-port <PORT>` | Local app port (range 10000-20000) | auto |
| `--ssl` | HTTPS with self-signed certificate | false |

Tor is a systemd service (not a Docker container), so the "Containers"
section shows "(none — systemd service)". The `virtual-port` is a .onion
port (not a real socket) and is neither validated nor generates a UFW rule.

## JSON output

With `--format json`, the output is a serializable `ServicePlan` object.

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

## See also

- [Command index](commands.md)
- [`wp create`](../user/wp/commands-wp.md) · [`git create`](../user/git/commands-git.md) · [`tor create`](../user/tor/commands-tor.md)
- [`doctor --security`](../user/general/commands-simple.md#doctor)
