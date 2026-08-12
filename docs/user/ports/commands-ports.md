> **Documento usuario:** `docs/user/ports/commands-ports.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🔌 Ports — Comandos `enola-cli ports`

Gestión de puertos usados por los servicios de Enola.

---

## `ports list`

Lista todos los puertos usados por los servicios Enola (cadena Tor→Nginx→App).

Incluye contenedores activos y detenidos (los detenidos retienen sus port bindings de Docker).

Columnas: Service | Type | Role | Port | Interface | Status

Roles:
- `onion-http` = puerto virtual en la URL `.onion` (lo que ve el visitante)
- `nginx-listen` = puerto de escucha de Nginx (Tor→Nginx, interno)
- `backend` = puerto del contenedor Docker (Nginx→App, interno)
- `ssh` = puerto SSH para servicios Git

```bash
sudo enola-cli ports list [--json]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--json` | Bool | Salida en formato JSON en vez de tabla |

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
