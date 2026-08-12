> **Documento usuario:** `docs/user/test/commands-test.md`
> **Versión:** 1.0 | **Actualizado:** 2026-08-07
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🧪 Test — Comando `enola-cli test`

Ejecución de tests del sistema, benchmarks y gestión de artefactos de test.

---

## `test run`

Ejecuta todos los tests del sistema. Opcionalmente filtra por nombre.

```bash
sudo enola-cli test run
sudo enola-cli test run --filter tor
```

### Flags

| Flag | Descripción | Default |
|------|-------------|---------|
| `-f`, `--filter` | Filtra tests por nombre (substring) | — |

---

## `test list`

Lista los tests disponibles.

```bash
enola-cli test list
```

Sin flags ni argumentos.

---

## `test benchmark`

Ejecuta benchmarks de rendimiento.

```bash
sudo enola-cli test benchmark
```

Sin flags ni argumentos.

---

## `test results`

Muestra los resultados del último test ejecutado.

```bash
enola-cli test results
```

Sin flags ni argumentos.

---

## `test clean`

Limpia los artefactos generados por los tests (contenedores, archivos temporales).

```bash
sudo enola-cli test clean
```

Sin flags ni argumentos.

---

## Ver también

- [Diagnósticos del sistema](../diag/commands-diag.md)
- [Mantenimiento](../maintenance/commands-maintenance.md)
