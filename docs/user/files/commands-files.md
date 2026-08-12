> **Documento usuario:** `docs/user/files/commands-files.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 📁 Files — Comandos `enola-cli files`

> **Stack:** Nginx autoindex. Datos en `/srv/enola-files/{name}/`.
> Arquitectura: `.onion:80 → Nginx → /srv/enola-files/{name}`
>
> **Seguridad**: los paths de file shares se validan con
> canonicalización léxica (colapsa `.` y `..`) y, si el path ya existe,
> `canonicalize` para resolver symlinks. Path traversal y TOCTOU están bloqueados.

Servidor de archivos anónimo accesible via Tor. Nginx sirve un directorio con
directory listing automático. Ideal para compartir archivos sin exponer la IP.

---

## `files list`

Lista todos los file shares con su dirección `.onion`, puerto Nginx y estado.

```bash
sudo enola-cli files list
```

Sin flags ni argumentos.

---

## `files create`

Crea un nuevo file share accesible via Tor.

```bash
sudo enola-cli files create --name <NOMBRE> [--auth] [--ssl]
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` / `-n` | String | Sí | Nombre del share (directorio en `/srv/enola-files/`) |
| `--auth` / `-a` | Bool | No | Habilita autorización de cliente Tor (solo clientes autorizados) |
| `--ssl` | Bool | No | Habilita HTTPS con certificado self-signed (TLSv1.3, añade :443) |

**Después de crear**, coloca archivos en `/srv/enola-files/<name>/` para servirlos.

**Ejemplos:**
```bash
sudo enola-cli files create --name myshare
sudo enola-cli files create --name myshare --ssl
sudo enola-cli files create --name myshare --auth
sudo enola-cli files create --name myshare --ssl --auth
```

---

## `files edit`

Muestra o cambia la configuración de un file share.

```bash
sudo enola-cli files edit <NOMBRE> [--port <PUERTO>]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del share |

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--port` / `-p` | u16 | Nuevo puerto Nginx (sin flag, solo muestra la config actual) |

> La dirección `.onion` no cambia — solo el puerto interno de Nginx.

**Ejemplos:**
```bash
sudo enola-cli files edit myshare                 # mostrar config actual
sudo enola-cli files edit myshare --port 18080    # cambiar puerto Nginx
```

---

## `files delete`

Elimina un file share (config Nginx + hidden service Tor).

```bash
sudo enola-cli files delete <NOMBRE> --force
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del share |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--force` / `-f` | Bool | Sí | Confirmación obligatoria — previene borrado accidental |

> El directorio `/srv/enola-files/<name>/` **no se borra** automáticamente. Elimínalo manualmente si no lo necesitas.

---

## `files fix-perms`

Corrige permisos y ownership del directorio del file share.

```bash
sudo enola-cli files fix-perms <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre del share |

> Establece `root:www-data` con permisos `750` para que Nginx pueda leer los archivos.
> Ejecútalo después de copiar archivos manualmente al directorio del share.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
