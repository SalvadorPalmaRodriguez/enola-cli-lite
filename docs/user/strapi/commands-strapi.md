> **Documento usuario:** `docs/user/strapi/commands-strapi.md`
> **Versión:** 2.1 | **Actualizado:** 2026-08-02
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🚀 Strapi — Comandos `enola-cli strapi`

> **Stack:** Node 20 + Strapi 5.49 + Postgres 16. Datos en `/srv/enola-strapi/{name}/`.
> Genera 6 secrets (permisos 0600) por instancia, montados como Docker secrets
> en `/run/secrets/` e inyectados via entrypoint wrapper (no plaintext en env vars).

Strapi es un headless CMS del catálogo Enola. Ideal para APIs de contenido,
aplicaciones Jamstack y backends de contenido desacoplados. Stack: 2 contenedores
(web + Postgres), puerto interno 1337.

---

## `strapi build-image`

Construye la imagen Docker de producción de Strapi (multi-stage, Node 20, Strapi 5.49).
Debe ejecutarse **una vez** antes de `strapi create`. El Dockerfile está embebido en el binario.

```bash
sudo enola-cli strapi build-image [--force]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Fuerza reconstrucción aunque la imagen ya exista localmente |

> El build tarda ~5-10 minutos.

---

## `strapi list`

Lista todas las instancias Strapi con su estado y puerto.

```bash
sudo enola-cli strapi list
```

Sin flags ni argumentos.

---

## `strapi create`

Crea una nueva instancia Strapi con Postgres.

```bash
sudo enola-cli strapi create --name <NOMBRE> --http-port <PUERTO>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre de la instancia (alfanumérico + `_-`) |
| `--http-port` | u16 | Sí | Puerto HTTP interno (Nginx → Docker → Strapi:1337) |

**Ejemplo:**
```bash
sudo enola-cli strapi create --name miapi --http-port 8200
```

---

## `strapi status`

Muestra el estado de una instancia Strapi.

```bash
sudo enola-cli strapi status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `strapi start`

Arranca una instancia Strapi (web + BD).

```bash
sudo enola-cli strapi start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `strapi stop`

Detiene una instancia Strapi (web + BD).

```bash
sudo enola-cli strapi stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `strapi delete`

Elimina una instancia Strapi. Los datos persisten en `/srv/enola-strapi/{name}/`.

```bash
sudo enola-cli strapi delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite el prompt de confirmación |

---

## `strapi publish`

Publica la instancia en Tor como hidden service.

```bash
sudo enola-cli strapi publish <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `strapi hide`

Retira la instancia de Tor (elimina el hidden service).

```bash
sudo enola-cli strapi hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor web | `strapi-{name}` |
| Contenedor BD | `db-{name}-strapi` |
| Datos | `/srv/enola-strapi/{name}/` |
| Puerto interno (Strapi) | `1337` |
| Tor service | `strapi-{name}` |

---

## Gestión de secrets

Strapi no soporta el patrón `_FILE` para leer secrets desde archivos. Enola CLI
usa un **entrypoint wrapper** para inyectar secrets de forma segura:

1. **Generación**: al crear una instancia, se generan 6 secrets aleatorios
   (db_password, app_keys, api_token_salt, admin_jwt_secret, jwt_secret,
   transfer_token_salt) y se escriben en `/srv/enola-strapi/<name>/secrets/`
   con permisos `0600`.

2. **Montaje**: los secrets se montan como Docker secrets en `/run/secrets/`
   (read-only) dentro del contenedor Strapi.

3. **Inyección**: un script `entrypoint.sh` (montado en `/entrypoint.sh`)
   lee los secrets desde `/run/secrets/` y los exporta como variables de entorno
   antes de ejecutar `npm start`.

4. **Verificación**: `docker inspect` no muestra los secrets en las variables
   de entorno del contenedor — solo se ven las variables no sensibles
   (DATABASE_CLIENT, DATABASE_HOST, NODE_ENV, etc.).

Los secrets en disco son para auditoría del usuario. El contenedor los lee
exclusivamente desde `/run/secrets/`.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
