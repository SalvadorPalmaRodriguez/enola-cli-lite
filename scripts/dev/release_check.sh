#!/usr/bin/env bash
# release_check.sh — Analiza si el código tiene cambios sin releasear y
# recomienda si es necesario un bump de versión (semver estricto).
#
# Uso:
#   bash scripts/dev/release_check.sh
#
# Qué hace:
#   1. Detecta commits desde el último tag (v{VERSION}).
#   2. Categoriza los commits por prefijo conventional-commit.
#   3. Verifica completitud del release actual (tag + gh release + feed).
#   4. Recomienda bump (major/minor/patch) o "sin release".
#
# Salida:
#   0 = análisis completado (recomendación emitida)
#   1 = error (no se pudo leer versión, etc.)

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

# ── Colores ─────────────────────────────────────────────────────────────
CYAN='\033[1;36m'; GREEN='\033[1;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'
BOLD='\033[1m'; RESET='\033[0m'

# ── Versión actual ──────────────────────────────────────────────────────
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$VERSION" ]; then
    echo -e "${RED}❌ No se pudo leer la versión de Cargo.toml${RESET}" >&2
    exit 1
fi

TAG="v${VERSION}"

echo ""
echo -e "${BOLD}📊 Análisis de release — Enola CLI${RESET}"
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════${RESET}"
echo -e "Versión actual: ${BOLD}${VERSION}${RESET} (tag ${TAG})"
echo ""

# ── 1. Detectar commits sin releasear ──────────────────────────────────
if ! git rev-parse -q --verify "$TAG" > /dev/null 2>&1; then
    echo -e "${RED}❌ El tag ${TAG} no existe en git.${RESET}"
    echo -e "   ¿Olvidaste crear el tag? Ejecuta: ${BOLD}git tag -s ${TAG}${RESET}"
    echo ""
    # Sin tag, asumir que todos los commits están sin releasear
    UNRELEASED=$(git --no-pager log --oneline)
else
    UNRELEASED=$(git --no-pager log "${TAG}..HEAD" --oneline 2>/dev/null || true)
fi

if [ -z "$UNRELEASED" ]; then
    echo -e "${GREEN}✅ No hay commits sin releasear.${RESET}"
    echo -e "   El código está sincronizado con el tag ${TAG}."
    echo ""
    echo -e "${GREEN}💡 Recomendación: ${BOLD}NO es necesario un nuevo release.${RESET}"
    exit 0
fi

COMMIT_COUNT=$(echo "$UNRELEASED" | wc -l | tr -d ' ')
echo -e "Commits sin releasear: ${BOLD}${COMMIT_COUNT}${RESET}"
echo ""

# ── 2. Categorizar commits ─────────────────────────────────────────────
MAJOR=0; MINOR=0; PATCH=0; NOBUMP=0
MAJOR_LIST=""; MINOR_LIST=""; PATCH_LIST=""; NOBUMP_LIST=""

while IFS= read -r line; do
    [ -z "$line" ] && continue
    hash=$(echo "$line" | awk '{print $1}')
    msg=$(echo "$line" | cut -d' ' -f2-)

    # Detectar BREAKING CHANGE o "!" en el subject
    if echo "$msg" | grep -qE 'BREAKING CHANGE|^[a-z]+!:' ; then
        MAJOR=$((MAJOR+1))
        MAJOR_LIST="${MAJOR_LIST}\n    ${line}"
        continue
    fi

    # Categorizar por prefijo conventional-commit
    case "$msg" in
        feat:*|feat\(*)
            MINOR=$((MINOR+1))
            MINOR_LIST="${MINOR_LIST}\n    ${line}"
            ;;
        fix:*|fix\(*)
            PATCH=$((PATCH+1))
            PATCH_LIST="${PATCH_LIST}\n    ${line}"
            ;;
        refactor:*|refactor\(*|perf:*|perf\(*|revert:*|revert\(*)
            PATCH=$((PATCH+1))
            PATCH_LIST="${PATCH_LIST}\n    ${line}"
            ;;
        docs:*|docs\(*|chore:*|chore\(*|style:*|style\(*|test:*|test\(*|build:*|build\(*|ci:*|ci\(*)
            NOBUMP=$((NOBUMP+1))
            NOBUMP_LIST="${NOBUMP_LIST}\n    ${line}"
            ;;
        *)
            # Sin prefijo reconocido: asumir cambio de código → patch
            PATCH=$((PATCH+1))
            PATCH_LIST="${PATCH_LIST}\n    ${line}"
            ;;
    esac
done <<< "$UNRELEASED"

echo -e "${BOLD}Desglose por tipo:${RESET}"
[ "$MAJOR" -gt 0 ] && echo -e "  ${RED}● ${MAJOR} breaking change${RESET}"
[ "$MINOR" -gt 0 ] && echo -e "  ${CYAN}● ${MINOR} feat (nueva funcionalidad)${RESET}"
[ "$PATCH" -gt 0 ] && echo -e "  ${YELLOW}● ${PATCH} fix/refactor/perf (corrección)${RESET}"
[ "$NOBUMP" -gt 0 ] && echo -e "  ${GREEN}● ${NOBUMP} docs/chore/style/test (sin bump)${RESET}"
echo ""

# ── 3. Verificar completitud del release actual ─────────────────────────
echo -e "${BOLD}Completitud del release ${TAG}:${RESET}"

# Tag
if git rev-parse -q --verify "$TAG" > /dev/null 2>&1; then
    echo -e "  ${GREEN}✅ tag ${TAG} existe${RESET}"
else
    echo -e "  ${RED}❌ tag ${TAG} NO existe${RESET}"
fi

# GitHub release (si gh está disponible y autenticado)
if command -v gh > /dev/null 2>&1; then
    if gh release view "$TAG" > /dev/null 2>&1; then
        echo -e "  ${GREEN}✅ GitHub release ${TAG} existe${RESET}"
    else
        echo -e "  ${RED}❌ GitHub release ${TAG} NO existe${RESET}"
    fi
else
    echo -e "  ${YELLOW}⚠️  gh CLI no disponible — no se pudo verificar GitHub release${RESET}"
fi

# Feed latest
FEED_LATEST=$(grep -oE '"latest": "[^"]+"' feed/advisories.json 2>/dev/null | sed 's/.*"\(.*\)"/\1/' || echo "")
if [ "$FEED_LATEST" = "$VERSION" ]; then
    echo -e "  ${GREEN}✅ feed/advisories.json latest == ${VERSION}${RESET}"
else
    echo -e "  ${RED}❌ feed/advisories.json latest == '${FEED_LATEST}' (debería ser ${VERSION})${RESET}"
fi
echo ""

# ── 4. Recomendación ────────────────────────────────────────────────────
echo -e "${BOLD}💡 Recomendación:${RESET}"

# Calcular próxima versión
NEXT_VERSION=""
if [ "$MAJOR" -gt 0 ]; then
    BUMP="MAJOR"
    NEXT_VERSION=$(python3 -c "
v='$VERSION'.split('-')[0]
maj,min,pat = map(int, v.split('.'))
print(f'{maj+1}.0.0-alpha')
")
elif [ "$MINOR" -gt 0 ]; then
    BUMP="MINOR"
    NEXT_VERSION=$(python3 -c "
v='$VERSION'.split('-')[0]
maj,min,pat = map(int, v.split('.'))
print(f'{maj}.{min+1}.0-alpha')
")
elif [ "$PATCH" -gt 0 ]; then
    BUMP="PATCH"
    NEXT_VERSION=$(python3 -c "
v='$VERSION'.split('-')[0]
maj,min,pat = map(int, v.split('.'))
print(f'{maj}.{min}.{pat+1}-alpha')
")
else
    BUMP="NONE"
fi

if [ "$BUMP" = "NONE" ]; then
    echo -e "${GREEN}  No es necesario un nuevo release.${RESET}"
    echo -e "  Los ${COMMIT_COUNT} commits son docs/chore/style/test (sin impacto funcional)."
    echo ""
    echo -e "${YELLOW}═══════════════════════════════════════════════════════════════${RESET}"
    exit 0
fi

echo -e "${YELLOW}  Bump ${BOLD}${BUMP}${RESET}${YELLOW} → ${BOLD}${NEXT_VERSION}${RESET}"
echo ""
echo -e "  Motivo:"
[ "$MAJOR" -gt 0 ] && echo -e "    - ${MAJOR} breaking change(s) detectado(s)"
[ "$MINOR" -gt 0 ] && echo -e "    - ${MINOR} feat(s) sin publicar"
[ "$PATCH" -gt 0 ] && echo -e "    - ${PATCH} fix/refactor/perf sin publicar"
echo ""
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════${RESET}"
echo ""

# ── 5. Confirmación interactiva ────────────────────────────────────────
echo -e "${BOLD}¿Qué quieres hacer?${RESET}"
echo -e "  ${GREEN}[y]${RESET} Aceptar → ejecutar release.sh --bump ${NEXT_VERSION}"
echo -e "  ${CYAN}[d]${RESET} Dry-run → preparar y firmar SIN publicar"
echo -e "  ${RED}[n]${RESET} Ignorar → salir sin hacer nada"
echo ""
read -r -p "  Opción [y/d/n]: " CHOICE || CHOICE="n"

case "$CHOICE" in
    y|Y)
        echo ""
        echo -e "${GREEN}▶ Ejecutando release.sh --bump ${NEXT_VERSION}...${RESET}"
        bash scripts/dev/release.sh --bump "$NEXT_VERSION"
        ;;
    d|D)
        echo ""
        echo -e "${CYAN}▶ Ejecutando release.sh --bump ${NEXT_VERSION} --dry-run...${RESET}"
        bash scripts/dev/release.sh --bump "$NEXT_VERSION" --dry-run
        ;;
    n|N|"")
        echo ""
        echo -e "${YELLOW}▶ Recomendación ignorada. No se ejecutó nada.${RESET}"
        ;;
    *)
        echo ""
        echo -e "${RED}❌ Opción inválida: '$CHOICE'. No se ejecutó nada.${RESET}" >&2
        exit 1
        ;;
esac
