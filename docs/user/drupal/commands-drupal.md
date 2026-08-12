> **Documento usuario:** `docs/user/drupal/commands-drupal.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🐉 Drupal — Comandos `enola-cli drupal`

> **Stack:** `drupal:10-apache` + `mariadb:10.11`. Datos en `/srv/enola-drupal/{name}/`.

Drupal complementa a WordPress en el catálogo CMS de Enola (PHP, no Java). Ideal para sitios
institucionales, contenido estructurado complejo y permisos granulares. Comparte con WordPress
todo el stack de seguridad de Enola: secrets 0600, binding `127.0.0.1`, Tor opcional, aislamiento Nginx.

---

## `drupal create`

Crea una nueva instancia Drupal con su base de datos MariaDB.

```bash
sudo enola-cli drupal create --name <NOMBRE> --http-port <PUERTO>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre del sitio (alfanumérico + `_-`) |
| `--http-port` | u16 | Sí | Puerto HTTP interno (Nginx → Docker → Drupal Apache) |

**Notas:**
- El puerto solo es accesible desde `127.0.0.1`. Los visitantes acceden via `.onion`.
- El puerto debe estar libre a nivel de SO y Docker.
- Se crean automáticamente: contenedor web, contenedor BD, red Docker y secrets.

**Ejemplo:**
```bash
sudo enola-cli drupal create --name miblog --http-port 8090
```

---

## `drupal list`

Lista todas las instancias Drupal con su estado y puerto.

```bash
sudo enola-cli drupal list
```

Sin flags ni argumentos.

---

## `drupal status`

Muestra el estado de un sitio Drupal (contenedores y puerto activo).

```bash
sudo enola-cli drupal status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

**Estados posibles:** `running`, `stopped`, `initializing`, `not-found`.

**Ejemplo:**
```bash
sudo enola-cli drupal status miblog
# 🟢 Container drupal-miblog:    Running
# 🟢 Container db-miblog-drupal: Running
# 🌐 HTTP:  http://127.0.0.1:8090/
```

---

## `drupal start`

Arranca un sitio Drupal. Inicia la base de datos primero y luego el contenedor web (el orden importa para que Drupal encuentre la BD al arrancar).

```bash
sudo enola-cli drupal start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `drupal stop`

Detiene un sitio Drupal. Para el contenedor web primero y luego la base de datos (orden inverso al arranque para evitar conexiones colgadas).

```bash
sudo enola-cli drupal stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `drupal delete`

Elimina los contenedores y la red Docker del sitio. Los datos persisten en `/srv/enola-drupal/{name}/` a menos que se eliminen manualmente.

```bash
sudo enola-cli drupal delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite la comprobación de si el sitio está corriendo. Sin este flag, se aborta si el sitio está activo. |

**Ejemplo:**
```bash
sudo enola-cli drupal delete miblog --force
```

---

## `drupal publish`

Publica el sitio en Tor como hidden service. El sitio pasa a ser accesible via una dirección `.onion`.

```bash
sudo enola-cli drupal publish <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

**Resultado:** devuelve la dirección `.onion` asignada.

**Ejemplo:**
```bash
sudo enola-cli drupal publish miblog
# → 🧅 http://r4nd0m...abc.onion/
```

---

## `drupal hide`

Retira el sitio de Tor (elimina el hidden service). El sitio sigue accesible via `127.0.0.1:PUERTO`.

```bash
sudo enola-cli drupal hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `drupal edit`

Cambia el puerto HTTP del sitio. Como Docker no permite reasignar port bindings en caliente, este comando recrea el contenedor `drupal-{name}` de forma atómica preservando imagen, variables de entorno, volumen `/var/www/html`, red y secrets. El contenedor de BD no se toca (el puerto 3306 es interno a la red Docker).

Si el sitio está publicado en Tor, el `HiddenServicePort` se actualiza automáticamente.

```bash
sudo enola-cli drupal edit <NOMBRE> --http-port <PUERTO>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--http-port` | u16 | Sí | Nuevo puerto HTTP (debe estar libre) |

**Ejemplo:**
```bash
sudo enola-cli drupal edit miblog --http-port 8095
```

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor web | `drupal-{name}` |
| Contenedor BD | `db-{name}-drupal` |
| Red Docker | `enola_net_drupal_{name}` |
| Datos | `/srv/enola-drupal/{name}/{web,db,secrets}/` |
| Tor service | `drupal-{name}` (sin colisión con `wp-{name}`) |
| Hostname `.onion` | `/var/lib/tor/enola_drupal-{name}/hostname` |

---

## Diferencias con WordPress

| Característica | WordPress | Drupal |
|----------------|-----------|--------|
| Stack BD | MariaDB 10.11 | MariaDB 10.11 |
| Naming contenedor | `wp-{name}` + `db-{name}` | `drupal-{name}` + `db-{name}-drupal` |
| Datos | `/srv/enola-wordpress/{name}_wp/` | `/srv/enola-drupal/{name}/` |
| Puerto HTTP | Auto (8080-9000) | **Manual obligatorio** |
| Setup wizard | Sí (HTTP 200/301/302/304/500 hasta completarlo) | Sí (mismo rango de códigos) |
| Tor service | `wp-{name}` | `drupal-{name}` |
| `edit` puertos en caliente | ✅ | ✅ |

---

## Ejemplo completo

```bash
# 1. Crear el sitio
sudo enola-cli drupal create --name miblog --http-port 8090

# 2. Verificar estado
sudo enola-cli drupal status miblog

# 3. Completar el wizard de Drupal en el navegador
#    Password de la BD ya disponible en /run/secrets/db_password dentro del contenedor

# 4. Publicar en Tor (anonimato)
sudo enola-cli drupal publish miblog
# → 🧅 http://r4nd0m...abc.onion/

# 5. Cambiar puerto
sudo enola-cli drupal edit miblog --http-port 8095

# 6. Detener y reanudar
sudo enola-cli drupal stop miblog
sudo enola-cli drupal start miblog

# 7. Ocultar de Tor
sudo enola-cli drupal hide miblog

# 8. Eliminar
sudo enola-cli drupal delete miblog --force
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---

