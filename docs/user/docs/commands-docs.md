> **Documento usuario:** `docs/user/docs/commands-docs.md`
> **Versión:** 1.0 | **Actualizado:** 2026-08-07
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 📖 Docs — Comando `enola-cli docs`

Documentación integrada en el binario. Funciona **offline** — no requiere conexión a internet.

---

## `docs quickstart`

Muestra la guía de inicio rápido (configuración inicial, primer servicio).

```bash
enola-cli docs quickstart
```

Sin flags ni argumentos.

---

## `docs commands`

Muestra la referencia completa de todos los comandos, con ejemplos.

```bash
enola-cli docs commands              # Todos los comandos
enola-cli docs commands tor          # Solo comandos de Tor
enola-cli docs commands git          # Solo comandos de Git
enola-cli docs commands wp           # Solo comandos de WordPress
```

### Argumentos

| Argumento | Descripción | Grupos disponibles |
|-----------|-------------|-------------------|
| `grupo` | Filtra por grupo de comandos | `tor`, `git`, `wp`, `files`, `ports`, `firewall`, `diag`, `maintenance` |

Si se omite, muestra todos los comandos.

---

## `docs concepts`

Explica conceptos clave de Enola CLI (Tor, puertos, etc.).

```bash
enola-cli docs concepts              # Todos los conceptos
enola-cli docs concepts tor          # Solo conceptos de Tor
enola-cli docs concepts ports        # Solo conceptos de puertos
```

### Argumentos

| Argumento | Descripción | Temas disponibles |
|-----------|-------------|-------------------|
| `tema` | Filtra por tema concreto | `tor`, `ports`, `vpn`, `apparmor`, `docker`, `cms`, `pqc`, `advisories` |

Si se omite, muestra todos los conceptos.

---

## `docs faq`

Muestra preguntas frecuentes y solución de problemas.

```bash
enola-cli docs faq                   # Toda la FAQ
enola-cli docs faq sudo              # Filtrar por "sudo"
enola-cli docs faq onion             # Filtrar por "onion"
```

### Argumentos

| Argumento | Descripción |
|-----------|-------------|
| `filtro` | Término de búsqueda para filtrar la FAQ |

Si se omite, muestra toda la FAQ.

---

## `docs examples`

Muestra ejemplos de uso por caso de uso.

```bash
enola-cli docs examples              # Todos los ejemplos
enola-cli docs examples wordpress    # Solo ejemplos de WordPress
enola-cli docs examples git-server   # Solo ejemplos de servidor Git
```

### Argumentos

| Argumento | Descripción | Casos disponibles |
|-----------|-------------|-------------------|
| `caso` | Filtra por caso de uso | `deploy`, `git-server`, `wordpress`, `backup`, `firewall`, `files`, `cms`, `vpn`, `apparmor`, `update`, `web` |

Si se omite, muestra todos los ejemplos.

---

## Ver también

- [Referencia completa de comandos](../general/commands.md)
- [Conceptos clave](../general/concepts.md)
- [FAQ](../general/faq.md)
- [Ejemplos de uso](../guia/examples.md)
