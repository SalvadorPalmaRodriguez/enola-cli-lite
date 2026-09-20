> **Documento usuario:** `docs/user/general/commands-plan.md`
> **Versión:** 1.2 | **Actualizado:** 2026-09-20
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
- **Perfil AppArmor** que se aplicaría (SIN ejecutar `aa-enforce`). Solo
  `wp create` y `git create` crean un perfil por servicio; `tor create` no
  crea ninguno y el plan lo indica con `(none)`.
- **Nivel de riesgo** (bajo/medio/alto) calculado estáticamente desde el plan.
- **Notas**: advertencias sobre flags que `create` ignoraría — el plan los
  señala en la sección `📝 Notes` en vez de fingir que tienen efecto.

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

Con `--ssl` el plan añade:

- un `https-port` auto-asignado en el rango **15001-20000**;
- el site Nginx `/etc/nginx/sites-available/proxy_<name>` y el par
  `/etc/nginx/ssl/<name>.crt` + `.key` (certificado autofirmado);
- una **Nota** recordando que `git create --ssl` **NO registra el puerto
  HTTPS en UFW** (solo se sincronizan `http-port` y `ssh-port`).

El contenedor Forgejo usa el **bridge por defecto** de Docker (la salida
muestra `network: (default bridge)`), no una red dedicada.

> **Nota:** `--admin-user`/`--admin-password` (presentes en `git create`)
> se omiten en `plan` porque no afectan puertos, contenedor, firewall ni AppArmor.

**Ejemplo:**

```bash
sudo enola-cli plan git create --name repo --ssl
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
| `-s, --service-type <TYPE>` | `raw`/`tcp`, `web`/`proxy`/`http`, `static`, `files`/`fileserver` | `web` |
| `-p, --virtual-port <PORT>` | Puerto público .onion (solo se respeta en `raw`) | `80` |
| `-t, --target-port <PORT>` | Puerto local de tu app (se asume que ya escucha ahí; no se valida ni auto-asigna) | ver tabla |
| `--ssl` | HTTPS con cert autofirmado (solo `web`) | false |

Un `--service-type` desconocido es un error (mismo mensaje que `tor create`).

**Comportamiento por tipo** (refleja `tor create` exactamente):

| Tipo | Puertos del plan | Rutas |
|------|------------------|-------|
| `raw`/`tcp` | `virtual-port` (el que pases) + `target-port` (`--target-port`, o el virtual si no se pasa) | `/var/lib/tor/enola_<name>` + `/etc/tor/enola.d/<name>.conf` |
| `web`/`proxy`/`http` | `virtual-port` 80 + `nginx-port` auto (10000-20000) + `backend-port` (`--target-port`, default 8080). Con `--ssl`: `virtual-port` 80 + `virtual-port-https` 443 + `nginx-http-port` auto (10000-15000) + `nginx-https-port` auto (15001-20000) + `backend-port` | igual con prefijo `proxy_` (`/var/lib/tor/enola_proxy_<name>`, `/etc/tor/enola.d/proxy_<name>.conf`) + `/etc/nginx/sites-available/proxy_<name>`; con `--ssl` además `/etc/nginx/ssl/<name>.crt` + `.key` |
| `static` | `virtual-port` 80 + `nginx-port` auto (20000-30000) | `/var/lib/tor/enola_<name>`, `/etc/tor/enola.d/<name>.conf`, `/etc/nginx/sites-available/<name>`, `/var/www/<name>` |
| `files`/`fileserver` | `virtual-port` 80 + `nginx-port` auto (20000-30000) | prefijo `fileserver_` en Tor y Nginx + `/srv/enola-files/<name>` |

Limitaciones reales de `tor create` que el plan señala en `📝 Notes`:

- `--virtual-port` solo se respeta en `raw`; en `web`/`static`/`files` el
  `.onion` siempre se publica en `:80` (y `:443` con `--ssl`).
- `static` y `files` ignoran `--target-port` (el puerto Nginx se auto-asigna
  en 20000-30000) y también `--ssl` (solo `web` soporta HTTPS).
- UFW: `tor create` solo registra el `--target-port` cuando lo pasas
  explícitamente; los puertos de Nginx nunca se registran.

**Ejemplo:**

```bash
sudo enola-cli plan tor create --name svc --service-type files --target-port 1234
```

**Salida (texto):**

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
  "risk": "low",
  "notes": []
}
```

`containers[].network` es `null` cuando el servicio usa el bridge por
defecto de Docker (p. ej. Git/Forgejo) en vez de una red dedicada.
`apparmor` es `null` para servicios Tor (`tor create` no crea perfil).

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

## Acceso desde la web

El dashboard local (`enola-cli web`, solo `127.0.0.1` y con token) expone los
mismos dry-runs como endpoints REST:

| Método | Ruta | Body |
|--------|------|------|
| POST | `/api/plan/wp/create` | `{ "name": "blog", "http_port": 8090? }` |
| POST | `/api/plan/git/create` | `{ "name": "repo", "ssl": true?, "http_port": ..., "ssh_port": ...? }` |
| POST | `/api/plan/tor/create` | `{ "name": "svc", "service_type": "web"?, "virtual_port": 80?, "target_port": ...?, "ssl": false? }` |

Los `?` son opcionales y usan los mismos defaults que el CLI
(`service_type = "web"`, `virtual_port = 80`, `ssl = false`). La respuesta es
el objeto `ServicePlan` serializado — **el mismo JSON que `--format json`**
(ver [Salida JSON](#salida-json)), no una versión en texto.

## Ver también

- [Índice de comandos](commands.md)
- [`wp create`](../wp/commands-wp.md) · [`git create`](../git/commands-git.md) · [`tor create`](../tor/commands-tor.md)
- [`doctor --security`](commands-simple.md#doctor)
