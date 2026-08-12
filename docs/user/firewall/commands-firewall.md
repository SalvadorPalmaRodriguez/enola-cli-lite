> **Documento usuario:** `docs/user/firewall/commands-firewall.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🛡️ Firewall — Comandos `enola-cli firewall`

Gestión de firewall UFW. Aplica políticas seguras por defecto y configura la cadena
DOCKER-USER para evitar que Docker bypasee las reglas.

> Los servicios de Enola bindean a `127.0.0.1` — las reglas UFW no les afectan.
> UFW controla los puertos externos del host.

---

## `firewall setup`

Configura UFW con una política segura por defecto.

Aplica: deny incoming, allow outgoing, allow SSH, configurar cadena DOCKER-USER.

```bash
sudo enola-cli firewall setup [--ssh-port <PUERTO>] [--force]
```

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--ssh-port` | u16 | `22` | Puerto SSH a mantener abierto (anti-lockout) |
| `--force` / `-f` | Bool | `false` | Omite el prompt de confirmación |

**Ejemplos:**
```bash
sudo enola-cli firewall setup
sudo enola-cli firewall setup --ssh-port 2222
sudo enola-cli firewall setup --force
```

---

## `firewall status`

Muestra el estado actual del firewall: activo/inactivo, políticas, reglas y cadena DOCKER-USER.

```bash
sudo enola-cli firewall status
```

Sin flags ni argumentos.

---

## `firewall allow`

Permite tráfico en un puerto.

```bash
sudo enola-cli firewall allow --port <PUERTO> [--proto <PROTO>] [--from <IP/CIDR>]
```

| Flag | Tipo | Obligatorio | Default | Descripción |
|------|------|-------------|---------|-------------|
| `--port` / `-p` | u16 | Sí | — | Puerto a abrir (1-65535) |
| `--proto` | String | No | `tcp` | Protocolo: `tcp`, `udp`, `both` |
| `--from` | String | No | Anywhere | IP o CIDR de origen |

**Ejemplos:**
```bash
sudo enola-cli firewall allow --port 443
sudo enola-cli firewall allow --port 8080 --proto tcp
sudo enola-cli firewall allow --port 5432 --from 192.168.1.0/24
```

---

## `firewall deny`

Deniega tráfico en un puerto.

```bash
sudo enola-cli firewall deny --port <PUERTO> [--proto <PROTO>]
```

| Flag | Tipo | Obligatorio | Default | Descripción |
|------|------|-------------|---------|-------------|
| `--port` / `-p` | u16 | Sí | — | Puerto a cerrar (1-65535) |
| `--proto` | String | No | `tcp` | Protocolo: `tcp`, `udp`, `both` |

**Ejemplos:**
```bash
sudo enola-cli firewall deny --port 23
sudo enola-cli firewall deny --port 3306 --proto tcp
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
