> **Documento usuario:** `docs/user/wagtail/commands-wagtail.md`
> **Versión:** 2.2 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🐍 Wagtail — Comandos `enola-cli wagtail`

> **Stack:** `wagtail/bakerydemo:latest` (sitio demo Python/Django) + Postgres. Datos en `/srv/enola-wagtail/{name}/`.
> Secrets montados como Docker secrets en `/run/secrets/` e inyectados via
> entrypoint wrapper (no plaintext en env vars).

Wagtail es un CMS Python del catálogo Enola. Ideal para sitios con contenido
estructurado, editor amigable y ecosistema Django. Stack: 2 contenedores
(web + Postgres), puerto interno 8000.

---

## `wagtail list`

Lista todas las instancias Wagtail con su estado y puerto.

```bash
sudo enola-cli wagtail list
```

Sin flags ni argumentos.

---

## `wagtail create`

Crea una nueva instancia Wagtail con Postgres.

```bash
sudo enola-cli wagtail create --name <NOMBRE> --http-port <PUERTO>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre de la instancia (alfanumérico + `_-`) |
| `--http-port` | u16 | Sí | Puerto HTTP interno (Nginx → Docker → Wagtail:8000) |

**Ejemplo:**
```bash
sudo enola-cli wagtail create --name miweb --http-port 8300
```

---

## `wagtail status`

Muestra el estado de una instancia Wagtail.

```bash
sudo enola-cli wagtail status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `wagtail start`

Arranca una instancia Wagtail (web + BD).

```bash
sudo enola-cli wagtail start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `wagtail stop`

Detiene una instancia Wagtail (web + BD).

```bash
sudo enola-cli wagtail stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `wagtail delete`

Elimina una instancia Wagtail. Los datos persisten en `/srv/enola-wagtail/{name}/`.

```bash
sudo enola-cli wagtail delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite la comprobación de si la instancia está corriendo. Sin este flag, se aborta si está activa. |

---

## `wagtail publish`

Publica la instancia en Tor como hidden service.

```bash
sudo enola-cli wagtail publish <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `wagtail hide`

Retira la instancia de Tor (elimina el hidden service).

```bash
sudo enola-cli wagtail hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor web | `wagtail-{name}` |
| Contenedor BD | `db-{name}-wagtail` |
| Datos | `/srv/enola-wagtail/{name}/` |
| Puerto interno (Wagtail) | `8000` |
| Tor service | `wagtail-{name}` |

---

## Gestión de secrets

Wagtail (Django/bakerydemo) no soporta el patrón `_FILE` para leer secrets
desde archivos. Enola CLI usa un **entrypoint wrapper** para inyectar secrets
de forma segura:

1. **Generación**: al crear una instancia, se generan 3 secrets
   (db_password, django_secret_key, admin_password) y se escriben en
   `/srv/enola-wagtail/<name>/secrets/` con permisos `0600`.

2. **Montaje**: los secrets se montan como Docker secrets en `/run/secrets/`
   (read-only) dentro del contenedor Wagtail.

3. **Inyección**: un script `entrypoint.sh` (montado en `/entrypoint.sh`)
   lee `db_password` y `django_secret_key` desde `/run/secrets/`, construye
   `DATABASE_URL` y exporta `SECRET_KEY` antes de ejecutar
   `python manage.py runserver 0.0.0.0:8000`.

4. **Verificación**: `docker inspect` no muestra `DATABASE_URL` ni `SECRET_KEY`
   en las variables de entorno del contenedor.

---

## Limitaciones conocidas

- Wagtail **no tiene subcomando `edit`** (a diferencia de WordPress, Drupal y Ghost).
  Para cambiar el puerto HTTP: `delete --force` + `create` con el nuevo puerto.
  Los datos persisten en `/srv/enola-wagtail/{name}/`.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
