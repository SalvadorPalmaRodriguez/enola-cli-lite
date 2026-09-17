#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════
# install.sh — Instalador oficial de Enola CLI para Linux (Ubuntu/Debian/RHEL)
# ═══════════════════════════════════════════════════════════════════════════
# LAUNCH-021: Descarga el binario release, verifica firma minisign + SHA256,
# instala en /usr/local/bin, registra hash de integridad para INT-008.
#
# DISEO (no toca el portátil del operador, ejecuta TODO en la mquina del usuario):
#   1. Detecta SO (Linux) + arquitectura (x86_64 / aarch64).
#   2. Resuelve URL base de releases (override por env var, default placeholder).
#   3. Descarga: binario + .sha256 + .pqsig (best-effort, v0.5.0+); .minisig salvo ENOLA_INSTALL_NO_VERIFY=1.
#   4. Verifica SHA256 (siempre, sin minisign instalado tambin sirve).
#   5. Verifica firma minisign si est disponible (recomendado).
#   6. Instala en /usr/local/bin/enola-cli (root) o ~/.local/bin (sin sudo).
#   7. Genera /usr/local/share/enola/cli.sha256 para INT-008.
#   7b. Instala /usr/local/share/enola/cli.pqsig (firma PQC, best-effort, v0.5.0+).
#   8. Verifica que `enola-cli --help` arranca.
#
# USO RPIDO (Ubuntu / Debian / RHEL / Arch):
#   curl -fsSL https://tu-dominio.example/install.sh | sudo bash
#
# USO MANUAL (revisar antes de ejecutar):
#   curl -fsSL https://tu-dominio.example/install.sh -o install.sh
#   less install.sh                    # auditar
#   sudo bash install.sh               # ejecutar
#
# OVERRIDE de variables (cada una opcional):
#   ENOLA_INSTALL_BASE_URL  Base donde estn los artefactos (default: placeholder)
#   ENOLA_INSTALL_VERSION   Versin a instalar (default: latest)
#   ENOLA_INSTALL_PREFIX    Prefix de instalacin (default: /usr/local)
#   ENOLA_INSTALL_NO_VERIFY Saltar verificacin minisign (DESACONSEJADO)
#   ENOLA_INSTALL_PUBKEY    Clave pblica minisign (default: la del repo)
#   ENOLA_INSTALL_STRICT_PUBKEY  Abortar si ENOLA_INSTALL_PUBKEY difiere del default (CI)
#   ENOLA_INSTALL_SKIP_DEPS Saltar bootstrap de dependencias (enola-cli setup)
#   ENOLA_INSTALL_FORCE_DOWNLOAD  Forzar descarga aunque haya un binario local
#                                 junto al script (modo tarball extrado)
#
# CDIGOS DE SALIDA:
#   0  OK
#   1  Error genrico
#   2  SO o arquitectura no soportada
#   3  Falta dependencia (curl, sha256sum)
#   4  SHA256 mismatch (binario corrupto/manipulado)
#   5  Firma minisign invlida (binario manipulado)
#   6  Falta privilegios para instalar en --prefix
# ═══════════════════════════════════════════════════════════════════════════
set -euo pipefail

# ── Defaults configurables ─────────────────────────────────────────────────
BASE_URL="${ENOLA_INSTALL_BASE_URL:-https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download}"
VERSION="${ENOLA_INSTALL_VERSION:-latest}"
PREFIX="${ENOLA_INSTALL_PREFIX:-/usr/local}"
NO_VERIFY="${ENOLA_INSTALL_NO_VERIFY:-0}"


# Clave pblica minisign del proyecto (la misma publicada en /minisign.pub
# del repo y en el feed: id 34B4F35407C4C064). El operador puede
# sustituirla en su fork.
DEFAULT_PUBKEY="RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e"
PUBKEY="${ENOLA_INSTALL_PUBKEY:-$DEFAULT_PUBKEY}"
STRICT_PUBKEY="${ENOLA_INSTALL_STRICT_PUBKEY:-0}"

# ── Helpers de logging ─────────────────────────────────────────────────────
log()  { printf "  %s\n" "$*"; }
ok()   { printf "  \033[1;32m%s\033[0m %s\n" "✅" "$*"; }
warn() { printf "  \033[1;33m%s\033[0m %s\n" "⚠️ " "$*" >&2; }
err()  { printf "  \033[1;31m%s\033[0m %s\n" "❌" "$*" >&2; }
hdr()  { printf "\n\033[1;36m═══ %s ═══\033[0m\n" "$*"; }

# ── Modo local / offline (tarball extrado) ────────────────────────────────
# Si junto a install.sh hay un binario enola-cli ejecutable, estamos ante un
# tarball cliente extrado: se usa ese binario y NO se descarga nada.
# ENOLA_INSTALL_FORCE_DOWNLOAD=1 fuerza el modo remoto.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOCAL_BIN="$SCRIPT_DIR/enola-cli"
OFFLINE=0
if [ -x "$LOCAL_BIN" ] && [ "${ENOLA_INSTALL_FORCE_DOWNLOAD:-0}" != "1" ]; then
    OFFLINE=1
fi

# SEC-004: Warn when using a custom pubkey that overrides the built-in trust anchor.
if [ -n "${ENOLA_INSTALL_PUBKEY:-}" ] && [ "$ENOLA_INSTALL_PUBKEY" != "$DEFAULT_PUBKEY" ]; then
    warn "Using custom minisign public key (ENOLA_INSTALL_PUBKEY)."
    warn "    This overrides the built-in trust anchor. Only use this in development/testing."
    warn "    In production, remove ENOLA_INSTALL_PUBKEY from your environment."
    if [ "$STRICT_PUBKEY" = "1" ]; then
        err "ENOLA_INSTALL_STRICT_PUBKEY=1 — aborting due to custom pubkey."
        exit 1
    fi
fi


# ── 1. Deteccin de SO y arquitectura ─────────────────────────────────────
hdr "Enola CLI — Instalador (LAUNCH-021)"
log "Detectando entorno…"

OS="$(uname -s)"
case "$OS" in
    Linux) ;;
    *) err "SO no soportado: $OS (solo Linux x86_64/aarch64)"; exit 2 ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)   ARCH_TAG="x86_64-linux" ;;
    aarch64|arm64)  ARCH_TAG="aarch64-linux" ;;
    *) err "Arquitectura no soportada: $ARCH (solo x86_64 / aarch64)"; exit 2 ;;
esac

# Distro (informativo)
DISTRO="desconocida"
if [ -f /etc/os-release ]; then
    # shellcheck disable=SC1091
    DISTRO="$(. /etc/os-release && echo "${PRETTY_NAME:-$ID}")"
fi

ok "SO: Linux ($DISTRO)"
ok "Arch: $ARCH ($ARCH_TAG)"
if [ "$OFFLINE" = "1" ]; then
    ok "Modo: LOCAL — binario detectado junto al instalador ($LOCAL_BIN); no se descargar nada"
else
    ok "Modo: REMOTO — se descargar el binario desde $BASE_URL"
fi

# ── 2. Verificar dependencias ──────────────────────────────────────────────
hdr "Dependencias"
need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "Falta '$1'. Instala con tu gestor de paquetes:"
        case "$1" in
            curl|sha256sum) log "  Ubuntu/Debian: sudo apt install -y curl coreutils" ;;
            minisign)       log "  Ubuntu/Debian: sudo apt install -y minisign" ;;
        esac
        return 1
    fi
}

if [ "$OFFLINE" != "1" ]; then
    need_cmd curl || exit 3
    ok "curl ($(curl --version | head -1 | awk '{print $2}'))"
fi
need_cmd sha256sum  || exit 3
ok "sha256sum ($(sha256sum --version 2>/dev/null | head -1 || echo 'coreutils'))"

HAS_MINISIGN=0
if command -v minisign >/dev/null 2>&1; then
    HAS_MINISIGN=1
    ok "minisign ($(minisign -v 2>&1 | head -1))"
else
    if [ "$NO_VERIFY" = "1" ]; then
        warn "minisign NO instalado y ENOLA_INSTALL_NO_VERIFY=1 — verificacin de firma desactivada."
        warn "Solo se verificar SHA256. Esto NO protege contra una compromiso del servidor."
    else
        warn "minisign NO instalado. Recomendado: sudo apt install minisign"
        warn "Sin minisign, una compromiso del servidor de releases pasara desapercibida."
        warn "Para forzar instalacin sin firma: ENOLA_INSTALL_NO_VERIFY=1 sudo bash install.sh"
        # En modo local la autenticidad la aporta la firma del tarball, no hace falta minisign.
        [ "$OFFLINE" != "1" ] && exit 5
    fi
fi


TMP="$(mktemp -d -t enola-install.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

if [ "$OFFLINE" = "1" ]; then
    # ── Modo local: binario y sha256 vienen en el tarball extrado ──
    hdr "Origen local (tarball extrado)"
    cp "$LOCAL_BIN" "$TMP/enola-cli"
    if [ ! -f "$SCRIPT_DIR/enola-cli.sha256" ]; then
        err "Tarball incompleto: falta $SCRIPT_DIR/enola-cli.sha256"
        log "El tarball cliente debe incluir enola-cli.sha256. Vuelve a descargarlo o"
        log "fuerza el modo remoto con: ENOLA_INSTALL_FORCE_DOWNLOAD=1 sudo bash install.sh"
        exit 3
    fi
    cp "$SCRIPT_DIR/enola-cli.sha256" "$TMP/enola-cli.sha256"
    VERSION="$("$LOCAL_BIN" --version 2>/dev/null | awk '{print $NF}')"
    VERSION="${VERSION:-local}"
    ok "Binario local: $LOCAL_BIN (versin: $VERSION)"
    # T1-via-A (D3): el .pqsig del binario crudo es un asset de release, no va
    # en el tarball — la autenticidad PQC la aporta la firma del tarball
    # (*.tar.gz.pqsig), verificada ANTES de extraer.
    log "Modo local: el .pqsig del binario crudo es un asset de release, no parte del tarball."
    log "  La autenticidad PQC la aporta la firma del tarball (*.tar.gz.pqsig), verificada antes de extraer."
fi

# ── 3. Resolver URL de descarga ────────────────────────────────────────────
if [ "$OFFLINE" != "1" ]; then
hdr "Resolucin de versin"
if [ "$VERSION" = "latest" ]; then
    # Convencin: el operador publica un fichero LATEST con el tag de la
    # versin actual. Si no existe, fallar con mensaje claro.
    if ! VERSION="$(curl -fsSL "${BASE_URL}/LATEST" 2>/dev/null | tr -d '[:space:]')"; then
        err "No se pudo resolver versin 'latest' desde ${BASE_URL}/LATEST"
        log "Especifica versin manualmente: ENOLA_INSTALL_VERSION=v1.4.0 sudo bash install.sh"
        exit 1
    fi
    [ -z "$VERSION" ] && { err "Fichero LATEST vaco en ${BASE_URL}/LATEST"; exit 1; }
fi
ok "Versin: $VERSION"

BIN_NAME="enola-cli-${VERSION}-${ARCH_TAG}"
BIN_URL="${BASE_URL}/${BIN_NAME}"
SHA_URL="${BIN_URL}.sha256"
SIG_URL="${BIN_URL}.minisig"
PQSIG_URL="${BIN_URL}.pqsig"

log "Binario:  $BIN_URL"
log "SHA256:   $SHA_URL"
log "Firma:    $SIG_URL"
log "PQC:      $PQSIG_URL"
fi  # OFFLINE (resolución remota)

# ── 4. Descargar a un temporal ─────────────────────────────────────────────
if [ "$OFFLINE" != "1" ]; then
hdr "Descarga"
log "Trabajando en $TMP"
curl -fSL --progress-bar "$BIN_URL" -o "$TMP/enola-cli"        || { err "Descarga binario fall"; exit 1; }
curl -fSL --silent       "$SHA_URL" -o "$TMP/enola-cli.sha256" || { err "Descarga .sha256 fall"; exit 1; }
if [ "$NO_VERIFY" != "1" ]; then
    curl -fSL --silent "$SIG_URL" -o "$TMP/enola-cli.minisig" || { err "Descarga .minisig fall"; exit 1; }
fi
ok "Descargados $(du -h "$TMP/enola-cli" | awk '{print $1}') de binario"
# T1-via-A (D2): firma PQC del binario crudo, best-effort — releases < v0.5.0
# no publican .pqsig → 404 → warn y continuar (minisign+SHA256 siguen siendo
# el trust anchor). Fuera del guard NO_VERIFY a proposito: ese flag salta la
# verificacion, no la entrega de artefactos.
if curl -fSL --silent "$PQSIG_URL" -o "$TMP/enola-cli.pqsig" 2>/dev/null; then
    ok "Firma PQC descargada (.pqsig)"
else
    warn "No se pudo descargar .pqsig (release anterior a v0.5.0?)."
    warn "  La verificacion post-cuantica del binario no estara disponible."
    warn "  minisign + SHA256 siguen protegiendo esta instalacion."
    rm -f "$TMP/enola-cli.pqsig"
fi
fi  # OFFLINE

# ── 5. Verificar SHA256 ────────────────────────────────────────────────────
hdr "Verificacin de integridad"
EXPECTED_SHA="$(awk '{print $1}' "$TMP/enola-cli.sha256")"
ACTUAL_SHA="$(sha256sum "$TMP/enola-cli" | awk '{print $1}')"
if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
    err "SHA256 MISMATCH — el binario podra estar corrupto o manipulado."
    log "  Esperado: $EXPECTED_SHA"
    log "  Obtenido: $ACTUAL_SHA"
    exit 4
fi
ok "SHA256 OK ($EXPECTED_SHA)"

# ── 6. Verificar firma minisign ────────────────────────────────────────────
if [ "$OFFLINE" = "1" ]; then
    warn "Modo local: no se verifica la firma minisign del binario aqu."
    warn "La autenticidad la aporta la firma del tarball (*.tar.gz.minisig),"
    warn "que debes verificar ANTES de extraerlo."
elif [ "$NO_VERIFY" != "1" ] && [ "$HAS_MINISIGN" = "1" ]; then
    if minisign -V -m "$TMP/enola-cli" -x "$TMP/enola-cli.minisig" -P "$PUBKEY" >/dev/null 2>&1; then
        ok "Firma minisign vlida (clave $PUBKEY)"
    else
        err "Firma minisign INVLIDA — el binario podra haber sido manipulado."
        log "  Si confas en la fuente y entiendes el riesgo:"
        log "    ENOLA_INSTALL_NO_VERIFY=1 sudo bash install.sh"
        exit 5
    fi
fi

# ── 7. Instalar ────────────────────────────────────────────────────────────
hdr "Instalacin"
INSTALL_DIR="$PREFIX/bin"
SHARE_DIR="$PREFIX/share/enola"
INSTALL_PATH="$INSTALL_DIR/enola-cli"

# Si el usuario no es root y el prefix no es escribible, intentar ~/.local
if [ ! -w "$PREFIX" ] && [ "$(id -u)" != "0" ]; then
    if [ "$PREFIX" = "/usr/local" ]; then
        warn "Sin permisos en $PREFIX — instalando en \$HOME/.local en su lugar."
        warn "Asegrate de que ~/.local/bin est en tu PATH:"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        PREFIX="$HOME/.local"
        INSTALL_DIR="$PREFIX/bin"
        SHARE_DIR="$PREFIX/share/enola"
        INSTALL_PATH="$INSTALL_DIR/enola-cli"
    else
        err "Sin permisos de escritura en $PREFIX. Ejecuta con sudo o cambia ENOLA_INSTALL_PREFIX."
        exit 6
    fi
fi

mkdir -p "$INSTALL_DIR" "$SHARE_DIR"
install -m 0755 "$TMP/enola-cli" "$INSTALL_PATH"
ok "Binario instalado: $INSTALL_PATH"

# INT-008: hash de referencia para self-integrity check
echo "$EXPECTED_SHA" > "$SHARE_DIR/cli.sha256"
chmod 0644 "$SHARE_DIR/cli.sha256"
ok "Hash de integridad: $SHARE_DIR/cli.sha256"

# T1-via-A (D4): firma PQC del binario crudo junto a cli.sha256. Si no hay
# .pqsig nuevo (release < v0.5.0, descarga fallida o modo offline), se ELIMINA
# el cli.pqsig obsoleto — una firma del binario anterior daria falso negativo
# en `enola-cli verify --pqsig` (mismo invariante que U2 en update_checker).
if [ -f "$TMP/enola-cli.pqsig" ]; then
    install -m 0644 "$TMP/enola-cli.pqsig" "$SHARE_DIR/cli.pqsig"
    ok "Firma PQC instalada: $SHARE_DIR/cli.pqsig"
    log "  Verifica post-instalacion: enola-cli verify $INSTALL_PATH --pqsig $SHARE_DIR/cli.pqsig"
else
    rm -f "$SHARE_DIR/cli.pqsig"
fi

# UNINSTALL-FIX-001: instalar script de desinstalacion
# Tarball extrado (plano): $SCRIPT_DIR/uninstall.sh
# Repo:                    $SCRIPT_DIR/../ops/uninstall.sh
UNINSTALL_SRC=""
if [ -f "$SCRIPT_DIR/uninstall.sh" ]; then
    UNINSTALL_SRC="$SCRIPT_DIR/uninstall.sh"
elif [ -f "$SCRIPT_DIR/../ops/uninstall.sh" ]; then
    UNINSTALL_SRC="$SCRIPT_DIR/../ops/uninstall.sh"
fi
if [ -n "$UNINSTALL_SRC" ]; then
    install -m 0755 "$UNINSTALL_SRC" "$SHARE_DIR/uninstall.sh"
    ok "Script de desinstalacion: $SHARE_DIR/uninstall.sh"
else
    warn "No se encontr uninstall.sh junto al instalador — no se instala desinstalador."
    warn "Puedes desinstalar manualmente borrando $INSTALL_PATH y $SHARE_DIR."
fi


# ── 8. Smoke test ──────────────────────────────────────────────────────────
hdr "Verificacin post-instalacin"
if "$INSTALL_PATH" --version >/dev/null 2>&1 || "$INSTALL_PATH" --help >/dev/null 2>&1; then
    ok "$($INSTALL_PATH --version 2>/dev/null || echo 'enola-cli arranca correctamente')"
else
    err "El binario no arranca. Posible incompatibilidad de glibc."
    log "  Comprueba con: ldd $INSTALL_PATH"
    exit 1
fi

# ── 8b. Bootstrap de dependencias del sistema (INSTALL-012) ────────────────
# El bootstrap lo hace el propio binario instalado:
#   enola-cli setup              → scope Core: docker.io, nginx, tor, curl, openssl
#   enola-cli setup --security   → scope Security: ufw, apparmor, apparmor-utils
# (setup --vpn queda opt-in deliberadamente: wireguard/qrencode/socat)
# Es idempotente: si ya están instalados es no-op.
# IMPORTANTE: registramos qué deps existían ANTES de instalar, para que el
# manifiesto sepa cuáles instaló Enola y cuáles ya las tenía el usuario.
DEPS_BEFORE=""
for dep in docker nginx tor ufw; do
    if command -v "$dep" >/dev/null 2>&1; then
        DEPS_BEFORE="${DEPS_BEFORE}${dep},"
    fi
done

if [ "$(id -u)" = "0" ] && [ "${ENOLA_INSTALL_SKIP_DEPS:-0}" != "1" ]; then
    hdr "Instalación de dependencias del sistema"
    if "$INSTALL_PATH" setup; then
        ok "Dependencias Core listas (Docker / Tor / Nginx / curl / openssl)"
    else
        warn "'enola-cli setup' falló; el binario está instalado pero algunas"
        warn "dependencias del sistema podrían faltar. Re-ejecuta manualmente:"
        warn "  sudo enola-cli setup"
    fi
    if "$INSTALL_PATH" setup --security; then
        ok "Dependencias Security listas (UFW / AppArmor)"
    else
        warn "'enola-cli setup --security' falló. Re-ejecuta manualmente:"
        warn "  sudo enola-cli setup --security"
    fi
elif [ "${ENOLA_INSTALL_SKIP_DEPS:-0}" = "1" ]; then
    warn "ENOLA_INSTALL_SKIP_DEPS=1 — saltando bootstrap de dependencias."
    warn "Ejecuta manualmente más tarde: sudo enola-cli setup && sudo enola-cli setup --security"
elif [ "$(id -u)" != "0" ]; then
    warn "No eres root — bootstrap de dependencias OMITIDO."
    warn "Para instalar Docker / Tor / Nginx / UFW / AppArmor automáticamente:"
    warn "  sudo enola-cli setup && sudo enola-cli setup --security"
fi

# ── 8c. Manifiesto de instalación (UNINSTALL-MANIFEST-001) ─────────────────
# Registra qué instaló Enola para que uninstall.sh sepa exactamente qué borrar.
# Solo marca como dep_installed las deps que NO existían antes (Enola las instaló).
# Las deps que el usuario ya tenía NO se marcan → uninstall no las tocará.
MANIFEST="$SHARE_DIR/manifest"
log "Generando manifiesto de instalación: $MANIFEST"
cat > "$MANIFEST" <<MANIFEST_EOF
# enola-cli installation manifest — DO NOT EDIT
# Auto-generated by install.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ)
# Format: key|value
# Used by uninstall.sh to remove only what Enola created.
version|1
installed_at|$(date -u +%Y-%m-%dT%H:%M:%SZ)
binary|$INSTALL_PATH
share_dir|$SHARE_DIR
config_dir|${HOME}/.enola
opt_dir|/opt/enola
MANIFEST_EOF

# Registrar deps instaladas por Enola (no pre-existentes)
for dep in docker nginx tor ufw; do
    if command -v "$dep" >/dev/null 2>&1; then
        if ! echo "$DEPS_BEFORE" | grep -q "$dep"; then
            echo "dep_installed|$dep" >> "$MANIFEST"
            log "  dep_installed|$dep (instalado por Enola)"
        else
            log "  dep_pre_existing|$dep (ya estaba instalado, NO se desinstalará)"
        fi
    fi
done
chmod 0644 "$MANIFEST"
ok "Manifiesto: $MANIFEST"

# ── 9. Mensaje final ───────────────────────────────────────────────────────
hdr "Listo"
cat <<EOF

  Enola CLI  instalado correctamente en $INSTALL_PATH

  Primeros pasos:
    enola-cli --version
    enola-cli docs quickstart           # tutorial rpido
    enola-cli docs commands             # ndice de comandos
    enola-cli setup                     # configurar dependencias del sistema

  Si vas a usar Tor o servicios protegidos:
    sudo enola-cli doctor               # diagnstico completo

  Documentacin:    ${BASE_URL%/*}/docs
  Verificar firma: ${BASE_URL%/*}/verify

EOF
# T1-via-A (D6): solo anunciar el verify PQC si cli.pqsig se instalo (en modo
# offline no se instala → no anunciar un comando que apunta a nada).
if [ -f "$SHARE_DIR/cli.pqsig" ]; then
    printf "  Verificar PQC:   enola-cli verify %s --pqsig %s/cli.pqsig
"         "$INSTALL_PATH" "$SHARE_DIR"
fi
exit 0

