> **Documento usuario:** `docs/user/git/commands-git.md`
> **Versión:** 2.1 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🔧 Git — Comandos `enola-cli git`

Gestiona servidores Git (Forgejo). Arquitectura: `.onion:80 → Nginx → Docker:HTTP_PORT → Forgejo`

Forgejo permite clonar repositorios via SSH sobre Tor: `git clone ssh://xxx.onion/repo`

---

## `git list`

Lista todos los servidores Git.

```bash
sudo enola-cli git list
```

Sin flags ni argumentos.

---

## `git create`

Crea un nuevo servidor Git (Forgejo).

Dos modos de primer acceso:
- **Modo CLI** (recomendado): admin creado automáticamente con `--admin-user` y `--admin-password`. El admin deberá cambiar su contraseña en el primer login.
- **Modo web**: el usuario completa el asistente de instalación en el navegador.

```bash
sudo enola-cli git create --name <NOMBRE> [--ssl] [--http-port <PUERTO>] [--ssh-port <PUERTO>] [--admin-user <USER>] [--admin-password <PASS>]
```

| Flag | Tipo | Obligatorio | Default | Descripción |
|------|------|-------------|---------|-------------|
| `--name` / `-n` | String | Sí | — | Nombre del servidor |
| `--ssl` | Bool | No | `false` | Habilita HTTPS con certificado autofirmado |
| `--http-port` | u16 | No | Auto (10000-15000) | Puerto HTTP interno de Forgejo |
| `--ssh-port` | u16 | No | Auto (30000-35000) | Puerto SSH interno de Forgejo |
| `--admin-user` | String | No¹ | — | Usuario admin inicial (modo CLI) |
| `--admin-password` | String | No¹ | — | Contraseña del admin inicial (modo CLI) |

> ¹ `--admin-password` es obligatorio si se especifica `--admin-user`. Si se omite `--admin-user`, Forgejo muestra el asistente web.

**Ejemplos:**
```bash
# Modo CLI — admin creado automáticamente
sudo enola-cli git create --name myrepo --admin-user alice --admin-password MiPass123

# Modo web — asistente de instalación en el navegador
sudo enola-cli git create --name myrepo

# Con puertos específicos
sudo enola-cli git create --name myrepo --http-port 10500 --ssh-port 30100

# Con HTTPS self-signed
sudo enola-cli git create --name myrepo --admin-user alice --admin-password MiPass123 --ssl
```

---

## `git start`

Arranca un servidor Git.

```bash
sudo enola-cli git start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

---

## `git stop`

Detiene un servidor Git.

```bash
sudo enola-cli git stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

---

## `git status`

Muestra el estado de un servidor Git (running/stopped y puertos).

```bash
sudo enola-cli git status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

---

## `git delete`

Elimina un servidor Git.

```bash
sudo enola-cli git delete <NOMBRE> --force
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | **Obligatorio** — confirma el borrado (el comando aborta sin este flag) |

---

## `git registration`

Habilita, deshabilita o consulta el registro de usuarios en un servidor Forgejo.

```bash
sudo enola-cli git registration <NOMBRE> [--enable] [--disable] [--status]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--enable` | Bool | Habilita el auto-registro de usuarios |
| `--disable` | Bool | Deshabilita el auto-registro (solo admin crea cuentas) |
| `--status` | Bool | Muestra el estado actual sin modificar |

> Debes especificar exactamente uno de los tres flags.

**Ejemplos:**
```bash
sudo enola-cli git registration myrepos --enable
sudo enola-cli git registration myrepos --disable
sudo enola-cli git registration myrepos --status
```

---

## `git edit`

Cambia los puertos de un servidor Git.

```bash
sudo enola-cli git edit <NOMBRE> [--http-port <P>] [--https-port <P>] [--ssh-port <P>] [--auto-ports]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--http-port` | u16 | Nuevo puerto HTTP (Nginx si SSL activo) |
| `--https-port` | u16 | Nuevo puerto HTTPS (solo si SSL activo) |
| `--ssh-port` | u16 | Nuevo puerto SSH |
| `--auto-ports` | Bool | Encuentra puertos libres automáticamente |

---

## `git publish`

Publica un servidor Git en Tor como hidden service.

```bash
sudo enola-cli git publish <NOMBRE> [--ssl]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--ssl` | Bool | Habilita HTTPS con certificado autofirmado |

---

## `git hide`

Retira un servidor Git de Tor (elimina el hidden service).

```bash
sudo enola-cli git hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servidor |

---

## `git watcher`

Ejecuta el Git Pipeline Watcher en primer plano. Monitoriza los pipelines CI/CD
de Forgejo (ejecuciones de Actions) mostrando su estado en tiempo real sin
necesidad de abrir el navegador.

**Qué vigila:** ejecuciones de pipelines (Forgejo Actions) de todos los repos
del servidor Git.

**Cómo funciona:** realiza polling periódico del endpoint de pipelines de Forgejo
y muestra cambios de estado (started, success, failure) en la terminal.

**Cómo detenerlo:** `Ctrl+C` (interrumpe el watcher, no afecta al servidor).

**Requisitos:** el servidor Git debe estar creado y arrancado antes de ejecutar
el watcher.

```bash
sudo enola-cli git watcher
```

Sin flags ni argumentos.

---

## `git user` — Gestión de usuarios

Subcomandos para gestionar usuarios del servidor Git.

### `git user list`

Lista los usuarios del servidor Git.

> **Nota**: Si el servidor se creó en modo CLI, el admin debe cambiar su contraseña
> en el primer login antes de usar este comando. Alternativamente, pasar
> `--admin-user` y `--admin-pass` explícitamente.
> La contraseña admin se almacena como hash bcrypt (no plaintext) en
> `/srv/enola-git/<name>/.enola-admin-creds`. Al ejecutar comandos que requieren
> credenciales admin sin `--admin-pass`, se pedirá la contraseña interactivamente.

```bash
sudo enola-cli git user list <SERVIDOR> [--admin-user <USER>] [--admin-pass <PASS>]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVIDOR>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--admin-user` | String | Solo si el servidor se creó en modo web |
| `--admin-pass` | String | Solo si el servidor se creó en modo web |

---

### `git user create`

Crea un usuario en el servidor Git via `forgejo admin user create` (docker exec).

```bash
sudo enola-cli git user create <SERVIDOR> --username <USER> --email <EMAIL> --password <PASS> [--admin] [--admin-user <USER>] [--admin-pass <PASS>]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVIDOR>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--username` / `-u` | String | Sí | Nombre de usuario a crear |
| `--email` / `-e` | String | Sí | Email del usuario |
| `--password` / `-p` | String | Sí | Contraseña del usuario |
| `--admin` | Bool | No | Dar permisos de administrador (default: `false`) |
| `--admin-user` | String | No | Admin de Forgejo (solo modo web) |
| `--admin-pass` | String | No | Contraseña admin (solo modo web) |

---

### `git user delete`

Elimina un usuario del servidor Git.

```bash
sudo enola-cli git user delete <SERVIDOR> --username <USER> [--admin-user <USER>] [--admin-pass <PASS>]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVIDOR>` | String | Sí | Nombre del servidor |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--username` / `-u` | String | Sí | Usuario a eliminar |
| `--admin-user` | String | No | Admin de Forgejo (solo modo web) |
| `--admin-pass` | String | No | Contraseña admin (solo modo web) |

---

## Seguridad de credenciales admin

### Almacenamiento de credenciales

Cuando se crea un servidor Git en modo CLI (`--admin-user` + `--admin-pass`),
las credenciales se guardan en `/srv/enola-git/<name>/.enola-admin-creds`:

- La contraseña se almacena como **hash bcrypt** (cost 12), nunca como plaintext.
- El archivo tiene permisos `0600` (solo root).
- Formato: `ADMIN_USER=<user>` + `ADMIN_PASS_HASH=$2b$...` + `ADMIN_PASS_HASH_ALGO=bcrypt`

### Prompt interactivo

Al ejecutar `git user list/create/delete` sin `--admin-pass`:

1. El CLI lee el archivo `.enola-admin-creds`.
2. Si encuentra `ADMIN_PASS_HASH`, pide la contraseña interactivamente:

   ```
   🔐 Servidor 'mygit' — credenciales admin (modo CLI)
      Usuario: admin
      Contraseña admin: █
   ```

3. Verifica la contraseña contra el hash bcrypt.
4. Si es correcta, ejecuta la operación via `forgejo admin` (docker exec).

> **Compatibilidad**: si el archivo tiene formato anterior (`ADMIN_PASS=plaintext`),
> se usa directamente sin prompt. Esto permite migración transparente.

### Rotación obligatoria de contraseña

Todo admin de Forgejo se crea con `--must-change-password=true`. Esto fuerza
al admin a cambiar su contraseña en el primer login, mitigando el riesgo de
que la contraseña inicial (visible en historial de shell o logs) persista
sin rotación.

> **Nota**: mientras la rotación esté pendiente, Forgejo bloquea la API REST
> con 403. Por eso `git user list/create/delete` operan via `forgejo admin`
> (docker exec), que no depende del estado de la contraseña.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
