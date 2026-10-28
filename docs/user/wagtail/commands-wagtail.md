> **Documento usuario:** `docs/user/wagtail/commands-wagtail.md`
> **Versión:** 3.0 | **Actualizado:** 2026-10-28
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🐍 Wagtail — Comandos `enola-cli wagtail`

> **Stack:** `enola/wagtail:7.4.4-py313` (proyecto Wagtail vacío, rama 7.4 LTS, construido por Enola) + Postgres 16. Datos en `/srv/enola-wagtail/{name}/`.
> Secrets montados como ficheros en `/run/secrets/` y leídos por la propia imagen
> (nunca en variables de entorno).

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

> ℹ️ **Imagen construida por Enola.** La primera vez que ejecutas `wagtail create`,
> Enola construye `enola/wagtail:7.4.4-py313` en tu máquina (~3-6 min, ~470 MB de disco,
> necesita Internet para descargar Python y los paquetes). Las siguientes veces la
> reutiliza. Es un proyecto Wagtail **vacío**, como el de la guía oficial
> (`wagtail start`): verás la página de bienvenida de Wagtail y el panel `/admin/`,
> no un sitio de demostración. Wagtail 7.4 es una rama LTS con soporte hasta 2027-11-02.
>
> **Instancias antiguas** (creadas con la demo `wagtail/bakerydemo`): no se migran.
> `doctor --security` las señala como fin de vida; para pasar a la imagen nueva,
> haz `wagtail delete <nombre> --force` y vuelve a crearla.

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

**Acceso al panel:** abre `http://127.0.0.1:<puerto>/admin/` (el arranque inicial
tarda ~30-60 s). El usuario es `admin` y la contraseña está en
`/srv/enola-wagtail/<nombre>/secrets/admin_password` (solo legible con `sudo`).

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
sudo enola-cli wagtail publish <NOMBRE> [--ssl] [--auth] [--ssl] [--auth]
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

La imagen `enola/wagtail` lee sus secretos directamente de ficheros, sin pasar por
variables de entorno ni por la línea de comandos:

1. **Generación**: al crear una instancia, se generan 3 secrets
   (db_password, django_secret_key, admin_password) y se escriben en
   `/srv/enola-wagtail/<name>/secrets/` con permisos `0600`.

2. **Propietario**: el proceso corre como el usuario de sistema `enola-wagtail`
   (sin root, sin capabilities), por lo que ese directorio y `media/` son suyos.

3. **Montaje**: los secrets se montan en `/run/secrets/` (solo lectura). Los ajustes
   de Django leen `django_secret_key` y `db_password` de ahí, y el arranque crea el
   superusuario `admin` con `admin_password` la primera vez.

4. **Verificación**: `docker inspect` no muestra contraseñas ni claves en las
   variables de entorno del contenedor.

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
