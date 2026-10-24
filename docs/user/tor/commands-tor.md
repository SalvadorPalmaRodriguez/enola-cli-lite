> **Documento usuario:** `docs/user/tor/commands-tor.md`
> **Versión:** 2.4 | **Actualizado:** 2026-10-24
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🧅 Tor — Comandos `enola-cli tor`

Gestiona servicios ocultos Tor. Arquitectura: `.onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT`

Tipos de servicio:
- `web` / `proxy` / `http`: HTTP via Nginx (Tor → Nginx → App) — **recomendado para web**
- `raw` / `tcp`: Conexión TCP directa (Tor → App) — para SSH, bases de datos
- `static`: Sitio estático servido por Nginx
- `files`: Servidor de archivos con directory listing

**Cómo se resuelve `<NAME>`** en `tor start|stop|remove|rotate|edit`:
1. Si coincide exactamente con un servicio (p. ej. `wp_dev`), se usa ese.
2. Si no, se prueban las variantes por tipo (`git_dev`, `wp_dev`, `proxy_dev`…): con una sola coincidencia se usa esa.
3. Si coinciden varias, el comando **falla** y lista los candidatos ordenados por tipo; repite con el nombre completo (`enola-cli tor list` los muestra todos). Nunca se elige uno al azar.

---

## `tor list`

Lista todos los servicios ocultos Tor configurados.

```bash
sudo enola-cli tor list
```

Sin flags ni argumentos.

---

## `tor create`

Crea un nuevo servicio oculto Tor.

```bash
sudo enola-cli tor create --name <NOMBRE> [--service-type <TIPO>] [--virtual-port <PUERTO>] [--target-port <PUERTO>] [--ssl]
```

| Flag | Tipo | Obligatorio | Default | Descripción |
|------|------|-------------|---------|-------------|
| `--name` / `-n` | String | Sí | — | Nombre del servicio (alfanumérico + guiones) |
| `--service-type` / `-s` | String | No | `web` | Tipo: `raw`, `web`, `static`, `files` |
| `--virtual-port` / `-p` | u16 | No | `80` | Puerto público `.onion` |
| `--target-port` / `-t` | u16 | No¹ | — | Puerto de la aplicación local |
| `--ssl` | Bool | No | `false` | Habilita HTTPS con certificado self-signed (crea endpoints 80 + 443) |

> ¹ `--target-port` es obligatorio para tipos `web` y `raw`. No se usa para `static` y `files`.

**Ejemplos:**
```bash
# Servicio web estándar (Tor → Nginx → App:3000)
sudo enola-cli tor create --name miweb --target-port 3000

# Servicio estático (Tor → Nginx, sin app backend)
sudo enola-cli tor create --name estatico --service-type static

# Servidor de archivos
sudo enola-cli tor create --name archivos --service-type files

# SSH por Tor (TCP directo)
sudo enola-cli tor create --name ssh --service-type raw --virtual-port 22 --target-port 22

# Con HTTPS self-signed
sudo enola-cli tor create --name miweb --target-port 3000 --ssl
```

---

## `tor start`

Arranca un servicio oculto Tor.

```bash
sudo enola-cli tor start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servicio |

---

## `tor stop`

Detiene un servicio oculto Tor.

```bash
sudo enola-cli tor stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servicio |

---

## `tor remove`

Elimina un servicio oculto Tor.

```bash
sudo enola-cli tor remove <NOMBRE> --force
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | **Obligatorio** — confirma el borrado (el comando aborta sin este flag) |

**Ejemplo:**
```bash
sudo enola-cli tor remove miweb --force
```

---

## `tor edit`

Cambia la configuración de puertos de un servicio oculto.

Flujo de puertos: `.onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT`

```bash
sudo enola-cli tor edit <NOMBRE> [--virtual-port <P>] [--nginx-port <P>] [--target-port <P>] [--auto-ports]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--virtual-port` / `-p` | u16 | Nuevo puerto público `.onion` |
| `--nginx-port` / `-n` | u16 | Nuevo puerto interno de Nginx |
| `--target-port` / `-t` | u16 | Nuevo puerto de la aplicación |
| `--auto-ports` | Bool | Encuentra puertos libres automáticamente |

> Si se cambia `--virtual-port`, el puerto de Nginx se actualiza automáticamente salvo que se especifique `--nginx-port`.

**Ejemplos:**
```bash
# Cambiar puertos manualmente
sudo enola-cli tor edit miweb -p 8081 -t 9000 -n 15000

# Auto-asignar puertos libres
sudo enola-cli tor edit miweb --auto-ports

# Mixto: puerto virtual fijo, resto auto
sudo enola-cli tor edit miweb -p 443 --auto-ports
```

---

## `tor rotate`

Rota la dirección `.onion` de un servicio (genera nueva identidad).

```bash
sudo enola-cli tor rotate <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del servicio |

---

## `tor ssl` — Activar HTTPS en un servicio ya publicado

Activa SSL de forma post-hoc sobre un servicio `.onion` publicado en HTTP:
genera el certificado self-signed canónico (`<nombre>.crt|.key` en
`/etc/nginx/ssl/`), convierte el site Nginx a HTTP+HTTPS y añade el puerto
443 al hidden service (la dirección `.onion` no cambia).

```bash
sudo enola-cli tor ssl <SERVICIO>
```

El `<SERVICIO>` es el nombre canónico del servicio (`ai_dev`, `wp_blog`,
`proxy_web1`, `fileserver_docs`, `git_repo`, `drupal_sitio`, ...).
Falla con error claro si el servicio ya tiene SSL o si no existe site
Nginx convertible (`raw`/`static`).

```bash
sudo enola-cli tor ssl ai_dev
# 🔐 SSL enabled for 'ai_dev'
# 🧅 Address: https://abc123...onion
```

## `tor auth` — Gestión de autorización de clientes

Subcomandos para controlar qué clientes pueden acceder a un servicio oculto con autorización habilitada.

Documentación detallada: [tor-client-auth.md](tor-client-auth.md)

### `tor auth list`

Lista los clientes autorizados de un servicio.

```bash
sudo enola-cli tor auth list <SERVICIO>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

---

### `tor auth enable`

Habilita la autorización de clientes para un servicio.

```bash
sudo enola-cli tor auth enable <SERVICIO>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

---

### `tor auth disable`

Deshabilita la autorización de clientes para un servicio.

```bash
sudo enola-cli tor auth disable <SERVICIO>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

---

### `tor auth add`

Añade un cliente autorizado (operación del lado del operador). El cliente debe haber generado su keypair con `tor auth generate` y enviado su clave pública.

```bash
sudo enola-cli tor auth add <SERVICIO> --client <CLIENTE> --pubkey <CLAVE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--client` / `-c` | String | Sí | Nombre del cliente |
| `--pubkey` / `-p` | String | Sí | Clave pública x25519 (base32, 52 chars) |

**Ejemplo:**
```bash
sudo enola-cli tor auth add miweb --client alice --pubkey ABCDEF...1234
```

---

### `tor auth revoke`

Revoca la autorización de un cliente.

```bash
sudo enola-cli tor auth revoke <SERVICIO> --client <CLIENTE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--client` / `-c` | String | Sí | Nombre del cliente a revocar |

---

### `tor auth generate`

Genera un nuevo keypair de cliente (operación del lado del cliente). La clave privada nunca sale de tu máquina. Envías solo la clave pública al operador.

```bash
sudo enola-cli tor auth generate --client <CLIENTE>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--client` / `-c` | String | Sí | Nombre del cliente |

> ⚠️ Las claves X25519 no son resistentes a ataques cuánticos. Rota periódicamente.

---

### `tor auth rotate`

Sustituye la clave pública de un cliente. El cliente genera el nuevo par con
`tor auth generate` y envía SOLO la nueva clave pública al operador; la clave
privada nunca llega al servidor.

> ⚠️ El cliente debe existir ya: `rotate` no crea clientes nuevos (usa
> `tor auth add` primero) ni habilita la autenticación del servicio si estaba
> desactivada.

```bash
sudo enola-cli tor auth rotate <SERVICIO> --client <CLIENTE> --pubkey <NUEVA_PUBLICA>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--client` / `-c` | String | Sí | Nombre del cliente a rotar |
| `--pubkey` / `-p` | String | Sí | Nueva clave pública X25519 (base32, 52 chars) |

---

## Nombres de servicio y migración desde v0.9.0

Desde v0.10.0 cada servicio publicado se llama `<dominio>_<nombre>`: así se llaman
el servicio Tor y su certificado (`/etc/nginx/ssl/<dominio>_<nombre>.{crt,key}`), y
el site Nginx es `proxy_<dominio>_<nombre>` (`fileserver_<nombre>` en `files`):

| Publicado con | Nombre en v0.10.0 | Nombre que daba v0.9.0 |
|---|---|---|
| `git publish repo` | `git_repo` | `repo` |
| `wp publish blog` | `wp_blog` | `proxy_blog` |
| `drupal publish sitio` (igual `ghost`, `magnolia`, `strapi`, `wagtail`) | `drupal_sitio` | `proxy_drupal-sitio` |
| `files create --name docs` | `fileserver_docs` | `fileserver_docs` (sin cambio) |
| `ai publish dev` | `ai_dev` | — (no existía) |
| `tor create --name web1 -s web` | `proxy_web1` | `proxy_web1` (sin cambio) |

Los servicios publicados con v0.9.0 **no se gestionan automáticamente**: `hide`,
`tor auth <dominio> <instancia>` y `tor ssl` buscan el nombre nuevo.
Si solo encuentran el antiguo, fallan con el comando exacto para migrar. Volver a
publicar un CMS cuyo servicio antiguo sigue existiendo se rechaza, para no crear
una segunda dirección `.onion`. En `wp` y `git`, cuyos nombres antiguos también los
genera `tor create`, se publica y se muestra un aviso.

Para migrar un servicio:

```bash
sudo enola-cli tor list                         # localiza el nombre antiguo
sudo enola-cli tor remove proxy_drupal-sitio    # elimina el servicio antiguo
sudo enola-cli drupal publish sitio [--ssl] [--auth]
```

> ⚠️ **La dirección `.onion` cambia** al volver a publicar: comunica la nueva a
> quien use el servicio y, si tenía autorización de cliente, vuelve a añadir las
> claves públicas (`tor auth add <dominio> <nombre> …`).

## Ver también

- [Autorización de cliente Tor](tor-client-auth.md) — guía detallada de auth de clientes.
- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
