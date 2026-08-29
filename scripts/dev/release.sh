#!/usr/bin/env bash
# release.sh — Orquestación completa del release de Enola CLI.
#
# Hace TODO el ciclo: versión → build reproducible → firma dual
# (minisign + ML-DSA-65) → feed re-firmado → tag → GitHub release.
#
# Uso:
#   bash scripts/dev/release.sh                  # release de la versión actual de Cargo.toml
#   bash scripts/dev/release.sh --bump X.Y.Z     # bump + sync + release
#   bash scripts/dev/release.sh --dry-run        # prepara y firma artefactos, NO publica
#
# Requisitos:
#   - gh CLI autenticado (gh auth status)
#   - minisign + clave secreta en ~/.minisign/enola.key (pedirá passphrase)
#   - Clave PQC en ~/.enola/pqc_signing.key (enola-sign-pqc)
#   - cargo-sbom instalado (cargo install cargo-sbom)
#   - Working tree limpio, rama main
#
# Artefactos publicados (mismo formato que releases anteriores):
#   enola-cli-vX.Y.Z-<arch>-client.tar.gz            (+ .sha256 .minisig .pqsig)
#   enola-cli-vX.Y.Z-<arch>                          (binario crudo, + .sha256 .minisig — para install.sh)
#   enola-cli-vX.Y.Z-<arch>.sbom.spdx.json
#   LATEST                                           (contiene el tag — install.sh lo resuelve)
#
# El feed (feed/advisories.json + docs/feed/) se actualiza y RE-FIRMA aquí,
# nunca en sync_version.sh (ver §13.42).

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

MINISIGN_KEY="${ENOLA_MINISIGN_KEY:-$HOME/.minisign/enola.key}"
PQC_KEY="$HOME/.enola/pqc_signing.key"
TARGET_DIR="${ENOLA_RELEASE_TARGET_DIR:-/tmp/enola-release-build}"
STAGE="$(mktemp -d -t enola-release.XXXXXX)"
trap 'rm -rf "$STAGE"' EXIT

DRY_RUN=0
BUMP=""
SKIP=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        --bump) BUMP="pending" ;;
        *) [ "$BUMP" = "pending" ] && BUMP="$arg" || { echo "❌ Argumento desconocido: $arg" >&2; exit 1; } ;;
    esac
done

log()  { printf "\n\033[1;36m═══ %s ═══\033[0m\n" "$*"; }
ok()   { printf "  \033[1;32m✅\033[0m %s\n" "$*"; }
die()  { printf "  \033[1;31m❌\033[0m %s\n" "$*" >&2; exit 1; }

# ── 0. Precondiciones ──────────────────────────────────────────────────
log "Precondiciones"
command -v gh        >/dev/null || die "gh CLI no instalado"
command -v minisign  >/dev/null || die "minisign no instalado"
command -v cargo-sbom >/dev/null || die "cargo-sbom no instalado (cargo install cargo-sbom)"
[ -f "$MINISIGN_KEY" ] || die "Clave minisign no encontrada: $MINISIGN_KEY"
[ -f "$PQC_KEY" ]      || die "Clave PQC no encontrada: $PQC_KEY (enola-sign-pqc keygen)"
gh auth status >/dev/null 2>&1 || die "gh CLI no autenticado (gh auth login)"

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" = "main" ] || die "Debes estar en main (actual: $BRANCH)"
[ -z "$(git status --porcelain)" ] || die "Working tree sucio — commitea o stashea antes del release"
ok "gh, minisign, claves y working tree OK"

# ── 1. Versión ─────────────────────────────────────────────────────────
log "Versión"
if [ -n "$BUMP" ] && [ "$BUMP" != "pending" ]; then
    bash scripts/dev/sync_version.sh --bump "$BUMP"
else
    bash scripts/dev/sync_version.sh
fi
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
TAG="v${VERSION}"
git rev-parse "$TAG" >/dev/null 2>&1 && die "El tag $TAG ya existe"
ok "Versión: $VERSION (tag $TAG)"

case "$(uname -m)" in
    x86_64)  ARCH_TAG="x86_64-linux" ;;
    aarch64) ARCH_TAG="aarch64-linux" ;;
    *) die "Arquitectura no soportada: $(uname -m)" ;;
esac

# ── 2. Build reproducible ──────────────────────────────────────────────
# CARGO_TARGET_DIR en path neutro + remap de paths personales (§13.23).
log "Build reproducible ($ARCH_TAG)"
RUSTFLAGS="--remap-path-prefix=${PROJECT_ROOT}=/build \
--remap-path-prefix=${HOME}/.cargo/registry=/cargo-registry \
--remap-path-prefix=${HOME}/.cargo/git=/cargo-git \
--remap-path-prefix=${HOME}/.rustup=/rustc" \
CARGO_TARGET_DIR="$TARGET_DIR" \
    cargo build --release > /tmp/enola_release_build.log 2>&1 \
    || die "Build falló — ver /tmp/enola_release_build.log"
BIN="$TARGET_DIR/release/enola-cli"
SIGN_PQC="$TARGET_DIR/release/enola-sign-pqc"
[ -x "$BIN" ] || die "Binario no encontrado: $BIN"
ok "Build OK: $BIN"

if strings "$BIN" | grep -E "/home/|${HOME}" | head -1 | grep -q .; then
    die "El binario filtra paths personales (/home/…) — revisa RUSTFLAGS/remap"
fi
ok "Sin filtración de paths personales"

# ── 3. Artefactos ──────────────────────────────────────────────────────
log "Artefactos"
CLIENT_DIR="enola-cli-${TAG}-${ARCH_TAG}-client"
TARBALL="${CLIENT_DIR}.tar.gz"
RAW_BIN="enola-cli-${TAG}-${ARCH_TAG}"
SBOM="enola-cli-${TAG}-${ARCH_TAG}.sbom.spdx.json"

mkdir -p "$STAGE/$CLIENT_DIR"
install -m 0755 "$BIN"                      "$STAGE/$CLIENT_DIR/enola-cli"
install -m 0755 scripts/deploy/install.sh   "$STAGE/$CLIENT_DIR/install.sh"
install -m 0644 minisign.pub                "$STAGE/$CLIENT_DIR/minisign.pub"
install -m 0644 pqc_sign.pub                "$STAGE/$CLIENT_DIR/pqc_sign.pub"

# Tarball determinista (orden, owner y mtime fijos)
SOURCE_EPOCH="$(git log -1 --format=%ct)"
tar --sort=name --owner=0 --group=0 --numeric-owner \
    --mtime="@${SOURCE_EPOCH}" \
    -C "$STAGE" -czf "$STAGE/$TARBALL" "$CLIENT_DIR"
ok "$TARBALL"

install -m 0755 "$BIN" "$STAGE/$RAW_BIN"
ok "$RAW_BIN (binario crudo para install.sh)"

printf '%s\n' "$TAG" > "$STAGE/LATEST"
ok "LATEST → $TAG"

cargo sbom > "$STAGE/$SBOM" 2>/dev/null || die "cargo sbom falló"
ok "$SBOM"

( cd "$STAGE" && sha256sum "$TARBALL" > "$TARBALL.sha256" && sha256sum "$RAW_BIN" > "$RAW_BIN.sha256" )
ok "SHA256 generados"

# ── 4. Feed (latest + published_at) ────────────────────────────────────
log "Feed de advisories"
bash scripts/dev/sync_version.sh --release-feed

# ── 5. Firmas ──────────────────────────────────────────────────────────
log "Firma minisign (pedirá passphrase)"
minisign -S -s "$MINISIGN_KEY" \
    -t "enola-cli $TAG $ARCH_TAG" \
    -m "$STAGE/$TARBALL" "$STAGE/$RAW_BIN"
minisign -S -s "$MINISIGN_KEY" -m feed/advisories.json
cp feed/advisories.json.minisig docs/feed/advisories.json.minisig
ok "Firmas minisign generadas (tarball, binario, feed)"

log "Firma PQC (ML-DSA-65)"
"$SIGN_PQC" sign "$STAGE/$TARBALL"
ok "Firma PQC generada"

# ── 6. Verificación de firmas ──────────────────────────────────────────
log "Verificación"
PUB="$(sed -n 2p minisign.pub)"
minisign -V -m "$STAGE/$TARBALL"      -P "$PUB" >/dev/null || die "Firma minisign del tarball inválida"
minisign -V -m "$STAGE/$RAW_BIN"      -P "$PUB" >/dev/null || die "Firma minisign del binario inválida"
minisign -V -m feed/advisories.json   -P "$PUB" >/dev/null || die "Firma minisign del feed inválida"
minisign -V -m docs/feed/advisories.json -p docs/feed/minisign.pub \
    -x docs/feed/advisories.json.minisig >/dev/null || die "Firma del feed en docs/feed/ inválida"
"$SIGN_PQC" verify "$STAGE/$TARBALL" pqc_sign.pub >/dev/null || die "Firma PQC inválida"
( cd "$STAGE" && sha256sum -c "$TARBALL.sha256" "$RAW_BIN.sha256" >/dev/null ) || die "SHA256 mismatch"
ok "Todas las firmas verificadas (minisign + PQC + SHA256)"

if [ "$DRY_RUN" = "1" ]; then
    log "DRY-RUN — no se publica"
    echo "  Artefactos en: $STAGE (se borran al salir)"
    ls -la "$STAGE"
    echo "  ⚠️  feed/ y docs/feed/ quedaron modificados en el working tree (revierte con git checkout si no quieres publicarlos)."
    exit 0
fi

# ── 7. Commit + tag firmados ───────────────────────────────────────────
log "Commit y tag firmados (PGP)"
git add feed/advisories.json feed/advisories.json.minisig \
        docs/feed/advisories.json docs/feed/advisories.json.minisig
# Cambios de sync_version (si hubo bump)
git add -u
git commit -S -m "chore(release): $TAG"
git tag -s "$TAG" -m "Release $TAG"
git --no-pager log --show-signature -1 | head -5
ok "Commit y tag $TAG firmados"

# ── 8. Push + GitHub release ───────────────────────────────────────────
log "Publicación en GitHub"
git push origin main "$TAG"

NOTES="$(awk -v v="$VERSION" 'BEGIN{f=0} /^## /{if (f) exit; if (index($0, v)) f=1; next} f' CHANGELOG.md 2>/dev/null || true)"
[ -z "$NOTES" ] && NOTES="Release $TAG — ver CHANGELOG.md"

gh release create "$TAG" \
    --title "enola-cli $TAG" \
    --notes "$NOTES" \
    "$STAGE/$TARBALL" \
    "$STAGE/$TARBALL.sha256" \
    "$STAGE/$TARBALL.minisig" \
    "$STAGE/$TARBALL.pqsig" \
    "$STAGE/$RAW_BIN" \
    "$STAGE/$RAW_BIN.sha256" \
    "$STAGE/$RAW_BIN.minisig" \
    "$STAGE/$SBOM" \
    "$STAGE/LATEST"

log "Release $TAG publicado"
gh release view "$TAG" --json url --jq .url
