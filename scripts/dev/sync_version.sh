#!/usr/bin/env bash
# sync_version.sh — Propaga la versión de Cargo.toml a todos los consumidores.
# Fuente canónica: Cargo.toml → version
#
# Uso:
#   bash scripts/dev/sync_version.sh                 # propaga la versión actual
#   bash scripts/dev/sync_version.sh --check         # verifica (exit 1 si hay deriva)
#   bash scripts/dev/sync_version.sh --bump X.Y.Z    # cambia versión y propaga
#   bash scripts/dev/sync_version.sh --release-feed  # actualiza feed (latest +
#                                                    #   published_at). SOLO desde
#                                                    #   release.sh: rompe la firma
#                                                    #   minisign y exige re-firmar.
#
# Nota: el regex asume el esquema de versión actual "0.1.x-alpha".
# Si el esquema cambia (major/minor o prerelease distinto), actualizar
# las dos expresiones VERSION_RE y BADGE_RE de este script.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

MODE="sync"
NEW_VERSION=""

for arg in "$@"; do
    case "$arg" in
        --check) MODE="check" ;;
        --bump) MODE="bump" ;;
        --release-feed) MODE="release-feed" ;;
        *) NEW_VERSION="$arg" ;;
    esac
done

# ── Bump: actualizar Cargo.toml ────────────────────────────────────────
if [ "$MODE" = "bump" ]; then
    if [ -z "$NEW_VERSION" ]; then
        echo "❌ --bump requiere una versión: bash scripts/dev/sync_version.sh --bump X.Y.Z" >&2
        exit 1
    fi
    if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?$'; then
        echo "❌ Formato de versión inválido: $NEW_VERSION (esperado: X.Y.Z[-prerelease])" >&2
        exit 1
    fi
    sed -i "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
    echo "✅ Cargo.toml → $NEW_VERSION"
    # Refrescar Cargo.lock sin compilar (rápido, resuelve el workspace)
    cargo metadata --format-version 1 > /dev/null 2>&1 || \
        echo "⚠️  No se pudo refrescar Cargo.lock (se hará en el próximo build)"
fi

# ── Leer versión canónica ──────────────────────────────────────────────
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$VERSION" ]; then
    echo "❌ No se pudo leer la versión de Cargo.toml" >&2
    exit 1
fi
VERSION_DASHED="${VERSION//-/--}"   # para badges shields.io (0.1.2--alpha)

# ── Expresiones de reemplazo (esquema 0.1.x-alpha) ─────────────────────
VERSION_RE='0\.1\.[0-9]+-alpha'
BADGE_RE='0\.1\.[0-9]+--alpha'

# ── Archivos con referencias de versión (allowlist) ────────────────────
# feed/advisories.json NO se toca aquí: está firmado con minisign y solo
# debe actualizarse (y re-firmarse) durante el release → ver release.sh.
FILES=(
    "llms-full.txt"
    "docs/index.md"
    "docs/user/index.md"
    "docs/en/index.md"
    "docs/assets/social-preview.svg"
    "README.md"
    "README.es.md"
    "SECURITY.md"
    "src/cli/defs.rs"
    "docs/en/security-model.md"
    "docs/en/verify-downloads.md"
    "docs/user/verify/verify-downloads.md"
    "docs/user/general/commands-simple.md"
    "docs/user/guia/install-from-iso.md"
    "docs/user/web/README.md"
    ".github/ISSUE_TEMPLATE/bug_report.yml"
)

# ── Aplicar reemplazos a un archivo (in-place) ─────────────────────────
apply_replacements() {
    local file="$1"
    sed -i -E "s/${VERSION_RE}/${VERSION}/g" "$file"
    sed -i -E "s/${BADGE_RE}/${VERSION_DASHED}/g" "$file"
}

# ── Verificar un archivo (dry-run sobre copia temporal) ────────────────
check_file() {
    local file="$1"
    local tmp
    tmp=$(mktemp)
    cp "$file" "$tmp"
    sed -i -E "s/${VERSION_RE}/${VERSION}/g" "$tmp"
    sed -i -E "s/${BADGE_RE}/${VERSION_DASHED}/g" "$tmp"
    if ! diff -q "$file" "$tmp" > /dev/null 2>&1; then
        echo "  ❌ $file"
        rm -f "$tmp"
        return 1
    fi
    rm -f "$tmp"
    return 0
}

# ── advisories.json: SOLO en modo --release-feed ───────────────────────
# Actualiza "latest" y "published_at" y copia a docs/feed/. Deja la firma
# minisign INVÁLIDA a propósito: release.sh la regenera inmediatamente.
release_feed() {
    sed -i -E "s/\"latest\": \"[^\"]+\"/\"latest\": \"${VERSION}\"/" feed/advisories.json
    sed -i -E "s/\"published_at\": \"[^\"]+\"/\"published_at\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"/" feed/advisories.json
    cp feed/advisories.json docs/feed/advisories.json
    echo "  ✅ feed/advisories.json → latest=${VERSION} (⚠️  firma pendiente de regenerar)"
}

# El check rutinario NO verifica "latest" (puede quedarse en la versión del
# último release mientras HEAD avanza). Solo verifica que docs/feed/ sea
# copia idéntica de feed/ (ambos se publican).
check_advisories() {
    if ! diff -q feed/advisories.json docs/feed/advisories.json > /dev/null 2>&1; then
        echo "  ❌ docs/feed/advisories.json (no es copia idéntica de feed/)"
        return 1
    fi
    if ! diff -q feed/advisories.json.minisig docs/feed/advisories.json.minisig > /dev/null 2>&1; then
        echo "  ❌ docs/feed/advisories.json.minisig (no es copia idéntica de feed/)"
        return 1
    fi
    return 0
}

# ── Ejecutar según modo ────────────────────────────────────────────────
if [ "$MODE" = "release-feed" ]; then
    release_feed
    exit 0
fi

if [ "$MODE" = "check" ]; then
    DRIFT=0
    for f in "${FILES[@]}"; do
        if [ -f "$f" ]; then
            check_file "$f" || DRIFT=1
        fi
    done
    check_advisories || DRIFT=1
    if [ "$DRIFT" -ne 0 ]; then
        echo ""
        echo "❌ Deriva de versión detectada." >&2
        echo "   Ejecuta: bash scripts/dev/sync_version.sh" >&2
        exit 1
    fi
    echo "✅ Versión $VERSION sincronizada en todos los archivos."
    exit 0
fi

# Modo sync (o tras bump)
for f in "${FILES[@]}"; do
    if [ -f "$f" ]; then
        apply_replacements "$f"
        echo "  ✅ $f"
    fi
done
echo ""
echo "✅ Versión $VERSION propagada (feed/advisories.json intacto — se actualiza en release.sh)."
