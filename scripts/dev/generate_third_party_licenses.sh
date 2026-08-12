#!/usr/bin/env bash
# generate_third_party_licenses.sh — Regenera THIRD_PARTY_LICENSES.txt desde Cargo.lock
#
# Parsea Cargo.lock via cargo metadata, excluye el paquete propio (enola-cli),
# obtiene la licencia SPDX de cada dependencia, descarga los textos completos
# de las licencias desde spdx.org, y genera THIRD_PARTY_LICENSES.txt.
#
# Usage:
#   bash scripts/dev/generate_third_party_licenses.sh
#
# Prerequisites:
#   - python3
#   - curl
#   - cargo (rust toolchain)
#   - Internet connection (for downloading SPDX license texts)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
OUTPUT_FILE="$PROJECT_DIR/THIRD_PARTY_LICENSES.txt"
CACHE_DIR="/tmp/enola_license_texts"

cd "$PROJECT_DIR"

echo "=== Generating THIRD_PARTY_LICENSES.txt ==="
echo "Project: $PROJECT_DIR"
echo "Date:    $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

# --- Step 1: Extract packages and licenses via cargo metadata ---
echo "[1/4] Extracting packages from cargo metadata..."

METADATA_JSON=$(cargo metadata --format-version 1 2>/dev/null)

PACKAGES_TSV=$(echo "$METADATA_JSON" | python3 -c "
import json, sys

data = json.load(sys.stdin)
packages = data.get('packages', [])

# Exclude the project itself
filtered = []
for p in packages:
    name = p['name']
    if name == 'enola-cli':
        continue
    ver = p['version']
    lic = p.get('license') or 'UNKNOWN'
    filtered.append((name, ver, lic))

# Sort by name, then version
filtered.sort(key=lambda x: (x[0], x[1]))

for name, ver, lic in filtered:
    print(f'{name}\t{ver}\t{lic}')
")

TOTAL=$(echo "$PACKAGES_TSV" | wc -l)
echo "    Found $TOTAL third-party packages"

# Check for UNKNOWN licenses
UNKNOWN_COUNT=$(echo "$PACKAGES_TSV" | grep -c 'UNKNOWN$' || true)
if [ "$UNKNOWN_COUNT" -gt 0 ]; then
    echo "    ⚠️  $UNKNOWN_COUNT packages with UNKNOWN license:"
    echo "$PACKAGES_TSV" | grep 'UNKNOWN$' | while IFS=$'\t' read -r name ver lic; do
        echo "       $name $ver"
    done
fi

# --- Step 2: Collect unique SPDX license identifiers ---
echo "[2/4] Collecting unique SPDX license identifiers..."

UNIQUE_LICENSES=$(echo "$PACKAGES_TSV" | python3 -c "
import sys

licenses = set()
for line in sys.stdin:
    parts = line.strip().split('\t')
    if len(parts) < 3:
        continue
    lic_str = parts[2]
    # Remove parentheses before splitting
    lic_str = lic_str.replace('(', '').replace(')', '')
    # Split on OR, AND, WITH, / to get individual identifiers
    for sep in [' OR ', ' AND ', ' WITH ', '/']:
        lic_str = lic_str.replace(sep, '\n')
    for lic in lic_str.split('\n'):
        lic = lic.strip()
        if lic and lic != 'UNKNOWN':
            licenses.add(lic)

for lic in sorted(licenses):
    print(lic)
")

LICENSE_COUNT=$(echo "$UNIQUE_LICENSES" | wc -l)
echo "    Found $LICENSE_COUNT unique SPDX identifiers"

# --- Step 3: Download license texts from spdx.org ---
echo "[3/4] Downloading license texts from spdx.org..."

mkdir -p "$CACHE_DIR"

download_license_text() {
    local spdx_id="$1"
    local cache_file="$CACHE_DIR/${spdx_id//\//_}.txt"

    if [ -f "$cache_file" ]; then
        return 0
    fi

    # SPDX IDs with special characters need URL encoding
    local url_id="${spdx_id// /%20}"
    local url="https://spdx.org/licenses/${url_id}.txt"

    if curl -sf --max-time 10 "$url" -o "$cache_file" 2>/dev/null; then
        return 0
    else
        # Try alternative: some licenses have different URL patterns
        local url2="https://raw.githubusercontent.com/spdx/license-list-data/main/text/${url_id}.txt"
        if curl -sf --max-time 10 "$url2" -o "$cache_file" 2>/dev/null; then
            return 0
        else
            rm -f "$cache_file"
            return 1
        fi
    fi
}

FAILED_DOWNLOADS=""
while IFS= read -r spdx_id; do
    if download_license_text "$spdx_id"; then
        echo "    ✅ $spdx_id"
    else
        echo "    ❌ $spdx_id (download failed — will use placeholder)"
        FAILED_DOWNLOADS="$FAILED_DOWNLOADS $spdx_id"
    fi
done <<< "$UNIQUE_LICENSES"

# --- Step 4: Generate THIRD_PARTY_LICENSES.txt ---
echo "[4/4] Generating $OUTPUT_FILE..."

GENERATED_DATE=$(date -u +%Y-%m-%d)

{
    echo "================================================================================"
    echo "  ENOLA CLI — THIRD-PARTY SOFTWARE LICENSE NOTICES"
    echo "  Generated: $GENERATED_DATE"
    echo "================================================================================"
    echo ""
    echo "This file lists all third-party software dependencies used in the"
    echo "Enola CLI project, along with their license information."
    echo ""
    echo "The Enola CLI software itself is proprietary. See the LICENSE file"
    echo "for details. The third-party components listed below retain their"
    echo "original licenses."
    echo ""
    echo "--------------------------------------------------------------------------------"
    echo "  SECTION 1: COMPLETE LIST OF THIRD-PARTY DEPENDENCIES"
    echo "--------------------------------------------------------------------------------"
    echo ""
    echo "Total dependencies: $TOTAL"
    echo ""
    echo "Package                                            Version         License"
    echo "--------------------------------------------------------------------------------"

    # Print packages with aligned columns
    echo "$PACKAGES_TSV" | python3 -c "
import sys
for line in sys.stdin:
    parts = line.strip().split('\t')
    if len(parts) < 3:
        continue
    name, ver, lic = parts[0], parts[1], parts[2]
    # Pad name to 50 chars, version to 16 chars
    print(f'{name:<50} {ver:<16} {lic}')
"

    echo ""
    echo "--------------------------------------------------------------------------------"
    echo "  SECTION 2: LICENSE TEXTS"
    echo "--------------------------------------------------------------------------------"
    echo ""
    echo "The following license texts are included for the licenses used by"
    echo "the dependencies listed above, as required by their respective"
    echo "license terms."
    echo ""

    # Print license texts
    while IFS= read -r spdx_id; do
        cache_file="$CACHE_DIR/${spdx_id//\//_}.txt"
        echo "--------------------------------------------------------------------------------"
        echo "  $spdx_id"
        echo "--------------------------------------------------------------------------------"
        echo ""
        if [ -f "$cache_file" ]; then
            cat "$cache_file"
        else
            echo "# TODO: Obtain license text for $spdx_id"
            echo "# Download from: https://spdx.org/licenses/${spdx_id// /%20}.txt"
        fi
        echo ""
        echo ""
    done <<< "$UNIQUE_LICENSES"

    echo "================================================================================"
    echo "  End of Third-Party License Notices"
    echo "  Generated by: scripts/dev/generate_third_party_licenses.sh"
    echo "================================================================================"
} > "$OUTPUT_FILE"

echo ""
echo "✅ Generated $OUTPUT_FILE"
echo "   Total dependencies: $TOTAL"
echo "   Unique licenses: $LICENSE_COUNT"

if [ -n "$FAILED_DOWNLOADS" ]; then
    echo ""
    echo "⚠️  Failed to download license texts for:$FAILED_DOWNLOADS"
    echo "   Placeholders have been inserted. Please download manually."
fi
