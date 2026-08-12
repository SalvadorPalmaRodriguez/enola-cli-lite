> **Documento usuario:** `docs/user/wp/commands-wp.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 📝 WordPress — Comandos `enola-cli wp`

> **Stack:** `wordpress:latest` + `mariadb:10.11`. Datos en `/srv/enola-wordpress/{name}_wp/`.

WordPress es el CMS principal del catálogo Enola. Ideal para blogs, sitios institucionales
y proyectos que necesitan plugins y temas extensos. Stack: 2 contenedores (web + BD), ~512 MB RAM.

---

## `wp list`

Lista todos los sitios WordPress con su estado y puerto.

```bash
sudo enola-cli wp list
```

Sin flags ni argumentos.

---

## `wp create`

Crea una nueva instancia WordPress con MariaDB.

```bash
sudo enola-cli wp create --name <NOMBRE> [--http-port <PUERTO>]
```

| Flag | Tipo | Obligatorio | Default | Descripción |
|------|------|-------------|---------|-------------|
| `--name` / `-n` | String | Sí | — | Nombre del sitio (alfanumérico + `_-`) |
| `--http-port` | u16 | No | Auto (8080-9000) | Puerto HTTP interno (Nginx → Docker → WordPress) |

**Notas:**
- El puerto solo es accesible desde `127.0.0.1`. Los visitantes acceden via `.onion`.
- Se crean automáticamente: contenedor web, contenedor BD, red Docker y secrets.

**Ejemplo:**
```bash
sudo enola-cli wp create --name miblog
sudo enola-cli wp create --name miblog --http-port 8090
```

---

## `wp status`

Muestra el estado de un sitio WordPress (contenedores y puerto activo).

```bash
sudo enola-cli wp status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp start`

Arranca un sitio WordPress (BD primero, luego web).

```bash
sudo enola-cli wp start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp stop`

Detiene un sitio WordPress (web primero, luego BD).

```bash
sudo enola-cli wp stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp restart`

Reinicia un sitio WordPress.

```bash
sudo enola-cli wp restart <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp delete`

Elimina un sitio WordPress.

```bash
sudo enola-cli wp delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite el prompt de confirmación |

---

## `wp update`

Actualiza WordPress (con backup automático previo).

```bash
sudo enola-cli wp update <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp config`

Edita la configuración de un sitio WordPress.

```bash
sudo enola-cli wp config <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp publish`

Publica el sitio en Tor como hidden service.

```bash
sudo enola-cli wp publish <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp hide`

Retira el sitio de Tor (elimina el hidden service).

```bash
sudo enola-cli wp hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

---

## `wp edit`

Cambia los puertos y configuración SSL de un sitio WordPress.

```bash
sudo enola-cli wp edit <NOMBRE> [--http-port <P>] [--https-port <P>] [--ssl <true|false>] [--auto-ports]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del sitio |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--http-port` | u16 | Nuevo puerto HTTP (Nginx) |
| `--https-port` | u16 | Nuevo puerto HTTPS (Nginx SSL) |
| `--ssl` | Bool | Habilita/deshabilita SSL |
| `--auto-ports` | Bool | Encuentra puertos libres automáticamente |

**Ejemplo:**
```bash
sudo enola-cli wp edit miblog --http-port 8095 --ssl true
```

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor web | `wp-{name}` |
| Contenedor BD | `db-{name}` |
| Red Docker | `enola_net_wp_{name}` |
| Datos | `/srv/enola-wordpress/{name}_wp/` |
| Tor service | `wp-{name}` |
| Hostname `.onion` | `/var/lib/tor/enola_wp-{name}/hostname` |

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
