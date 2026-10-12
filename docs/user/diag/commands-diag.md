> **Documento usuario:** `docs/user/diag/commands-diag.md`
> **Versión:** 2.1 | **Actualizado:** 2026-10-06
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🩺 Diagnostics — Comandos `enola-cli diag`

Diagnósticos y salud del sistema. Verifica el estado de cada componente de Enola.

---

## `diag summary`

Muestra un resumen de todos los servicios.

```bash
sudo enola-cli diag summary
```

Sin flags ni argumentos.

---

## `diag nginx`

Verifica el estado de Nginx.

```bash
sudo enola-cli diag nginx
```

Sin flags ni argumentos.

---

## `diag tor`

Verifica el estado de Tor.

```bash
sudo enola-cli diag tor
```

Sin flags ni argumentos.

---

## `diag ssh`

Verifica el estado de SSH.

```bash
sudo enola-cli diag ssh
```

Sin flags ni argumentos.

---

## `diag wordpress`

Verifica el estado de WordPress.

```bash
sudo enola-cli diag wordpress
```

Sin flags ni argumentos.

---

## `diag wp-sync`

Verifica la sincronización entre WordPress y Nginx.

```bash
sudo enola-cli diag wp-sync
```

Sin flags ni argumentos.

---

## `diag nginx-test`

Testea la configuración de Nginx.

```bash
sudo enola-cli diag nginx-test
```

Sin flags ni argumentos.

---

## `diag resources`

Muestra los recursos del sistema (RAM, disco, GPU).

```bash
sudo enola-cli diag resources
```

Sin flags ni argumentos.

---

## `diag onion`

Sonda una dirección `.onion` a través de Tor (proxy `socks5h://`) y devuelve su
alcanzabilidad. Valida la dirección antes de salir a la red y reintenta con
backoff (un hidden service recién publicado tarda en propagarse).

```bash
sudo enola-cli diag onion abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuv.onion
sudo enola-cli diag onion abc…xyz.onion --format json
```

Argumento posicional: la dirección `.onion` (v3 = 56 caracteres) o una URL
`http(s)://<addr>.onion`.

Con `--format json` devuelve `{url, reachable, status, latency_ms, title,
reason, attempts}`. Cuando no hay respuesta, `reason` distingue `timeout`,
`proxy_unavailable` (el proxy Tor local está caído) y `site_unreachable`.

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
