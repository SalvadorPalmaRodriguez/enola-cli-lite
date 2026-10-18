> **Documento usuario:** `docs/user/ai/commands-ask.md`
> **Versión:** 1.0 | **Actualizado:** 2026-10-18
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** ../general/commands.md, ../general/config-reference.md, ../mcp/commands-mcp.md

# 🤖 `ask` — Asistente de IA local (Ollama)

`enola-cli ask` permite consultar y operar tu infraestructura Enola en lenguaje
natural utilizando un modelo LLM local ejecutado en **Ollama**, sin enviar datos
a servicios en la nube.

Comparte el mismo `ToolRegistry` que el servidor MCP (`enola-cli mcp`), pero
sustituye la política desatendida por un **gate interactivo `[s/N]` en tu
terminal (TTY)** para cualquier operación que modifique el sistema.

---

## Sintaxis

```bash
enola-cli ask <PROMPT> [--model MODEL] [--endpoint URL]
              [--instance NAME] [--no-exec] [--yes]
```

| Argumento / Flag | Descripción |
|------------------|-------------|
| `<PROMPT>` | Pregunta o instrucción en lenguaje natural (ej. `"lista mis servicios tor"`). |
| `--model <MODEL>` | Modelo de Ollama a utilizar (override de `ENOLA_AI_MODEL` / `[ai].model`; por defecto: `llama3.1:8b`). |
| `--endpoint <URL>` | URL HTTP del servidor Ollama (override de `ENOLA_AI_ENDPOINT` / `[ai].endpoint`; por defecto: `http://127.0.0.1:11434`). Soporta direcciones `.onion` vía SOCKS5h automáticamente. |
| `--instance <NAME>` | Nombre de instancia IA gestionada (`enola-ai-<NAME>-ollama`); resuelve su puerto local automáticamente. |
| `--no-exec` | Modo simulación: las llamadas mutantes o destructivas se ejecutan con `try_run` (`ToolRegistry::try_call`), mostrando el plan o `argv` sin efectos secundarios. |
| `-y`, `--yes` | Aprueba operaciones mutantes/destructivas sin preguntar en TTY (para scripts automatizados). |

---

## Configuración (`[ai]`)

Prioridad de resolución (alta → baja):

1. **Flags CLI**: `--endpoint`, `--model`, `--instance`
2. **Variables de entorno**: `ENOLA_AI_ENDPOINT`, `ENOLA_AI_MODEL`, `ENOLA_AI_TIMEOUT_SECS`
3. **Archivo `~/.enola/config.toml`**: sección `[ai]`
4. **Defaults del binario**: `http://127.0.0.1:11434`, `llama3.1:8b`, `120` segundos

```toml
[ai]
endpoint = "http://127.0.0.1:11434"
model = "llama3.1:8b"
timeout_secs = 120
```

Verifica los valores efectivos con:

```bash
enola-cli config-show --json
```

---

## Seguridad y gate interactivo

1. **Operaciones de lectura (`ReadOnly`)**: se ejecutan directamente (`tor_list`,
   `wp_list`, `wp_status`, `ports_list`, `diag_summary`, `verify_onion`,
   `plan_wp_create`, `docs_search`, etc.).
2. **Operaciones mutantes (`Mutating`) y destructivas (`Destructive`)**:
   - Con `--no-exec`: **nunca** modifican el sistema; invocan `try_run` y
     devuelven el plan o `argv` exacto que se ejecutaría.
   - Sin `--no-exec`: exigen una **terminal interactiva (TTY)** (o `--yes`
     explícito; sin TTY y sin `--yes` falla cerrado). Antes de ejecutar,
     muestran el nombre de la tool y los argumentos con secretos redactados
     (`sanitize_args` / `redact_argv`), añaden una advertencia reforzada si la
     operación es `Destructive`, y piden confirmación `[s/N]`.
3. **Sin recursión**: la familia `ask` forma parte de `FORBIDDEN_FAMILIES`, por
   lo que `enola_run_command` jamás puede despachar `ask` como subproceso.
4. **Schemas limpios para modelos locales**: las meta-propiedades exclusivas de
   MCP (`confirm_token`, `try_run`) se filtran de los schemas enviados a Ollama
   para mejorar la precisión en modelos locales 8B.

---

## Ejemplos

```bash
# Consultar servicios Tor activos (lectura directa)
enola-cli ask "lista mis servicios tor y dime cuáles están activos"

# Simular la creación de un WordPress sin ejecutar nada (--no-exec)
sudo enola-cli ask "crea un sitio wordpress llamado miblog en el puerto 8090" --no-exec

# Ejecutar con confirmación interactiva [s/N] en TTY
sudo enola-cli ask "crea un sitio wordpress llamado miblog y publícalo en tor"

# Usar un modelo o instancia específica y salida JSON
enola-cli --format json ask "resumen de diagnóstico" --model qwen2.5:7b --instance dev
```

---

## Referencias cruzadas

- [Índice de comandos](../general/commands.md)
- [Referencia de configuración (`config.toml`)](../general/config-reference.md)
- [Servidor MCP (`enola-cli mcp`)](../mcp/commands-mcp.md)
