> **User document:** `docs/en/commands-apparmor.md`
> **Version:** 1.0 | **Updated:** 2026-09-20
> **Status:** ✅ **CURRENT — User guide**
> **Spanish original:** [`docs/user/apparmor/commands-apparmor.md`](../user/apparmor/commands-apparmor.md)
> **References:** [commands.md](commands.md)

# 🛡️ AppArmor — `enola-cli apparmor` commands

Service sandboxing with AppArmor. Loads base profiles (nginx, tor, docker)
and per-service profiles (created automatically with `git/wp create`).

---

## Two-tier model

Enola uses AppArmor at two levels: **base profiles** for the system daemons
(loaded with `apparmor setup`) and **per-service profiles** for Docker
containers (created automatically by `wp create` and `git create`).
The confinement chain for a request is:

```
Internet/Tor → tor (enola-tor) → nginx (enola-nginx) → service container (per-service profile)
```

| Tier | Confined process | Profile | Created by |
|------|-------------------|---------|------------|
| Base | `nginx` daemon | `enola-nginx` | `apparmor setup` |
| Base | system `tor` daemon | `enola-tor` | `apparmor setup` |
| Base | Docker containers (base) | `enola-docker-base` | `apparmor setup` |
| Per-service | WordPress container `wp-<name>` | `enola-wp-<name>` | `wp create` |
| Per-service | Forgejo container `enola-git-<name>` | `enola-git-<name>` | `git create` |

Notes:

- `tor create` does **not** create a per-service profile: all onion services
  share the single `tor` daemon, already confined by `enola-tor`. AppArmor
  confines processes, not configurations.
- The `db-<name>` container (WordPress MariaDB) has no profile of its own:
  it falls back to Docker's `docker-default` profile.
- The per-service profile is only injected as `security_opt` if it is loaded
  in the kernel; otherwise the container starts with `docker-default`
  (degrades silently, e.g. on WSL2).
- Per-service profiles are always created in `complain` mode (log only).

---

## `apparmor setup`

Loads the base AppArmor profiles (nginx, tor, docker-base).

```bash
sudo enola-cli apparmor setup [--mode <MODE>] [--force]
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--mode` | String | `complain` | Mode: `complain` (log only) or `enforce` (block + log) |
| `--force` / `-f` | Bool | `false` | Skip the confirmation prompt |

> Recommended: start with `complain`, switch to `enforce` after validating.

**Examples:**
```bash
sudo enola-cli apparmor setup
sudo enola-cli apparmor setup --mode enforce
sudo enola-cli apparmor setup --force
```

---

## `apparmor status`

Shows AppArmor status: installed, enabled, loaded profiles, and violations.

```bash
sudo enola-cli apparmor status
```

No flags or arguments.

---

## `apparmor mode`

Changes the mode of AppArmor profiles (enforce/complain/disable).

```bash
sudo enola-cli apparmor mode [--enforce] [--complain] [--disable] [--profile <PROFILE>]
```

| Flag | Type | Description |
|------|------|-------------|
| `--enforce` | Bool | Block violations |
| `--complain` | Bool | Log only, do not block |
| `--disable` | Bool | Unload profile |
| `--profile` | String | Specific profile (default: all Enola profiles) |

> The `--enforce`, `--complain`, and `--disable` flags are mutually exclusive.

**Examples:**
```bash
sudo enola-cli apparmor mode --enforce
sudo enola-cli apparmor mode --complain --profile enola-git-myserver
sudo enola-cli apparmor mode --disable --profile enola-git-myserver
```

---

## See also

- [Command index](commands.md) — full command catalog.
- [Concepts](concepts.md) — general architecture (Tor, Nginx, Docker, secrets).
- [Quick start](quickstart.md) — first site in 5 minutes.

---
