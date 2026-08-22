# 🛡️ Política de Seguridad — Enola CLI

> **Versión**: 3.1 (2026-08-04)
> **Audiencia**: usuarios finales, investigadores de seguridad, periodistas.
> **English:** [`docs/en/security-model.md`](../../en/security-model.md)

---

## ¿Qué es Enola CLI?

Enola CLI es una herramienta de línea de comandos para autohospedar servicios
(WordPress, Forgejo/Git, CMS) detrás de Tor en tu propio hardware,
sin depender de proveedores cloud. Está distribuido como binario único
(Linux x86_64) firmado digitalmente.

---

## Lo que protegemos por diseño

### 1. Tu nombre de usuario y rutas locales

El binario se compila con un perfil endurecido que:

- Elimina **todos los símbolos de debug** (`strip = "symbols"`).
- Reescribe las rutas absolutas del desarrollador (`--remap-path-prefix`):
  no aparece `/home/<dev>/.cargo/registry/...` ni `/home/<dev>/.rustup/...`
  en el binario distribuido.
- Compila con LTO "fat" + `opt-level = "z"` + `panic = "abort"` para
  encarecer el reverse engineering.
- Para releases distribuibles públicamente, se compila dentro de
  `Dockerfile.build` con `WORKDIR /build` para que **ningún path del
  build host** quede embebido (incluidos los del crate `openssl-sys` con
  feature `vendored`).

**Verificación reproducible** (cualquier usuario con shell Linux):

```bash
BIN=enola-cli   # ruta a tu binario descargado
strings "$BIN" | grep -c '/home/'        # esperado: 0 (o solo paths de AppArmor profiles internos)
strings "$BIN" | grep -c '\.cargo/'      # esperado: 0
nm "$BIN" 2>&1 | head -1                 # esperado: "no symbols"
file "$BIN" | grep -q stripped && echo OK
```

### 2. Tus credenciales y configuración

- **Configuración sensible** en `~/.enola/` con permisos `0600`.
- **Configuración** en `~/.enola/config.toml` con permisos `0600`.
- En el código: cualquier valor con sufijo `_token`, `_secret`, `_password`,
  `_key` se muestra como `[REDACTED]` en `enola-cli config-show` y en logs.
- **Nunca** se envían credenciales por logs o tracing — patrón obligatorio en
  `infrastructure::http` y todos los adaptadores.

### 2.1 Credenciales admin de Forgejo

- Las credenciales admin de Forgejo se almacenan como **hash bcrypt** (cost 12),
  nunca como plaintext, en `/srv/enola-git/<name>/.enola-admin-creds`.
- Al ejecutar comandos `git user list/create/delete` sin `--admin-pass`,
  el CLI **pide la contraseña interactivamente** y la verifica contra el hash.
- Compatibilidad: si existe un archivo con formato anterior (`ADMIN_PASS=plaintext`),
  se usa directamente sin prompt (migración transparente).
- Todo admin de Forgejo se crea con `--must-change-password=true`,
  forzando rotación de contraseña en el primer login.

### 2.2 Secrets en contenedores CMS

- Strapi y Wagtail no soportan el patrón `_FILE` para leer secrets desde
  archivos. Enola CLI usa un **entrypoint wrapper** que:
  1. Monta los secrets como Docker secrets en `/run/secrets/` (read-only).
  2. Los lee y los exporta como variables de entorno dentro del contenedor.
  3. Ejecuta `exec "$@"` para ceder el control al comando original.
- Esto evita que los secrets aparezcan como plaintext en `docker inspect`,
  que expone todas las variables de entorno del contenedor.
- Los archivos de secrets se generan con permisos `0600` en
  `/srv/enola-strapi/<name>/secrets/` y `/srv/enola-wagtail/<name>/secrets/`.

### 3. Tu red y paths de archivos

- **Docker bind a `127.0.0.1` SIEMPRE** — los contenedores nunca exponen
  puertos a `0.0.0.0`. Solo Tor (vía hidden services) o nginx (vía localhost)
  pueden hablar con ellos. Ver §13.16 de las instrucciones internas.
- **UFW + DOCKER-USER**: el comando `enola-cli firewall setup` aplica una
  política `deny incoming` y configura la chain `DOCKER-USER` para impedir
  que un contenedor comprometido bypaseé el firewall.
- **AppArmor**: perfiles obligatorios para los contenedores de servicio
  (Forgejo, WordPress) cuando el kernel lo soporta.
- **Tor v3 hidden services**: las URLs `.onion` son direcciones efímeras y
  no se publican en DNS. La privacidad la da Tor, no nosotros.
- **Validación de paths**: los paths de file shares
  se validan con canonicalización léxica (colapsa `.` y `..`) y, si el path
  ya existe en disco, con `canonicalize` para resolver symlinks. Esto previene
  ataques de path traversal y TOCTOU en `/srv/enola-files/`.

### 4. Integridad del binario y trust anchors

- **Firma digital `minisign`** (clave Ed25519): cada binario y cada archivo
  Docker tiene su `.minisig`. Verifica con la clave pública en `minisign.pub`.
- **Firma post-cuántica ML-DSA-65** (FIPS 204): además de minisign, ofrecemos
  firma dual lista para la era post-cuántica.
- **`enola-cli verify <archivo>`**: verifica la firma post-cuántica ML-DSA-65
  (clave pública embebida) y, si existe el `.sha256` hermano, la integridad,
  usando solo el binario distribuido. No requiere `enola-sign-pqc` ni red.
- **Validación de minisign**: si se establece
  `ENOLA_MINISIGN_BIN` con un path absoluto, el CLI valida que el binario
  exista, sea ejecutable y sea realmente minisign (identity check via `<bin> -V`).
  Si falla, emite warning y fallback a `minisign` del PATH.
- **Trust anchor del instalador**: si
  `ENOLA_INSTALL_PUBKEY` se establece con un valor distinto del default,
  el instalador emite un warning visible. `ENOLA_INSTALL_STRICT_PUBKEY=1`
  aborta la instalación (modo CI).
- **Self-integrity check**: el binario verifica su propio hash al arrancar
  (`check_self_integrity()` en `build.rs` + runtime).
- **Anti-debug runtime**: al arrancar, el binario:
  - Llama `prctl(PR_SET_DUMPABLE, 0)` → bloquea **core dumps** y **ptrace attach**.
    Si el proceso crashea, no se escribe `core` con la memoria del proceso —
    passphrases y claves en RAM no se filtran al disco.
  - Lee `/proc/self/status` y aborta con exit code 2 si detecta `TracerPid != 0`
    (gdb, strace, ltrace, rr, ... ya adjuntos al arrancar).

  Verifícalo tú mismo:
  ```bash
  strace ./enola-cli --version
  # → "exited with 2" — el binario detecta strace y aborta.
  ./enola-cli --version
  # → "enola 1.0.x" — funcionamiento normal.
  ```
  > **Nota honesta**: esta capa **no impide** el reverse engineering. Un
  > atacante puede recompilar Rust o parchear el binario en disco. Es una
  > capa de defensa en profundidad para el caso común (atacante oportunista
  > intentando volcar memoria de un proceso en ejecución).
- Más detalle: `docs/user/verify-downloads.md`.

---

## Modelo de amenazas — qué NO te protege

Sé honesto contigo mismo sobre lo que esta herramienta puede y no puede hacer.

| Amenaza | ¿Te protege Enola CLI? |
|---------|------------------------|
| Atacante que controla tu máquina con privilegios de root | **No.** Si tienen root, leen todo. |
| Atacante que controla tu ISP | Parcial. Si usas Tor para todo, sí. Si haces clear-net, no. |
| Atacante que rompe Tor (NSA-level) | No. Eso depende de Tor, no de nosotros. |
| Reverse engineering del binario | **No por completo.** El hardening encarece pero no impide. Un atacante puede parchear el binario en su propia máquina. Esto es aceptado por diseño: el modelo es de "buena fe + auditoría", no DRM. |
| Robo de credenciales desde tu disco | Si tu disco está cifrado y `~/.enola/` es 0600, sí. Si tu sistema está comprometido a nivel root, no. |
| MITM en conexiones de red | Sí, mediante TLS. El CLI fuerza `rustls-tls` en todas las conexiones. |
| Quantum adversary (Q-day) | Parcial. Tor v3 + SSH ya tienen modos PQC; los releases tienen firma dual ML-DSA. Faltan TLS PQC en nginx (esperando OpenSSL 3.5 LTS) y certificados PQC en CAs (esperando ~2027). Plan completo: `docs/user/quantum-security.md`. |

---

## Reportar vulnerabilidades — Divulgación Coordinada

> **Importante**: El uso de Enola CLI está sujeto a una licencia propietaria
> que exige **divulgación coordinada**. Al usar el software, aceptas estos
> términos. Ver [LICENSE](../../../LICENSE) §5.

Si encuentras un fallo de seguridad:

1. **NO abras un issue público** en Forgejo/GitHub.
2. **NO publiques ni compartas** el fallo en ningún canal (foros, redes
   sociales, blogs, chats) hasta que haya sido remediado.
3. Reporta el fallo **dentro de las 72 horas** del descubrimiento por uno
   de estos canales privados:
   - **Tor**: la URL `.onion` del proyecto está publicada en el sitio web
     oficial (`docs/user/verify-downloads.md` lista el dominio canónico).
   - **Email cifrado con PGP** al maintainer:

**Clave PGP del maintainer** (publicada 2026-04-28):
```
Fingerprint: 6101 0A8C D06A 8E27 563D C9CC 7C2D E4F2 DC40 C81B
Email:       salvadorpalmarodriguez@gmail.com
Tipo:        RSA 4096 + subclave RSA 4096
Expira:      2030-04-28 (4 años, renovable)
Keyserver:   hkps://keys.openpgp.org
```
Descarga la clave pública desde el keyserver:
```bash
gpg --keyserver hkps://keys.openpgp.org --recv-keys 61010A8CD06A8E27563DC9CC7C2DE4F2DC40C81B
gpg --fingerprint 61010A8CD06A8E27563DC9CC7C2DE4F2DC40C81B  # Verificar que coincide
```

4. El maintainer acusará recibo en un plazo razonable (objetivo: 7 días)
   y trabajará contigo para resolver el fallo.
5. **Solo después** de que el fallo haya sido remediado y con consentimiento
   escrito del maintainer, podrá divulgarse públicamente.
6. Damos crédito en las notas de la versión salvo que pidas anonimato.

**Bug bounty**: actualmente no hay programa formal de recompensas
(autohospedado, presupuesto pequeño). Estamos abiertos a disclosures
coordinados y reconocemos la contribución públicamente.

---

## Auditorías y dependencias

- **`cargo audit`** y **`cargo deny`** ejecutados antes de cada release
  vía `bash scripts/supply-chain-check.sh`.
  Si hay CVEs en dependencias, el release se **bloquea** hasta resolver.
- **`cargo deny`** verifica además licencias (allow-list compatible con
  binario propietario), bans (versiones duplicadas) y sources (solo crates.io).
  Configuración en `deny.toml`.
- **CVEs históricos**: política estándar es `cargo update -p <crate>` y
  verificación inmediata.
- **OpenSSL vendored**: el binario compila OpenSSL desde `openssl-src-rs`.
  Runbook de respuesta a CVEs interno del maintainer
  (objetivo: bump + rebuild + release en <48h ante CVE crítico).
- **Auditoría externa profesional**: aplazada a futuro, presupuesto pendiente.

---

## CrowdSec — Detección de intrusión recomendada

CrowdSec es una herramienta de seguridad **externa** a Enola CLI. No está
integrada en el binario ni en la UI — se instala y gestiona de forma
independiente. Es **altamente recomendada** para cualquier despliegue
expuesto a internet o Tor.

### Instalación

```bash
sudo apt install crowdsec
sudo systemctl enable --now crowdsec
```

### Comandos útiles de `cscli`

#### Decisiones y alertas locales

| Comando | Descripción |
|---------|-------------|
| `cscli decisions list` | Lista las decisiones activas (IPs baneadas, duración, razón) |
| `cscli alerts list` | Lista todas las alertas generadas |
| `cscli alerts show <id>` | Detalle de una alerta específica |
| `cscli metrics` | Métricas de parsers, scenarios y bouncers en tiempo real |

#### Alertas de la comunidad (CrowdSec Central Intel)

| Comando | Descripción |
|---------|-------------|
| `cscli hub list` | Collections, parsers y scenarios instalados |
| `cscli hub show <item>` | Detalle de un item del hub |

#### Bouncers

| Comando | Descripción |
|---------|-------------|
| `cscli bouncers list` | Bouncers registrados y su estado |

#### Métricas del daemon

| Comando | Descripción |
|---------|-------------|
| `cscli metrics show` | Métricas detalladas por componente |
| `journalctl -u crowdsec -f` | Logs del servicio en systemd |

#### API local

| Comando | Descripción |
|---------|-------------|
| `cscli api status` | Estado de la API local (por defecto `127.0.0.1:8080`) |
| `curl http://127.0.0.1:8080/v1/decisions` | Consultar decisiones vía API (requiere API key) |

#### Dashboard web (opcional)

| Comando | Descripción |
|---------|-------------|
| `cscli dashboard setup` | Configura Metabase (requiere Docker) |
| `cscli dashboard start` | Inicia dashboard en `https://127.0.0.1:443` |

### Por qué no está integrado en Enola CLI

CrowdSec es una herramienta del sistema (como `nginx` o `tor`), no un
subcomando de Enola. Mantenerlo externo permite:

- **Actualizaciones independientes**: CrowdSec se actualiza con `apt`,
  sin depender de releases de Enola CLI.
- **Separación de responsabilidades**: Enola CLI gestiona servicios Tor;
  CrowdSec gestiona detección de amenazas.
- **Menor superficie de ataque**: no exponemos `cscli` vía la API web
  de Enola (que escucha en localhost).

### Integración con Nginx

Si usas CrowdSec con Nginx, instala el bouncer de Nginx:

```bash
sudo apt install crowdsec-nginx-bouncer
sudo systemctl reload nginx
```

Esto hace que Nginx bloquee automáticamente las IPs que CrowdSec
ha marcado como maliciosas, devolviendo HTTP 403.

---

## Referencias rápidas para usuarios curiosos

| Tema | Documento público |
|------|-------------------|
| Verificar el binario que descargaste | `docs/user/verify-downloads.md` |
| Estado de seguridad post-cuántica | `docs/user/quantum-security.md` |
| Endurecer tu sistema antes de instalar | `docs/user/guia/quickstart.md` |
| Desinstalación atómica | `docs/user/uninstall.md` |

---
