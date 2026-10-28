> **Documento usuario:** `docs/user/magnolia/commands-magnolia.md`
> **Versión:** 3.0 | **Actualizado:** 2026-10-28
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🌳 Magnolia — Comandos `enola-cli magnolia`

> **Stack:** `enola/magnolia:6.4.11-jdk21` (Magnolia Community 6.4 sobre Tomcat 10.1 y Java 21, construido por Enola) + PostgreSQL 16.
> Límite de 3 GB de RAM para el contenedor web. Datos en `/srv/enola-magnolia/{name}/`.

Magnolia es un CMS Java empresarial del catálogo Enola. Ideal para sitios corporativos
grandes con flujos de trabajo complejos. Dos contenedores (web Tomcat + PostgreSQL), puerto interno 8080.

---

## `magnolia list`

Lista todas las instancias Magnolia con su estado y puerto.

```bash
sudo enola-cli magnolia list
```

Sin flags ni argumentos.

---

> ℹ️ **Imagen construida por Enola.** La primera vez que ejecutas `magnolia create`,
> Enola construye `enola/magnolia:6.4.11-jdk21` en tu máquina (~3-6 min, ~900 MB de
> disco, necesita Internet para descargar Magnolia —verificado por SHA-256— y el driver
> de PostgreSQL). Las siguientes veces la reutiliza. Es Magnolia **Community Edition**
> (GPLv3) con el repositorio en PostgreSQL, no con la base H2 embebida.
>
> **Instancias antiguas** (creadas con la imagen `magnolia-docker` de 2020, con H2): no se
> migran —sus datos están en otro formato—. `doctor --security` las señala como fin de
> vida; para pasar a la imagen nueva haz `magnolia delete <nombre> --force` y créala de nuevo
> (el contenido de la instancia vieja no se conserva).

## `magnolia create`

Crea una nueva instancia Magnolia.

```bash
sudo enola-cli magnolia create --name <NOMBRE> --http-port <PUERTO>
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre de la instancia (alfanumérico + `_-`) |
| `--http-port` | u16 | Sí | Puerto HTTP interno (Nginx → Docker → Tomcat:8080) |

**Ejemplo:**
```bash
sudo enola-cli magnolia create --name miweb --http-port 8100
```

**Acceso al panel:** abre `http://127.0.0.1:<puerto>/.magnolia/admincentral`. El primer
arranque instala los módulos en PostgreSQL (~30 s a unos minutos según el equipo); hasta
entonces la página redirige al instalador y después pide usuario y contraseña. El usuario
es `superuser` y la contraseña está en
`/srv/enola-magnolia/<nombre>/secrets/admin_password` (solo legible con `sudo`). Magnolia
no tiene contraseña por defecto: la antigua `superuser` / `superuser` ya no existe.

---

## `magnolia status`

Muestra el estado de una instancia Magnolia.

```bash
sudo enola-cli magnolia status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `magnolia start`

Arranca una instancia Magnolia.

```bash
sudo enola-cli magnolia start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `magnolia stop`

Detiene una instancia Magnolia.

```bash
sudo enola-cli magnolia stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `magnolia delete`

Elimina una instancia Magnolia. Los datos persisten en `/srv/enola-magnolia/{name}/`.

```bash
sudo enola-cli magnolia delete <NOMBRE> [--force]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Omite la comprobación de si la instancia está corriendo. Sin este flag, se aborta si está activa. |

---

## `magnolia publish`

Publica la instancia en Tor como hidden service.

```bash
sudo enola-cli magnolia publish <NOMBRE> [--ssl] [--auth] [--ssl] [--auth]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## `magnolia hide`

Retira la instancia de Tor (elimina el hidden service).

```bash
sudo enola-cli magnolia hide <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la instancia |

---

## Naming convention

| Recurso | Patrón |
|---------|--------|
| Contenedor | `magnolia-{name}` |
| Datos | `/srv/enola-magnolia/{name}/` |
| Contenedor BD | `db-{name}-magnolia` |
| Puerto interno (Tomcat) | `8080` |
| Tor service | `magnolia-{name}` |

---

## Gestión de secrets

La imagen `enola/magnolia` lee sus secretos directamente de ficheros, sin pasar por
variables de entorno ni por la línea de comandos:

1. **Generación**: al crear una instancia se generan 2 secrets (`db_password` y
   `admin_password`) en `/srv/enola-magnolia/<name>/secrets/` con permisos `0600`.
2. **Propietario**: el proceso corre como el usuario de sistema `enola-magnolia` (sin root,
   sin capabilities); ese directorio y `data/` son suyos. La aplicación es de solo lectura.
3. **Montaje**: los secrets se montan en `/run/secrets/` (solo lectura). Magnolia fija la
   contraseña de `superuser` desde `admin_password` la primera vez; al arrancar, el entrypoint
   genera la configuración de PostgreSQL de Jackrabbit con `db_password` (fichero `0600`
   dentro de `data/`).
4. **Verificación**: `docker inspect` no muestra contraseñas ni claves en las variables de
   entorno del contenedor.

---

## Limitaciones conocidas

- Magnolia **no tiene subcomando `edit`** (a diferencia de WordPress, Drupal y Ghost).
  Para cambiar el puerto HTTP: `delete --force` + `create` con el nuevo puerto.
  Los datos persisten en `/srv/enola-magnolia/{name}/`.
- Magnolia **no tiene subcomando `restart`**. Usa `stop` + `start`.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
