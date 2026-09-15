> **Documento usuario:** `docs/user/general/commands-plan.md`
> **Versión:** 1.0 | **Actualizado:** 2026-09-15
> **Estado:** ✅ **VIGENTE — Referencia del comando `plan`**
> **English:** [`docs/en/commands-plan.md`](../../en/commands-plan.md)
> **Referencias:** [commands.md](commands.md), [wp/commands-wp.md](../wp/commands-wp.md), [git/commands-git.md](../git/commands-git.md), [tor/commands-tor.md](../tor/commands-tor.md)

# `enola plan` — Dry-run de creación de servicios

```
sudo enola-cli plan <wp|tor|git> create [OPCIONES]
sudo enola-cli --format json plan <wp|tor|git> create [OPCIONES]
```

## Descripción

`plan` es un **dry-run**: muestra el plan de cambios que **se aplicarían** si
ejecutaras el comando `create` correspondiente, **SIN modificar el sistema**
(0 efectos colaterales).

Muestra:

- **Puertos** a usar (validados/auto-asignados vía `PortValidator`).
- **Contenedores Docker** a crear (imagen, puerto interno, volúmenes).
- **Rutas de filesystem** a crear.
- **Reglas UFW** que se añadirían (SIN ejecutarlas).
- **Perfil AppArmor** que se aplicaría (SIN ejecutar `aa-enforce`).
- **Nivel de riesgo** (bajo/medio/alto) calculado estáticamente desde el plan.

> **Requiere root** (`sudo`): aunque es dry-run, la validación de puertos
> realiza binds TCP temporales y el comando es una función de configuración
> que solo opera cuando Enola está inicializado.

## Subcomandos

### `plan wp create`

Planifica la creación de un sitio WordPress.

```bash
sudo enola-cli plan wp create --name <name> [--http-port <PORT>]
```

| Flag | Descripción | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Nombre del sitio (minúsculas, dígitos, guiones) | (requerido) |
| `--http-port <PORT>` | Puerto HTTP interno (rango 8080-9000) | auto |

**Ejemplo:**

```bash
sudo enola-cli plan wp create --name foo --http-port 8090
```

**Salida (texto):**

```
📋 Plan: wordpress service 'foo'
──────────────────────────────────────────────────────────

🔌 Ports:
  • http-port → 127.0.0.1:8090 (manual)

📦 Containers:
  • wp-foo — image: wordpress:latest
    internal port: 80 (host: 8090)
    network: enola_net_foo
    volume: /srv/enola-wordpress/foo_wp → /var/www/html
  • db-foo — image: mariadb:10.6
    internal port: 3306
    network: enola_net_foo
    volume: /srv/enola-wordpress/foo_db → /var/lib/mysql

📂 Filesystem:
  • /srv/enola-wordpress/foo_wp — WordPress files (bind mount → /var/www/html)
  • /srv/enola-wordpress/foo_db — MariaDB data (bind mount → /var/lib/mysql)
  • /srv/enola-wordpress/foo_secrets — Secrets directory (0700, root:root)

🛡 Firewall (UFW — NOT applied):
  • allow 8090/tcp (loopback)

🔒 AppArmor (NOT applied):
  • profile: enola-wp-foo (mode: complain)

⚠️  Risk: 🟢 low

💡 This is a dry-run — nothing was executed.
```

### `plan git create`

Planifica la creación de un servidor Git (Forgejo).

```bash
sudo enola-cli plan git create --name <name> [--ssl] [--http-port <PORT>] [--ssh-port <PORT>]
```

| Flag | Descripción | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Nombre del servidor | (requerido) |
| `--ssl` | Habilitar HTTPS con cert autofirmado | false |
| `--http-port <PORT>` | Puerto HTTP interno (rango 10000-15000) | auto |
| `--ssh-port <PORT>` | Puerto SSH interno (rango 30000-35000) | auto |

> **Nota:** `--admin-user`/`--admin-password` (presentes en `git create`)
> se omiten en `plan` porque no afectan puertos, contenedor, firewall ni AppArmor.

**Ejemplo:**

```bash
sudo enola-cli plan git create --name repo --http-port 10500 --ssh-port 30100
```

El nivel de riesgo es **medio** (🟡) porque el plan expone SSH (aunque solo
en loopback).

### `plan tor create`

Planifica la creación de un servicio oculto Tor.

```bash
sudo enola-cli plan tor create --name <name> [-s <type>] [-p <virtual-port>] [-t <target-port>] [--ssl]
```

| Flag | Descripción | Default |
|------|-------------|---------|
| `-n, --name <NAME>` | Nombre del servicio | (requerido) |
| `-s, --service-type <TYPE>` | `raw`, `web`, `static`, `files` | `web` |
| `-p, --virtual-port <PORT>` | Puerto público .onion | `80` |
| `-t, --target-port <PORT>` | Puerto local de la app (rango 10000-20000) | auto |
| `--ssl` | HTTPS con cert autofirmado | false |

**Ejemplo:**

```bash
sudo enola-cli plan tor create --name svc --target-port 15000
```

Tor es un servicio systemd (no contenedor Docker), por lo que la sección
"Containers" muestra "(none — systemd service)". El `virtual-port` es un
puerto .onion (no es un socket real) y no se valida ni genera regla UFW.

## Salida JSON

Con `--format json`, la salida es un objeto `ServicePlan` serializable:

```json
{
  "kind": "wordpress",
  "service_name": "foo",
  "ports": [{ "label": "http-port", "port": 8090, "bind_interface": "127.0.0.1", "source": "manual" }],
  "containers": [...],
  "paths": [...],
  "firewall_rules": [{ "port": 8090, "protocol": "tcp", "scope": "loopback" }],
  "apparmor": { "profile_name": "enola-wp-foo", "mode": "complain" },
  "risk": "low"
}
```

## Nivel de riesgo

Calculado **estáticamente** desde el plan (no inspecciona el sistema vivo;
para auditoría runtime usa `doctor --security`):

| Nivel | Condición |
|-------|-----------|
| 🟢 **low** | Todo en 127.0.0.1, puertos ≥1024, sin SSH expuesto |
| 🟡 **medium** | Puerto privilegiado (<1024, excepto 80/443) o SSH expuesto (Git) |
| 🔴 **high** | Cualquier bind a 0.0.0.0 (defensivo — no ocurre en Enola) |

## Diferencia con `doctor --security`

| | `plan` | `doctor --security` |
|---|-------|---------------------|
| Cuándo | Antes de crear un servicio | Después, sobre servicios existentes |
| Qué hace | Plan declarativo (dry-run) | Auditoría del sistema vivo |
| Side effects | 0 | 0 (solo lectura) |
| Riesgo | Estático desde el plan | Desde contenedores/configs reales |

## Ver también

- [Índice de comandos](commands.md)
- [`wp create`](../wp/commands-wp.md) · [`git create`](../git/commands-git.md) · [`tor create`](../tor/commands-tor.md)
- [`doctor --security`](commands-simple.md#doctor)
