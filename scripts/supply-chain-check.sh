#!/usr/bin/env bash
# supply-chain-check.sh — MED-03: Manual supply chain security checks
#
# Runs cargo-audit (RustSec advisory database) and cargo-deny (licenses, bans, sources).
# This is the interim solution while Forgejo Actions are disabled (no Forgejo server running).
#
# Usage:
#   bash scripts/supply-chain-check.sh
#
# Prerequisites:
#   cargo install cargo-audit --locked
#   cargo install cargo-deny --locked

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "=== Supply Chain Security Check ==="
echo "Project: $(basename "$PROJECT_DIR")"
echo "Date:    $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

# Check if tools are installed
MISSING=0
if ! command -v cargo-audit &>/dev/null; then
    echo "❌ cargo-audit not installed. Run: cargo install cargo-audit --locked"
    MISSING=1
fi
if ! command -v cargo-deny &>/dev/null; then
    echo "❌ cargo-deny not installed. Run: cargo install cargo-deny --locked"
    MISSING=1
fi

if [ "$MISSING" -eq 1 ]; then
    echo ""
    echo "Install missing tools and re-run."
    exit 1
fi

echo "--- cargo audit (RustSec advisories) ---"
cargo audit 2>&1
AUDIT_EXIT=$?

echo ""
echo "--- cargo deny check (licenses, bans, sources, advisories) ---"
cargo deny check 2>&1
DENY_EXIT=$?

echo ""
echo "=== Summary ==="
if [ "$AUDIT_EXIT" -eq 0 ]; then
    echo "✅ cargo audit: passed"
else
    echo "❌ cargo audit: failed (exit $AUDIT_EXIT)"
fi

if [ "$DENY_EXIT" -eq 0 ]; then
    echo "✅ cargo deny:  passed"
else
    echo "❌ cargo deny:  failed (exit $DENY_EXIT)"
fi

# Exit non-zero if any check failed
if [ "$AUDIT_EXIT" -ne 0 ] || [ "$DENY_EXIT" -ne 0 ]; then
    exit 1
fi

echo ""
echo "✅ All supply chain checks passed."
