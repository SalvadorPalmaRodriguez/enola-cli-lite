> **Documento usuario:** `docs/user/tor/commands-tor.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🧅 Tor — Comandos `enola-cli tor`

Gestiona servicios ocultos Tor. Arquitectura: `.onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT`

Tipos de servicio:
- `web` / `proxy` / `http`: HTTP via Nginx (Tor → Nginx → App) — **recomendado para web**
- `raw` / `tcp`: Conexión TCP directa (Tor → App) — para SSH, bases de datos
- `static`: Sitio estático servido por Nginx
- `files`: Servidor de archivos con directory listing

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

Rota el keypair de un cliente (genera nuevas claves X25519, actualiza el servidor y revoca la clave antigua).

```bash
sudo enola-cli tor auth rotate <SERVICIO> --client <CLIENTE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<SERVICIO>` | String | Sí | Nombre del servicio |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--client` / `-c` | String | Sí | Nombre del cliente a rotar |

---

## Ver también

- [Autorización de cliente Tor](tor-client-auth.md) — guía detallada de auth de clientes.
- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
