> **Documento usuario:** `docs/user/mcp/commands-mcp.md`
> **Versión:** 1.6 | **Actualizado:** 2026-10-28
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md, config-reference.md

# 🔌 MCP — Servidor stdio para agentes IA

`enola-cli mcp` arranca un servidor **Model Context Protocol** (MCP) por
**stdio**. Permite que un agente IA externo (Claude Desktop, Cursor, Devin…)
opere la infraestructura del usuario usando **exclusivamente** los comandos de
`enola-cli` y su documentación de usuario — sin shell arbitraria, sin acceso
libre al filesystem y sin red sin restricciones.

## Sintaxis

```bash
enola-cli mcp [--allow-write] [--allow-tools TOOLS] [--allow-all-mutating]
              [--max-output-bytes BYTES] [--audit-log PATH]
              [--request-timeout-secs SECS]
```

| Flag | Descripción |
|------|-------------|
| `--allow-write` | Permite tools que mutan el sistema. Por defecto: **read-only**. |
| `--allow-tools` | Lista CSV de tools permitidas (`wp_create`) o rutas de comando (`wp:create`; `family` pelado para hojas como `doctor`). **Obligatoria** con `--allow-write`. |
| `--allow-all-mutating` | Escape explícito: con `--allow-write`, habilita **todas** las mutantes sin allowlist (muestra un aviso). También `[mcp].allow_all_mutating = true`; no hay variable de entorno. |
| `--max-output-bytes` | Límite de stdout/stderr devuelto por llamada (def: 65536). |
| `--audit-log` | Fichero de auditoría para llamadas mutantes (def: `stderr`; nunca stdout). |
| `--request-timeout-secs` | Timeout por llamada a tool (def: 60). |

Todo se puede fijar también por entorno (`ENOLA_MCP_*`) o por `config.toml`
`[mcp]` — precedencia: **flag > env > fichero > default**. Ver
`config-reference.md` § `[mcp]`.

## Protocolo

- **JSON-RPC 2.0** por stdio: stdout transporta **exclusivamente** JSON-RPC
  (una línea NDJSON por mensaje); los logs van a stderr.
- Métodos: `initialize`, `notifications/initialized`, `ping`, `tools/list`,
  `tools/call`, `resources/list`, `resources/read`.
- `protocolVersion`: `2026-07-28`.

## Tools

**Lectura** (siempre disponibles): `tor_list`, `diag_tor`, `diag_nginx`,
`diag_summary`, `config_show`, `doctor`, `update_check`, `wp_list`,
`wp_status`, `ports_list`, `verify_onion`, `plan_wp_create`,
`plan_tor_create`, `docs_search`.

**Mutantes** (requieren `--allow-write` + allowlist): `wp_create`.

**Destructivas** (requieren `--allow-write` + allowlist, `force: true`
donde la hoja la declare, y **confirmación de dos pasos**): `wp_publish`,
`wp_hide`, `tor_create`, `tor_remove`, `files_create`, y vía
`enola_run_command` las rutas de la tabla de impacto de abajo.

**Vetadas siempre** (`enola_run_command` no las despacha con ninguna
política): las familias `mcp`, `ask`, `ssh`, `uninstall`, `web` y `setup`, y las
rutas `update apply`, `test run` y `test benchmark`, y las que imprimen un
secreto generado (`tor auth generate`, `vpn peer add`, `git user create`):
esas claves y contraseñas acabarían en el contexto del agente. Hazlas desde
la terminal o con los botones del panel web. Tampoco son
expresables las flags `--binary`/`--allow-unsigned` de `update`, ni
ningún valor de flag que empiece por `-`.

**Meta-tools**: `enola_list_commands` (catálogo de familias/subcomandos) y
`enola_run_command` (`{family, subcommand, args}` validado contra el árbol
generado — cobertura de todos los subcomandos, forzando `--format json`;
los comandos mutantes quedan igualmente sujetos a la política).
`enola_run_command` acepta además `try_run` (simulación) y `confirm_token`
(segunda fase de destructivas).

### Tabla de impacto (para dimensionar la allowlist)

Las entradas de `--allow-tools` son nombres de tool (`wp_create`) o rutas
`family:sub` (`family` pelado para hojas; rutas de nivel 3 con espacio:
`tor:auth revoke`).

| Entrada | Clase | Impacto |
|---------|-------|---------|
| `wp:create`, `*:start|stop|restart|edit|build-image` | Mutating | Crea/gestiona contenedores Docker |
| `update:verify-feed` | Mutating | Lee un fichero local o hace fetch http(s) arbitrario (oráculo/SSRF) |
| `tor:create`, `tor:edit`, `tor:rotate` | Destructive | Expone o modifica hidden services `.onion` (publica un puerto local a la red) |
| `tor:auth add|generate|disable` | Destructive | Concede o elimina credenciales de cliente `.onion` |
| `tor:remove`, `tor:auth revoke`, `tor:auth rotate` | Destructive | Elimina hidden services / credenciales (se pierde la `.onion`) |
| `vpn:create`, `vpn:peer add`, `vpn:peer add-pubkey` | Destructive | Abre el servidor VPN / concede acceso de red a terceros |
| `files:create`, `*:publish` | Destructive | Publica ficheros o sitios a la red en un paso |
| `*:delete`, `*:hide`, `git:user delete`, `vpn:peer remove` | Destructive | Destrucción irreversible de datos/recursos |
| `firewall:setup`, `firewall:allow`, `firewall:deny` | Destructive | Abre/cierra puertos del firewall del host |
| `apparmor:setup`, `apparmor:mode` | Destructive | Cambia el confinamiento del kernel (debilita la defensa) |
| `git:create`, `git:registration`, `git:user create` | Destructive | Crea repos/usuarios y concede acceso (incl. `--admin-password`) |
| `maintenance:backup` | Destructive | Exporta datos (tarball de backup) |
| `maintenance:disable-checks`, `maintenance:ssh-harden-pqc`, `maintenance:cleanup` | Destructive | Desactiva chequeos periódicos / modifica sshd / borra ficheros |
| `update:download` | Destructive | Descarga un binario candidato (`update apply` está vetado) |

## Simulación (`try_run`)

Toda tool mutante/destructiva acepta `"try_run": true`. El servidor valida
**los mismos parámetros y permisos** que en una ejecución real y devuelve un
resultado estructurado **sin efectos secundarios**:

```json
{"simulated": true, "reliable": true, "operation": "wp_create",
 "params": {...}, "plan": {...}, "effects": [...], "limitations": ""}
```

- `reliable: true` → el `plan` procede del planificador real (`wp_create`,
  `tor_create`, `enola_run_command` devuelven el `argv` exacto).
- `reliable: false` → simulación limitada; `limitations` explica qué no se
  puede predecir (`wp_publish`, `wp_hide`, `tor_remove`, `files_create`).
- `{"simulated": false, "reason": "..."}` → la tool no admite simulación.
- Una simulación **nunca** crea tokens de confirmación ni ejecuta nada.

## Confirmación de operaciones destructivas

`force: true` por sí solo **no** autoriza una destructiva. Flujo:

1. Primera llamada con los parámetros reales → respuesta
   `{"confirmation_required": true, "confirm_token": "<hex>", "params": {...},
   "expires_in_secs": 300}` (nada se ejecuta).
2. Segunda llamada con **los mismos parámetros** + `confirm_token` → ejecuta.

El token es **monouso**, **expira** (300 s), está **vinculado** a la tool y a
los argumentos exactos (cambiar cualquier parámetro lo invalida) y **no
sobrevive** a un reinicio del servidor. La confirmación humana real la
aporta el cliente MCP que aprueba cada `tools/call`; el token garantiza que
la operación aprobada es exactamente la que se mostró.

> ⚠️ El `confirm_token` **no es una frontera ante un agente hostil**: si el
> llamante puede hacer dos `tools/call`, puede completar ambos pasos. La
> frontera real de seguridad es `--allow-write` + allowlist (y los vetos
> estructurales de `enola_run_command`); el token solo garantiza que lo
> ejecutado es exactamente lo que el operador vio en el challenge.

**Resources**: la documentación de usuario embebida se expone como
`enola://docs/<domain>/<file>` (text/markdown) vía `resources/list` y
`resources/read`.

## Seguridad

- **Read-only por defecto**: sin `--allow-write` ninguna tool puede mutar
  el sistema — el rechazo ocurre *antes* de invocar el caso de uso.
- `--allow-write` exige allowlist (`--allow-tools` / `[mcp].allowed_tools`): sin
  ella `mcp` no arranca. Para habilitar todas las mutantes hay que pedirlo
  explícitamente con `--allow-all-mutating` (aviso al arrancar). Antes de
  v0.10.x una allowlist vacía las habilitaba todas.
- **Schemas estrictos**: cada tool declara `additionalProperties: false`;
  se rechazan parámetros desconocidos, tipos incorrectos y valores fuera
  de rango (`maxLength`, `minimum`/`maximum` en puertos).
- **Destructivas = dos pasos** (ver sección anterior): `force` solo no
  basta; se exige `confirm_token` monouso vinculado a la operación.
- `enola_run_command` **nunca** despacha las familias `mcp`, `ask`, `ssh`,
  `uninstall`, `web`, `setup` ni las rutas `update apply`, `test run`,
  `test benchmark`, `tor auth generate`, `vpn peer add`, `git user create`; las flags `--binary`/`--allow-unsigned` de `update`
  no son expresables, y todo valor de flag que empieza por `-` se
  rechaza. Nunca construye cadenas de shell: argv tipado + validación
  contra el árbol.
- **Entorno del hijo reducido**: los subprocesos despachados corren con
  entorno limpio (`env_clear`) + allowlist (`PATH`, `HOME`, `USER`,
  `SUDO_USER`, `LANG`, `TERM`, `LC_*`, `ENOLA_ACCEPT_LICENSE`) — las
  `ENOLA_*` del servidor no se heredan.
- `files_create` exige `share_path` bajo `/srv/enola-files/` (el CLI
  interactivo admite otras rutas; el agente no).
- Cada llamada mutante queda en el **log de auditoría** (timestamp, tool,
  argumentos con secretos redactados, estado: `rejected`/`simulated`/
  `confirmation_requested`/`authorized`/`ok`/`error`, id de operación) en
  `--audit-log`/stderr. El fichero se crea `0600` y su directorio `0700`
  (O_NOFOLLOW).
- **Política privilegiada**: con euid 0 la sección `[mcp]` solo se lee de
  `/etc/enola/config.toml` (root-owned) — el `~/.enola/config.toml` del
  usuario se ignora.
- El servidor **no** eleva privilegios por su cuenta: opera con los
  privilegios del proceso que lo lanza (si se lanza con `sudo`, las tools
  mutantes y los subprocesos de `enola_run_command` heredan root — limite
  la allowlist en consecuencia).

## Cancelación

Si el cliente muere a mitad de una mutación, la operación puede quedar a
medias: el estado parcial queda registrado en el log de auditoría y **no
hay rollback automático** salvo que el caso de uso lo garantice. Los
subprocesos despachados por `enola_run_command` se matan al expirar el
timeout o al morir el servidor.

## Despliegue con privilegios (recomendado)

Si el servidor debe correr como root, fija la política **fuera del
control del usuario**: argv en un lanzador root-owned, `[mcp]` en
`/etc/enola/config.toml` (la única config que aplica con euid 0),
allowlist **específica** (no vacía) y audit log en directorio root-owned.

```ini
# /etc/systemd/system/enola-mcp.service
[Service]
ExecStart=/usr/local/bin/enola-cli mcp --allow-write \
          --allow-tools wp_create,wp:publish \
          --audit-log /var/log/enola/mcp-audit.log
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=read-only
RestrictAddressFamilies=AF_UNIX AF_INET AF_NETLINK
```

- **Nunca** una regla sudoers con comodín (`enola-cli mcp *`): dejaría al
  usuario elegir `--allow-write`/`--allow-tools` y por tanto la política
  entera.
- Nota honesta: esto **no** es un sandbox de privilegios real — el
  daemon Docker equivale a root. Su valor es fijar argv/env (la política
  no depende de flags elegidos por quien lanza) y centralizar la
  auditoría en journald.
- Si además se usa el dashboard `web` en el mismo host, ojo: su token de
  sesión va a stderr y es legible vía `logs view`/journald.

## Ejemplo de configuración de cliente

```jsonc
// claude_desktop_config.json
{
  "mcpServers": {
    "enola": {
      "command": "sudo",
      "args": ["enola-cli", "mcp", "--allow-write",
               "--allow-tools", "wp_create,wp_publish"]
    }
  }
}
```

## Ejemplos

```bash
# Servidor read-only (solo consultas)
enola-cli mcp

# Permitir mutaciones acotadas con auditoría en fichero
enola-cli mcp --allow-write --allow-tools wp_create,wp_publish \
              --audit-log ~/.enola/mcp-audit.log
```
