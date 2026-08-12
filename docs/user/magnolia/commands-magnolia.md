> **Documento usuario:** `docs/user/magnolia/commands-magnolia.md`
> **Versión:** 2.1 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🌳 Magnolia — Comandos `enola-cli magnolia`

> **Stack:** `magnolia-cms:6` (Tomcat). Requiere ≥4 GB RAM.
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
| `--force` / `-f` | Bool | Omite el prompt de confirmación |

---

## `magnolia publish`

Publica la instancia en Tor como hidden service.

```bash
sudo enola-cli magnolia publish <NOMBRE>
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
