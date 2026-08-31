> **Documento usuario:** `docs/user/vpn/commands-vpn.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🔒 VPN — Comandos `enola-cli vpn`

Gestión de túneles WireGuard para acceso remoto seguro. Genera keypairs, escribe
`/etc/wireguard/{name}.conf` y levanta la interfaz con `wg-quick`.

---

## `vpn create`

Crea una nueva interfaz WireGuard y la arranca.

```bash
sudo enola-cli vpn create <NOMBRE> [--port <PUERTO>] [--subnet <CIDR>] [--autostart] [--sync-firewall] [--tor]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la interfaz (max 15 chars, ej: `wg0`, `enola-vpn`) |

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--port` / `-p` | u16 | `51820` | Puerto UDP de escucha |
| `--subnet` / `-n` | String | `10.8.0.0/24` | Subred VPN en notación CIDR |
| `--autostart` / `-a` | Bool | `false` | Habilita autostart en boot (systemd `wg-quick@{name}`) |
| `--sync-firewall` | Bool | `false` | Añade automáticamente regla UFW para el puerto UDP |
| `--tor` | Bool | `false` | Expone la VPN vía Tor (bridge `socat` UDP→TCP + hidden service) |

**Ejemplos:**
```bash
sudo enola-cli vpn create wg0
sudo enola-cli vpn create myvpn --port 51821 --subnet 10.9.0.0/24
sudo enola-cli vpn create myvpn --autostart
sudo enola-cli vpn create myvpn --port 51821 --sync-firewall
sudo enola-cli vpn create wg0 --tor
```

---

## `vpn start`

Arranca una interfaz WireGuard detenida (`wg-quick up`).

```bash
sudo enola-cli vpn start <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la interfaz |

---

## `vpn stop`

Detiene una interfaz WireGuard (`wg-quick down`).

```bash
sudo enola-cli vpn stop <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la interfaz |

---

## `vpn status`

Muestra el estado de una interfaz WireGuard (peers conectados, tráfico).

```bash
sudo enola-cli vpn status <NOMBRE>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la interfaz |

---

## `vpn list`

Lista todas las interfaces WireGuard del sistema.

```bash
sudo enola-cli vpn list
```

Sin flags ni argumentos.

---

## `vpn delete`

Elimina una interfaz WireGuard (detiene + borra config). Irreversible.

```bash
sudo enola-cli vpn delete <NOMBRE> --force [--sync-firewall]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<NOMBRE>` | String | Sí | Nombre de la interfaz |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--force` / `-f` | Bool | Sí | Omite confirmación |
| `--sync-firewall` | Bool | No | Elimina la regla UFW del puerto VPN |

**Ejemplo:**
```bash
sudo enola-cli vpn delete wg0 --force
sudo enola-cli vpn delete wg0 --force --sync-firewall
```

---

## `vpn peer` — Gestión de peers (clientes)

### `vpn peer add`

Añade un nuevo peer a una interfaz VPN. Genera keypair e imprime el `.conf` del cliente.

```bash
sudo enola-cli vpn peer add <INTERFAZ> <NOMBRE_PEER> --endpoint <HOST> [--dns <DNS>] [--psk] [--ip <IP>] [--tor]
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<INTERFAZ>` | String | Sí | Nombre de la interfaz |
| `<NOMBRE_PEER>` | String | Sí | Nombre del peer (ej: `laptop`, `phone`) |

| Flag | Tipo | Obligatorio | Descripción |
|------|------|-------------|-------------|
| `--endpoint` / `-e` | String | Sí | Hostname o IP del servidor para que el cliente conecte |
| `--dns` | String | No | Servidores DNS para el peer (ej: `1.1.1.1`) |
| `--psk` | Bool | No | Añade preshared key extra (capa adicional de seguridad) |
| `--ip` | String | No | IP específica en la subred VPN (auto-asignada si se omite) |
| `--tor` | Bool | No | Genera además un config Tor (`Endpoint = 127.0.0.1`) con instrucciones |

**Ejemplos:**
```bash
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
sudo enola-cli vpn peer add wg0 phone --endpoint 1.2.3.4 --dns 1.1.1.1
sudo enola-cli vpn peer add wg0 server --endpoint myhostname.com --psk
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com --tor
```

---

### `vpn peer add-pubkey`

Añade un peer usando su clave pública existente (el cliente gestiona sus propias claves).

```bash
sudo enola-cli vpn peer add-pubkey <INTERFAZ> <NOMBRE_PEER> <PUBLIC_KEY> <IP>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<INTERFAZ>` | String | Sí | Nombre de la interfaz |
| `<NOMBRE_PEER>` | String | Sí | Nombre del peer |
| `<PUBLIC_KEY>` | String | Sí | Clave pública WireGuard (base64, 44 chars) |
| `<IP>` | String | Sí | IP a asignar en la subred VPN (ej: `10.8.0.5`) |

**Ejemplo:**
```bash
sudo enola-cli vpn peer add-pubkey wg0 myserver PUBKEY_BASE64 10.8.0.5
```

---

### `vpn peer remove`

Elimina un peer de una interfaz por su clave pública.

```bash
sudo enola-cli vpn peer remove <INTERFAZ> <PUBLIC_KEY>
```

| Argumento | Tipo | Obligatorio | Descripción |
|-----------|------|-------------|-------------|
| `<INTERFAZ>` | String | Sí | Nombre de la interfaz |
| `<PUBLIC_KEY>` | String | Sí | Clave pública del peer a eliminar |

---

## Conexión vía Tor (UDP-over-TCP)

WireGuard usa UDP, pero Tor solo transporta TCP. Para exponer una VPN a través
de Tor, `enola-cli` crea un bridge `socat` que convierte el UDP de WireGuard en
TCP y lo publica como hidden service Tor.

**Flujo:** cliente WireGuard → `socat` local (UDP) → Tor SOCKS → `.onion` → Tor → `socat` servidor (TCP) → WireGuard (UDP).

### Servidor

```bash
sudo enola-cli vpn create wg0 --tor
```

Esto crea la interfaz WireGuard, una unidad systemd `enola-vpn-bridge-wg0.service`
(bridge `socat`) y un hidden service Tor (`vpn-wg0`). La dirección `.onion` se
imprime al final.

### Cliente

El cliente necesita Tor y `socat` localmente (Linux/macOS):

```bash
sudo apt install tor socat
sudo systemctl start tor
socat UDP-LISTEN:51820,fork SOCKS4A:127.0.0.1:<ONION>:51820,socksport=9050
```

Después, importa el config Tor generado por:

```bash
sudo enola-cli vpn peer add wg0 laptop --endpoint <IP> --tor
```

El config Tor usa `Endpoint = 127.0.0.1:51820` (el bridge local), no la IP pública.

### Limitaciones

- **Solo clientes desktop** (Linux/macOS). La app WireGuard de móvil no soporta
  SOCKS ni resolución de `.onion`.
- **Latencia**: Tor añade latencia significativa; es esperado.
- El modo directo (`--endpoint <IP>`) sigue disponible y no se ve afectado.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).
- [Inicio rápido](../guia/quickstart.md) — primer sitio en 5 minutos.

---
