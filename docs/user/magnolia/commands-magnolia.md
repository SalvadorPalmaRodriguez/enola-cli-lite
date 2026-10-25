> **Documento usuario:** `docs/user/magnolia/commands-magnolia.md`
> **Versión:** 2.3 | **Actualizado:** 2026-10-26
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🌳 Magnolia — Comandos `enola-cli magnolia`

> **Stack:** `ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest` (Tomcat, Java) con H2 embebido. Requiere ≥4 GB RAM.
> Datos en `/srv/enola-magnolia/{name}/`.

Magnolia es un CMS Java empresarial del catálogo Enola. Ideal para sitios corporativos
grandes con flujos de trabajo complejos. Contenedor único (Tomcat), puerto interno 8080.

---

## `magnolia list`

Lista todas las instancias Magnolia con su estado y puerto.

```bash
sudo enola-cli magnolia list
```

Sin flags ni argumentos.

---

> ⚠️ **Imagen sin mantenimiento upstream**: la imagen oficial de Magnolia no se
> actualiza desde 2020 (OpenJDK 11.0.8, Tomcat 9.0.38, sin parches de seguridad). Enola
> la mantiene fijada por digest y muestra un aviso al crear y al publicar; `doctor
> --security` la señala. Evita publicarla en Tor o asume el riesgo.

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
| Puerto interno (Tomcat) | `8080` |
| Tor service | `magnolia-{name}` |

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
