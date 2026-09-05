> **Documento usuario:** `docs/user/ghost/commands-ghost.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# ✍️ Ghost — Comandos `enola-cli ghost`

> **Stack:** `ghost:5-alpine` + **SQLite embebido** (sin contenedor BD adicional).
> Datos en `/srv/enola-ghost/{name}/content/`.

Ghost es el segundo CMS del catálogo Enola y el primero que valida `DbStack::Sqlite`
end-to-end: **un solo contenedor**, sin MariaDB/Postgres separado, ideal para portátil con
poca RAM (~256 MB). Pensado para blogs editoriales, newsletters y publicaciones con
suscripciones de pago.

---

## `ghost create`

Crea una nueva instancia Ghost con SQLite embebido.

```bash
sudo enola-cli ghost create --name <NOMBRE> --http-port <PUERTO>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre del blog (alfanumérico + `_-`) |
| `--http-port` | u16 | Sí | Puerto HTTP interno (Nginx → Docker → Ghost:2368) |

**Notas:**
- El puerto solo es accesible desde `127.0.0.1`. Los visitantes acceden via `.onion`.
- SQLite se crea automáticamente al arrancar — no hay contenedor de BD.
- Stack de un solo contenedor: ~256 MB RAM.

**Ejemplo:**
```bash
sudo enola-cli ghost create --name miblog --http-port 8095
```

---

## `ghost list`

Lista todas las instancias Ghost con su estado y puerto.

```bash
sudo enola-cli ghost list
```

Sin flags ni argumentos.

---

## `ghost status`

Muestra el estado de un blog Ghost (contenedor y puerto activo).

```bash
sudo enola-cli ghost status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

**Estados posibles:** `running`, `stopped`, `initializing`, `not-found`.

**Ejemplo:**
```bash
sudo enola-cli ghost status miblog
# ✍️  Ghost 'miblog'
#   status:    running
#   http_port: 8095
#   onion:     -
```

---

## `ghost start`

Arranca un blog Ghost. Al tener un solo contenedor (sin BD externa), el arranque es un único paso.

```bash
sudo enola-cli ghost start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

---

## `ghost stop`

Detiene un blog Ghost.

```bash
sudo enola-cli ghost stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

---

## `ghost delete`

Elimina el contenedor y la red Docker del blog. Los datos persisten en `/srv/enola-ghost/{name}/` a menos que se eliminen manualmente.

```bash
sudo enola-cli ghost delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite la comprobación de si el blog está corriendo. Sin este flag, se aborta si está activo. |

**Ejemplo:**
```bash
sudo enola-cli ghost delete miblog --force
```

---

## `ghost publish`

Publica el blog en Tor como hidden service.

```bash
sudo enola-cli ghost publish <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

---

## `ghost hide`

Retira el blog de Tor (elimina el hidden service).

```bash
sudo enola-cli ghost hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

---

## `ghost edit`

Cambia el puerto HTTP del blog. Recrea el contenedor de forma atómica preservando datos.

```bash
sudo enola-cli ghost edit <NOMBRE> --http-port <PUERTO>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del blog |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--http-port` | u16 | No | Nuevo puerto HTTP (debe estar libre). Sin flag, solo muestra la config actual. |

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor web | `ghost-{name}` (anti-colisión con `wp-{name}` y `drupal-{name}`) |
| Red Docker | `enola_net_ghost_{name}` |
| Datos | `/srv/enola-ghost/{name}/content/` |
| SQLite db (dentro del contenedor) | `/var/lib/ghost/content/data/ghost.db` |
| Tor service | `ghost-{name}` |
| Hostname `.onion` | `/var/lib/tor/enola_ghost-{name}/hostname` |

---

## Puertos

| Capa | Puerto |
|------|--------|
| Visitante (`.onion` virtual) | 80 / 443 (cuando esté en Tor) |
| Nginx → Ghost (host:contenedor) | `{http_port}:2368` |
| **Puerto interno de Ghost** | **2368** (default oficial — NO 80) |

---

## Diferencias con WordPress y Drupal

| Característica | WordPress | Drupal | Ghost |
|----------------|-----------|--------|-------|
| Lenguaje | PHP | PHP | Node.js |
| Stack BD | MariaDB 10.11 | MariaDB 10.11 | **SQLite** (embebida) |
| Contenedores | 2 (web + db) | 2 (web + db) | **1** (web sólo) |
| RAM mínima | 512 MB | 768 MB | **~256 MB ⚡** |
| Puerto interno | 80 | 80 | **2368** |
| Naming contenedor | `wp-{name}` + `db-{name}` | `drupal-{name}` + `db-{name}-drupal` | `ghost-{name}` |
| Datos | `/srv/enola-wordpress/{name}_wp/` | `/srv/enola-drupal/{name}/` | `/srv/enola-ghost/{name}/content/` |
| Setup wizard codes | 200/301/302/304/500 | 200/301/302/304/500 | 200/301/302/304/500/502/503 (502/503 durante boot Node) |
| Tor service | `wp-{name}` | `drupal-{name}` | `ghost-{name}` |
| `publish/hide/edit` nativo | ✅ | ✅ | ⏳ pendiente |

---

## ¿Cuándo elegir Ghost?

- **Blog/newsletter editorial** con plan de membresía integrado (Members + Stripe nativo).
- **Portátil o VPS pequeño** donde 256 MB de RAM marca la diferencia.
- **Stack sin BD externa**: SQLite reduce superficie de ataque y simplifica backups
  (un único directorio `content/`).

Si necesitas plugins masivos / temas → WordPress. Si necesitas multilingüe + permisos
granulares → Drupal. Si necesitas blog rápido con paywall → Ghost.

---

## Ejemplo completo

```bash
# 1. Crear el blog
sudo enola-cli ghost create --name miblog --http-port 8095

# 2. Verificar estado
sudo enola-cli ghost status miblog

# 3. Completar el wizard de Ghost en el navegador
#    Abre http://127.0.0.1:8095/ghost/ y crea el usuario admin.
#    SQLite ya está creada automáticamente en /var/lib/ghost/content/data/.

# 4. Publicar en Tor (mientras llega implementación nativa)
sudo enola-cli tor create --name ghost-miblog --target-port 8095

# 5. Detener / borrar
sudo enola-cli ghost stop miblog
sudo enola-cli ghost delete miblog --force
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---

