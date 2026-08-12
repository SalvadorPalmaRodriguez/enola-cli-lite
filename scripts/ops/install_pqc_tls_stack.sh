#!/usr/bin/env bash
set -euo pipefail

# install_pqc_tls_stack.sh — instala OpenSSL 3.5.x oficial + Nginx compilado contra él
#
# Fuente oficial:
#   - OpenSSL: https://openssl-library.org/source/
#   - Nginx:   https://nginx.org/en/download.html
#
# Uso:
#   sudo bash install_pqc_tls_stack.sh
#   bash install_pqc_tls_stack.sh --dry-run

OPENSSL_VERSION="3.5.7"
NGINX_VERSION="1.28.3"
OPENSSL_BASE_URL="https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}"
OPENSSL_TARBALL="openssl-${OPENSSL_VERSION}.tar.gz"
OPENSSL_SHA256_FILE="${OPENSSL_TARBALL}.sha256"
NGINX_BASE_URL="https://nginx.org/download"
NGINX_TARBALL="nginx-${NGINX_VERSION}.tar.gz"
NGINX_ASC="${NGINX_TARBALL}.asc"
NGINX_KEY_URL="https://nginx.org/keys/nginx_signing.key"
OPENSSL_PREFIX="/opt/enola/openssl-${OPENSSL_VERSION}"
MARKER_DIR="/usr/local/share/enola"
MARKER_FILE="${MARKER_DIR}/pqc_tls.env"
DRY_RUN=0

for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        *) echo "Unknown arg: $arg"; exit 1 ;;
    esac
done

run() {
    if [ "$DRY_RUN" = "1" ]; then
        echo "[dry-run] $*"
    else
        eval "$@"
    fi
}

require_root() {
    if [ "$DRY_RUN" = "1" ]; then
        return
    fi
    if [ "$(id -u)" -ne 0 ]; then
        echo "❌ Este instalador requiere root. Usa sudo."
        exit 1
    fi
}

detect_pm() {
    if command -v apt-get >/dev/null 2>&1; then
        echo apt
    elif command -v dnf >/dev/null 2>&1; then
        echo dnf
    elif command -v pacman >/dev/null 2>&1; then
        echo pacman
    else
        echo unknown
    fi
}

install_prereqs() {
    local pm="$1"
    case "$pm" in
        apt)
            run "apt-get -qq update"
            run "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential curl ca-certificates perl make gcc g++ pkg-config zlib1g-dev libpcre2-dev gnupg nginx"
            ;;
        dnf)
            run "dnf install -y -q gcc gcc-c++ make perl curl ca-certificates pkgconf-pkg-config zlib-devel pcre2-devel gnupg2 nginx"
            ;;
        pacman)
            run "pacman -Sy --noconfirm --quiet base-devel curl perl pkgconf zlib pcre2 gnupg nginx ca-certificates"
            ;;
        *)
            echo "❌ Package manager no soportado. Se necesita apt, dnf o pacman."
            exit 1
            ;;
    esac
}

current_openssl_is_pqc() {
    command -v openssl >/dev/null 2>&1 && openssl version 2>/dev/null | grep -q "OpenSSL 3.5"
}

current_nginx_is_pqc() {
    command -v nginx >/dev/null 2>&1 && nginx -V 2>&1 | grep -q "OpenSSL 3.5"
}

TMPDIR="$(mktemp -d /tmp/enola-pqc-tls-XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

require_root
PM="$(detect_pm)"

echo "══════════════════════════════════════════════════════════"
echo "  🔐 Enola PQC TLS Stack Installer"
echo "  OpenSSL: ${OPENSSL_VERSION} (official source)"
echo "  Nginx:   ${NGINX_VERSION} (official source, linked to OpenSSL 3.5)"
echo "══════════════════════════════════════════════════════════"
echo ""

if current_openssl_is_pqc && current_nginx_is_pqc; then
    echo "✅ Ya existe un stack PQC TLS activo:"
    openssl version || true
    nginx -V 2>&1 | head -1 || true
    run "mkdir -p '${MARKER_DIR}'"
    run "cat > '${MARKER_FILE}' <<'EOF'
OPENSSL_VERSION=${OPENSSL_VERSION}
NGINX_VERSION=${NGINX_VERSION}
OPENSSL_PREFIX=${OPENSSL_PREFIX}
EOF"
    exit 0
fi

echo "📦 Instalando prerrequisitos del sistema..."
install_prereqs "$PM"

echo "⬇️  Descargando OpenSSL ${OPENSSL_VERSION} desde fuente oficial..."
PRECACHE_DIR="/tmp/enola-pqc-precache"
if [ -f "${PRECACHE_DIR}/${OPENSSL_TARBALL}" ] && [ -f "${PRECACHE_DIR}/${OPENSSL_SHA256_FILE}" ]; then
    cp "${PRECACHE_DIR}/${OPENSSL_TARBALL}" "${TMPDIR}/${OPENSSL_TARBALL}"
    cp "${PRECACHE_DIR}/${OPENSSL_SHA256_FILE}" "${TMPDIR}/${OPENSSL_SHA256_FILE}"
    echo "   (usando copia pre-descargada)"
else
    run "curl -fsSL --retry 3 --retry-delay 5 '${OPENSSL_BASE_URL}/${OPENSSL_TARBALL}' -o '${TMPDIR}/${OPENSSL_TARBALL}'"
    run "curl -fsSL --retry 3 --retry-delay 5 '${OPENSSL_BASE_URL}/${OPENSSL_SHA256_FILE}' -o '${TMPDIR}/${OPENSSL_SHA256_FILE}'"
fi

if [ "$DRY_RUN" != "1" ]; then
    EXPECTED_SHA="$(awk '{print $1}' "${TMPDIR}/${OPENSSL_SHA256_FILE}")"
    ACTUAL_SHA="$(sha256sum "${TMPDIR}/${OPENSSL_TARBALL}" | awk '{print $1}')"
    if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
        echo "❌ SHA256 de OpenSSL no coincide"
        echo "   Expected: $EXPECTED_SHA"
        echo "   Got:      $ACTUAL_SHA"
        exit 1
    fi
fi

echo "✅ OpenSSL descargado y verificado"

echo "⬇️  Descargando Nginx ${NGINX_VERSION} desde fuente oficial..."
if [ -f "${PRECACHE_DIR}/${NGINX_TARBALL}" ] && [ -f "${PRECACHE_DIR}/${NGINX_ASC}" ] && [ -f "${PRECACHE_DIR}/nginx_signing.key" ]; then
    cp "${PRECACHE_DIR}/${NGINX_TARBALL}" "${TMPDIR}/${NGINX_TARBALL}"
    cp "${PRECACHE_DIR}/${NGINX_ASC}" "${TMPDIR}/${NGINX_ASC}"
    cp "${PRECACHE_DIR}/nginx_signing.key" "${TMPDIR}/nginx_signing.key"
    echo "   (usando copia pre-descargada)"
else
    run "curl -fsSL --retry 3 --retry-delay 5 '${NGINX_BASE_URL}/${NGINX_TARBALL}' -o '${TMPDIR}/${NGINX_TARBALL}'"
    run "curl -fsSL --retry 3 --retry-delay 5 '${NGINX_BASE_URL}/${NGINX_ASC}' -o '${TMPDIR}/${NGINX_ASC}'"
    run "curl -fsSL --retry 3 --retry-delay 5 '${NGINX_KEY_URL}' -o '${TMPDIR}/nginx_signing.key'"
fi

if [ "$DRY_RUN" != "1" ]; then
    export GNUPGHOME="${TMPDIR}/gnupg"
    mkdir -p "$GNUPGHOME"
    chmod 700 "$GNUPGHOME"
    gpg --batch --import "${TMPDIR}/nginx_signing.key" >/dev/null 2>&1
    # Importar clave de firma nueva (rotación de claves de Nginx 2026)
    if [ -f "${PRECACHE_DIR}/nginx_signing_new.key" ]; then
        gpg --batch --import "${PRECACHE_DIR}/nginx_signing_new.key" >/dev/null 2>&1
    fi
    gpg --batch --verify "${TMPDIR}/${NGINX_ASC}" "${TMPDIR}/${NGINX_TARBALL}" >/dev/null 2>&1 || {
        # Fallback: la clave de firma puede no estar en nginx_signing.key
        # (rotación de claves). Intentar recuperar desde keyserver.
        SIGNER_KEY=$(gpg --batch --status-fd 1 --verify "${TMPDIR}/${NGINX_ASC}" "${TMPDIR}/${NGINX_TARBALL}" 2>&1 | grep "NO_PUBKEY" | awk '{print $NF}' | head -1)
        if [ -n "$SIGNER_KEY" ]; then
            gpg --keyserver keyserver.ubuntu.com --recv-key "$SIGNER_KEY" >/dev/null 2>&1
            gpg --batch --verify "${TMPDIR}/${NGINX_ASC}" "${TMPDIR}/${NGINX_TARBALL}" >/dev/null 2>&1 || {
                echo "❌ Firma GPG de Nginx no válida (clave $SIGNER_KEY no encontrada)"
                exit 1
            }
        else
            echo "❌ Firma GPG de Nginx no válida"
            exit 1
        fi
    }
fi

echo "✅ Nginx descargado y verificado"

echo "🧱 Compilando OpenSSL ${OPENSSL_VERSION}..."
run "tar -xzf '${TMPDIR}/${OPENSSL_TARBALL}' -C '${TMPDIR}'"
run "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && ./Configure --prefix='${OPENSSL_PREFIX}' --openssldir='${OPENSSL_PREFIX}' shared zlib linux-x86_64"
run "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && make -j\$(nproc)"
run "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && make install_sw"

OPENSSL_LIBDIR="${OPENSSL_PREFIX}/lib64"
if [ "$DRY_RUN" != "1" ] && [ ! -d "$OPENSSL_LIBDIR" ]; then
    OPENSSL_LIBDIR="${OPENSSL_PREFIX}/lib"
fi

echo "🔗 Registrando OpenSSL 3.5 en el sistema..."
run "mkdir -p /etc/ld.so.conf.d"
run "printf '%s\n' '${OPENSSL_LIBDIR}' > /etc/ld.so.conf.d/enola-openssl-3.5.conf"
run "ldconfig"
run "ln -sf '${OPENSSL_PREFIX}/bin/openssl' /usr/local/bin/openssl"

echo "🧱 Compilando Nginx ${NGINX_VERSION} contra OpenSSL ${OPENSSL_VERSION}..."
run "tar -xzf '${TMPDIR}/${NGINX_TARBALL}' -C '${TMPDIR}'"
run "cd '${TMPDIR}/nginx-${NGINX_VERSION}' && ./configure \
  --prefix=/usr/share/nginx \
  --sbin-path=/usr/sbin/nginx \
  --conf-path=/etc/nginx/nginx.conf \
  --error-log-path=/var/log/nginx/error.log \
  --http-log-path=/var/log/nginx/access.log \
  --pid-path=/run/nginx.pid \
  --lock-path=/var/lock/nginx.lock \
  --modules-path=/usr/lib/nginx/modules \
  --http-client-body-temp-path=/var/lib/nginx/body \
  --http-proxy-temp-path=/var/lib/nginx/proxy \
  --http-fastcgi-temp-path=/var/lib/nginx/fastcgi \
  --http-uwsgi-temp-path=/var/lib/nginx/uwsgi \
  --http-scgi-temp-path=/var/lib/nginx/scgi \
  --user=www-data \
  --group=www-data \
  --with-threads \
  --with-file-aio \
  --with-http_ssl_module \
  --with-http_v2_module \
  --with-http_realip_module \
  --with-http_auth_request_module \
  --with-http_stub_status_module \
  --with-http_gzip_static_module \
  --with-stream \
  --with-stream_ssl_module \
  --with-stream_ssl_preread_module \
  --with-compat \
  --with-pcre-jit \
  --with-cc-opt='-I${OPENSSL_PREFIX}/include' \
  --with-ld-opt='-Wl,-rpath,${OPENSSL_LIBDIR} -L${OPENSSL_LIBDIR}'"
run "cd '${TMPDIR}/nginx-${NGINX_VERSION}' && make -j\$(nproc)"
run "[ -x /usr/sbin/nginx ] && [ ! -e /usr/sbin/nginx.enola-backup ] && cp /usr/sbin/nginx /usr/sbin/nginx.enola-backup || true"
run "install -m 0755 '${TMPDIR}/nginx-${NGINX_VERSION}/objs/nginx' /usr/sbin/nginx"

if [ "$DRY_RUN" != "1" ]; then
    nginx -V 2>&1 | grep -q "OpenSSL ${OPENSSL_VERSION}" || {
        echo "❌ Nginx no parece estar enlazado con OpenSSL ${OPENSSL_VERSION}"
        nginx -V 2>&1 || true
        exit 1
    }
fi

echo "📝 Guardando marker de PQC TLS..."
run "mkdir -p '${MARKER_DIR}'"
run "cat > '${MARKER_FILE}' <<'EOF'
OPENSSL_VERSION=${OPENSSL_VERSION}
NGINX_VERSION=${NGINX_VERSION}
OPENSSL_PREFIX=${OPENSSL_PREFIX}
OPENSSL_LIBDIR=${OPENSSL_LIBDIR}
EOF"

if [ "$DRY_RUN" != "1" ]; then
    nginx -t >/dev/null 2>&1 || true
    systemctl restart nginx >/dev/null 2>&1 || true
fi

echo ""
echo "✅ PQC TLS stack instalado"
echo "   openssl: $( [ "$DRY_RUN" = "1" ] && echo "OpenSSL ${OPENSSL_VERSION}" || openssl version )"
echo "   nginx:   $( [ "$DRY_RUN" = "1" ] && echo "nginx/${NGINX_VERSION} (built with OpenSSL ${OPENSSL_VERSION})" || nginx -V 2>&1 | head -1 )"
echo "   marker:  ${MARKER_FILE}"

