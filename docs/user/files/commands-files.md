> **Documento usuario:** `docs/user/files/commands-files.md`
> **Versión:** 2.4 | **Actualizado:** 2026-10-28
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

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

El share se publica con el nombre canónico `fileserver_<NOMBRE>`, que es el
mismo para el servicio Tor, el site Nginx y el certificado
(`/etc/nginx/ssl/fileserver_<NOMBRE>.{crt,key}`). Si lo creaste sin `--ssl`,
puedes activar HTTPS más tarde con `sudo enola-cli tor ssl fileserver_<NOMBRE>`.
`--auth` se gestiona igual que en el resto de servicios (`tor auth … fileserver <NOMBRE>`).

**Seguridad de lo que se sirve** (con y sin `--ssl`): el share es para descargar
ficheros, no para alojar webs. Nginx envía una `Content-Security-Policy` estricta
(`default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; …`),
así que un `.html` compartido **no ejecuta scripts ni estilos incrustados** (sí los
`.js`/`.css` externos del propio share). Los ficheros ocultos (`.algo`) y las
extensiones sensibles (`.env`, `.git`, `.sql`, `.pem`, `.key`, `.crt`, `.der`,
`.p12`, `.pfx`) devuelven `404`. Para publicar una web usa `tor create` o un CMS.
Los shares creados antes de esta versión conservan su configuración anterior:
bórralos y vuelve a crearlos para aplicar estas cabeceras.

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

Elimina un file share: su servicio Tor `fileserver_<nombre>`, su config Nginx y su certificado TLS (`/etc/nginx/ssl/fileserver_<nombre>.{crt,key}`). No toca los de otros servicios.

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
>
> Solo actúa sobre la carpeta del share `fileserver_<NOMBRE>` (por defecto
> `/srv/enola-files/<NOMBRE>`), nunca sobre la de otro servicio con el mismo
> nombre. Se niega si esa carpeta es una ruta del sistema (`/etc`, `/usr`,
> `/var/lib`…), datos de otro servicio de Enola (`/srv/enola-git`…), tu carpeta
> personal entera o la de otro usuario.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
