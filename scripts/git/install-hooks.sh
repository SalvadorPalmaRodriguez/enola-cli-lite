#!/usr/bin/env bash
# install-hooks.sh — Instala los hooks git de Enola CLI.
#
# Crea symlinks relativos en .git/hooks/ hacia scripts/git/.
# Idempotente: reemplaza symlinks rotos u obsoletos.
#
# Uso:
#   bash scripts/git/install-hooks.sh

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOOKS_DIR="$PROJECT_ROOT/.git/hooks"

if [ ! -d "$HOOKS_DIR" ]; then
    echo "❌ No se encontró $HOOKS_DIR (¿es un repo git?)" >&2
    exit 1
fi

# Asegurar permisos de ejecución de los scripts fuente
chmod +x "$PROJECT_ROOT/scripts/git/pre-commit"
chmod +x "$PROJECT_ROOT/scripts/git/pre-push"

# Symlinks relativos (portables si el repo se mueve)
ln -sf "../../scripts/git/pre-commit" "$HOOKS_DIR/pre-commit"
ln -sf "../../scripts/git/pre-push" "$HOOKS_DIR/pre-push"

echo "✅ Hooks instalados:"
ls -la "$HOOKS_DIR/pre-commit" "$HOOKS_DIR/pre-push"
