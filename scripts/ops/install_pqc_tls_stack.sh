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

# SEC-A1: versiones y valores de verificación FIJADOS en el script (no se fía de
# nada descargado del mismo origen). Al subir de versión, actualizar los cuatro
# valores contrastando dos canales (ver skill `security`, "Instalador PQC TLS").
#   OpenSSL 3.5.9: SHA-256 = .sha256 de la release de GitHub; firma PGP válida de
#                  la clave B146647E… listada en openssl-library.org/source/.
#   nginx 1.30.6:  firma PGP válida (2026-10-02) de D6786CE3… (nginx.org/keys,
#                  contrastada en keyserver.ubuntu.com); SHA-256 calculado tras ella.
OPENSSL_VERSION="3.5.9"
OPENSSL_SHA256="603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a"
NGINX_VERSION="1.30.6"
NGINX_SHA256="3cf46f5ed7504d460125866b7bea99fb357f31d887e2e63b35503310e9b8b6c8"
# Huellas primarias de los firmantes de releases de nginx aceptados (nginx.org/en/pgp_keys.html).
NGINX_SIGNER_FPRS=(
    "D6786CE303D9A9022998DC6CC8464D549AF75C0A" # Sergey Kandaurov (pluknet.key) — firma 1.30.6
    "43387825DDB1BB97EC36BA5D007C8D7C15D87369" # Roman Arutyunyan (arut.key)
)
NGINX_SIGNER_KEY_URLS=(
    "https://nginx.org/keys/pluknet.key"
    "https://nginx.org/keys/arut.key"
)
OPENSSL_BASE_URL="https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}"
OPENSSL_TARBALL="openssl-${OPENSSL_VERSION}.tar.gz"
NGINX_BASE_URL="https://nginx.org/download"
NGINX_TARBALL="nginx-${NGINX_VERSION}.tar.gz"
NGINX_ASC="${NGINX_TARBALL}.asc"
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

# Ejecuta un comando pasado como argv (sin re-evaluar metacaracteres de shell).
run() {
    if [ "$DRY_RUN" = "1" ]; then
        echo "[dry-run] $*"
    else
        "$@"
    fi
}

# Ejecuta un snippet que requiere features de shell (cd &&, redirs, ||).
# Solo para los pocos casos que no pueden expresarse como argv plano. El
# snippet llega como UN solo argumento a `bash -c` (subshell aislado con
# -euo pipefail): a diferencia de `eval "$@"`, no re-parsea argv del caller.
run_sh() {
    if [ "$DRY_RUN" = "1" ]; then
        echo "[dry-run] $1"
    else
        bash -euo pipefail -c "$1"
    fi
}

# Escribe el marker de PQC TLS. Incluye OPENSSL_LIBDIR solo si está definido
# (el marker de salida temprana aún no lo tiene; el final sí).
write_marker() {
    if [ "$DRY_RUN" = "1" ]; then
        echo "[dry-run] escribir ${MARKER_FILE}"
        return
    fi
    mkdir -p "${MARKER_DIR}"
    {
        printf 'OPENSSL_VERSION=%s\n' "${OPENSSL_VERSION}"
        printf 'NGINX_VERSION=%s\n' "${NGINX_VERSION}"
        printf 'OPENSSL_PREFIX=%s\n' "${OPENSSL_PREFIX}"
        if [ -n "${OPENSSL_LIBDIR:-}" ]; then
            printf 'OPENSSL_LIBDIR=%s\n' "${OPENSSL_LIBDIR}"
        fi
    } > "${MARKER_FILE}"
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
            run apt-get -qq update
            run env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential curl ca-certificates perl make gcc g++ pkg-config zlib1g-dev libpcre2-dev gnupg nginx
            ;;
        dnf)
            run dnf install -y -q gcc gcc-c++ make perl curl ca-certificates pkgconf-pkg-config zlib-devel pcre2-devel gnupg2 nginx
            ;;
        pacman)
            run pacman -Sy --noconfirm --quiet base-devel curl perl pkgconf zlib pcre2 gnupg nginx ca-certificates
            ;;
        *)
            echo "❌ Package manager no soportado. Se necesita apt, dnf o pacman."
            exit 1
            ;;
    esac
}

current_openssl_is_pqc() {
    command -v openssl >/dev/null 2>&1 && openssl version 2>/dev/null | grep -q "OpenSSL ${OPENSSL_VERSION}"
}

current_nginx_is_pqc() {
    command -v nginx >/dev/null 2>&1 \
        && nginx -V 2>&1 | grep -q "nginx/${NGINX_VERSION}" \
        && nginx -V 2>&1 | grep -q "OpenSSL ${OPENSSL_VERSION}"
}

# SEC-A1: compara el SHA-256 de un fichero con el valor FIJADO en este script.
verify_sha256() {
    local file="$1" expected="$2" actual
    actual="$(sha256sum "$file" | awk '{print $1}')"
    if [ "$actual" != "$expected" ]; then
        echo "❌ SHA256 no coincide: $file"
        echo "   Expected: $expected"
        echo "   Got:      $actual"
        return 1
    fi
}

# SEC-A1: 0 si la salida --status-fd de gpg (fichero $1) contiene un VALIDSIG
# cuya huella PRIMARIA (último campo) está en NGINX_SIGNER_FPRS.
nginx_signer_is_pinned() {
    local status_file="$1" fpr pinned
    fpr="$(awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" {print $NF; exit}' "$status_file")"
    [ -n "$fpr" ] || return 1
    for pinned in "${NGINX_SIGNER_FPRS[@]}"; do
        [ "$fpr" = "$pinned" ] && return 0
    done
    return 1
}

# SEC-A1: el directorio de trabajo debe ser de root y privado antes de compilar.
assert_private_tmpdir() {
    local dir="$1" owner mode
    owner="$(stat -c '%u' "$dir")"
    mode="$(stat -c '%a' "$dir")"
    if [ ! -d "$dir" ] || [ -L "$dir" ] || [ "$owner" != "0" ] || [ "$mode" != "700" ]; then
        echo "❌ Directorio temporal inseguro: $dir (uid=$owner modo=$mode)"
        exit 1
    fi
}

# Cargado con `source` (tests): solo definir funciones, no instalar nada.
if [[ "${BASH_SOURCE[0]}" != "${0}" ]]; then
    return 0
fi

TMPDIR="$(mktemp -d /tmp/enola-pqc-tls-XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

require_root
if [ "$DRY_RUN" != "1" ]; then
    assert_private_tmpdir "$TMPDIR"
fi
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
    write_marker
    exit 0
fi

echo "📦 Instalando prerrequisitos del sistema..."
install_prereqs "$PM"

echo "⬇️  Descargando OpenSSL ${OPENSSL_VERSION} desde fuente oficial..."
run curl -fsSL --retry 3 --retry-delay 5 "${OPENSSL_BASE_URL}/${OPENSSL_TARBALL}" -o "${TMPDIR}/${OPENSSL_TARBALL}"
if [ "$DRY_RUN" != "1" ]; then
    verify_sha256 "${TMPDIR}/${OPENSSL_TARBALL}" "${OPENSSL_SHA256}" || exit 1
fi

echo "✅ OpenSSL descargado y verificado"

echo "⬇️  Descargando Nginx ${NGINX_VERSION} desde fuente oficial..."
run curl -fsSL --retry 3 --retry-delay 5 "${NGINX_BASE_URL}/${NGINX_TARBALL}" -o "${TMPDIR}/${NGINX_TARBALL}"
run curl -fsSL --retry 3 --retry-delay 5 "${NGINX_BASE_URL}/${NGINX_ASC}" -o "${TMPDIR}/${NGINX_ASC}"

if [ "$DRY_RUN" != "1" ]; then
    verify_sha256 "${TMPDIR}/${NGINX_TARBALL}" "${NGINX_SHA256}" || exit 1
    # Defensa en profundidad: firma de un firmante de releases FIJADO.
    export GNUPGHOME="${TMPDIR}/gnupg"
    mkdir -p "$GNUPGHOME"
    chmod 700 "$GNUPGHOME"
    for i in "${!NGINX_SIGNER_KEY_URLS[@]}"; do
        curl -fsSL --retry 3 --retry-delay 5 "${NGINX_SIGNER_KEY_URLS[$i]}" -o "${TMPDIR}/nginx_signer_${i}.key"
        gpg --batch --import "${TMPDIR}/nginx_signer_${i}.key" >/dev/null 2>&1
    done
    gpg --batch --status-fd 1 --verify "${TMPDIR}/${NGINX_ASC}" "${TMPDIR}/${NGINX_TARBALL}" \
        > "${TMPDIR}/nginx_verify.status" 2>/dev/null || true
    if ! nginx_signer_is_pinned "${TMPDIR}/nginx_verify.status"; then
        echo "❌ Firma GPG de Nginx no válida o de un firmante no fijado"
        exit 1
    fi
fi

echo "✅ Nginx descargado y verificado"

echo "🧱 Compilando OpenSSL ${OPENSSL_VERSION}..."
run tar -xzf "${TMPDIR}/${OPENSSL_TARBALL}" -C "${TMPDIR}"
run_sh "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && ./Configure --prefix='${OPENSSL_PREFIX}' --openssldir='${OPENSSL_PREFIX}' shared zlib linux-x86_64"
run_sh "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && make -j\$(nproc)"
run_sh "cd '${TMPDIR}/openssl-${OPENSSL_VERSION}' && make install_sw"

OPENSSL_LIBDIR="${OPENSSL_PREFIX}/lib64"
if [ "$DRY_RUN" != "1" ] && [ ! -d "$OPENSSL_LIBDIR" ]; then
    OPENSSL_LIBDIR="${OPENSSL_PREFIX}/lib"
fi

echo "🔗 Registrando OpenSSL 3.5 en el sistema..."
run mkdir -p /etc/ld.so.conf.d
run_sh "printf '%s\n' '${OPENSSL_LIBDIR}' > /etc/ld.so.conf.d/enola-openssl-3.5.conf"
run ldconfig
run ln -sf "${OPENSSL_PREFIX}/bin/openssl" /usr/local/bin/openssl

echo "🧱 Compilando Nginx ${NGINX_VERSION} contra OpenSSL ${OPENSSL_VERSION}..."
run tar -xzf "${TMPDIR}/${NGINX_TARBALL}" -C "${TMPDIR}"
run_sh "cd '${TMPDIR}/nginx-${NGINX_VERSION}' && ./configure \
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
run_sh "cd '${TMPDIR}/nginx-${NGINX_VERSION}' && make -j\$(nproc)"
run_sh "[ -x /usr/sbin/nginx ] && [ ! -e /usr/sbin/nginx.enola-backup ] && cp /usr/sbin/nginx /usr/sbin/nginx.enola-backup || true"
run install -m 0755 "${TMPDIR}/nginx-${NGINX_VERSION}/objs/nginx" /usr/sbin/nginx

if [ "$DRY_RUN" != "1" ]; then
    nginx -V 2>&1 | grep -q "OpenSSL ${OPENSSL_VERSION}" || {
        echo "❌ Nginx no parece estar enlazado con OpenSSL ${OPENSSL_VERSION}"
        nginx -V 2>&1 || true
        exit 1
    }
fi

echo "📝 Guardando marker de PQC TLS..."
write_marker

if [ "$DRY_RUN" != "1" ]; then
    nginx -t >/dev/null 2>&1 || true
    systemctl restart nginx >/dev/null 2>&1 || true
fi

echo ""
echo "✅ PQC TLS stack instalado"
echo "   openssl: $( [ "$DRY_RUN" = "1" ] && echo "OpenSSL ${OPENSSL_VERSION}" || openssl version )"
echo "   nginx:   $( [ "$DRY_RUN" = "1" ] && echo "nginx/${NGINX_VERSION} (built with OpenSSL ${OPENSSL_VERSION})" || nginx -V 2>&1 | head -1 )"
echo "   marker:  ${MARKER_FILE}"

