> **Documento usuario:** `docs/user/diag/commands-diag.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de usuario**

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

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
