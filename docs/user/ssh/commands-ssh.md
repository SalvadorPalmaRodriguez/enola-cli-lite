> **Documento usuario:** `docs/user/ssh/commands-ssh.md`
> **Versión:** 1.0 | **Actualizado:** 2026-09-10
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🔑 SSH — Comandos `enola-cli ssh`

Gestiona el acceso SSH (claves autorizadas) y servicios ocultos SSH sobre Tor.

---

## `ssh add-key`

Añade una clave pública al `~/.ssh/authorized_keys` de un usuario.

- Deduplica por el cuerpo de la clave (no la añade si ya existe).
- Escribe de forma atómica con permisos `0600` y lock anti-TOCTOU.

```bash
sudo enola-cli ssh add-key --user alice --pubkey "ssh-ed25519 AAAA..."
enola-cli ssh add-key --pubkey "ssh-ed25519 AAAA..."
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--user` | String | No | Usuario destino (por defecto: usuario invocador / `SUDO_USER`) |
| `--pubkey` / `-p` | String | Sí | Clave pública (tipo + cuerpo base64 [+ comentario]) |
| `--comment` | String | No | Comentario añadido si la clave no trae uno propio |

> ⚠️ Con `sudo`, sin `--user`, el destino es el usuario que invocó `sudo`
> (`SUDO_USER`), nunca `/root`.

---

## `ssh deploy-hidden`

Publica SSH solo a través de una dirección `.onion`, sin exponerlo a la red
pública. Asegura que `sshd` está activo y mapea `onion_port → local_ssh_port`.

```bash
sudo enola-cli ssh deploy-hidden --name mi-ssh --onion-port 22 --local-ssh-port 2222
```

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--name` | String | Sí | Nombre del servicio |
| `--onion-port` | Integer | Sí | Puerto virtual `.onion` (p. ej. 22) |
| `--local-ssh-port` | Integer | Sí | Puerto local de `sshd` en el host |

---

## Ver también

- [Autorización de cliente Tor](../tor/tor-client-auth.md) — control de acceso a `.onion`.
- [Comandos Tor](../tor/commands-tor.md) — gestión de servicios ocultos.
- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
