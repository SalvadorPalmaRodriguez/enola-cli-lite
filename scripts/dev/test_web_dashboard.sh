#!/bin/bash
# =============================================================================
# Enola Web Dashboard — Exhaustive API Test Script (100% coverage)
#
# Tests every button, input, flag, and option in the 19 tabs of the web UI.
# Maps 1:1 to the functions in assets/app.js and buttons in assets/index.html.
#
# Usage:
#   bash scripts/dev/test_web_dashboard.sh <TOKEN> [BASE_URL]
#
# Default BASE_URL = http://127.0.0.1:8090
# =============================================================================
set -euo pipefail

TOKEN="${1:?Usage: $0 <TOKEN> [BASE_URL]}"
BASE="${2:-http://127.0.0.1:8090/api}"
OUT="/tmp/enola_web_test_exhaustive.log"
PASS=0
FAIL=0
SKIP=0
ERRORS_FILE="/tmp/enola_web_test_errors.log"

> "$OUT"
> "$ERRORS_FILE"

# ── Helper: run a test and log result ────────────────────────────────────────
# Usage: t "TEST_NAME" -X POST -H "..." -d '...' URL
t() {
    local name="$1"
    shift
    local resp
    local code
    local max_time=30
    for arg in "$@"; do
        case "$arg" in */console/run*) max_time=120 ;; esac
    done
    resp=$(curl -s -w '\n__HTTP_CODE__%{http_code}' --max-time "$max_time" "$@" 2>&1) || true
    code=$(echo "$resp" | grep '__HTTP_CODE__' | sed 's/.*__HTTP_CODE__//')
    local body=$(echo "$resp" | sed '/__HTTP_CODE__/d')

    echo "--- $name ---" >> "$OUT"
    echo "$body" >> "$OUT"
    echo "" >> "$OUT"

    # Check for error in response body
    if echo "$body" | grep -q '"error"'; then
        local errmsg
        errmsg=$(echo "$body" | python3 -c "import sys,json; print(json.load(sys.stdin).get('error','?')[:100])" 2>/dev/null || echo "$body" | head -c 100)
        echo "FAIL  $name  → $errmsg" >> "$ERRORS_FILE"
        FAIL=$((FAIL + 1))
    elif [ "$code" = "200" ] || [ "$code" = "201" ] || [ "$code" = "202" ]; then
        # For console/run responses, verify CLI exit_code == 0
        if echo "$body" | grep -q '"exit_code"'; then
            local ec
            ec=$(echo "$body" | python3 -c "import sys,json; print(json.load(sys.stdin).get('exit_code',-1))" 2>/dev/null || echo "-1")
            if [ "$ec" != "0" ]; then
                echo "FAIL  $name  → CLI exit_code=$ec" >> "$ERRORS_FILE"
                FAIL=$((FAIL + 1))
                return
            fi
        fi
        PASS=$((PASS + 1))
    else
        echo "FAIL  $name  → HTTP $code" >> "$ERRORS_FILE"
        FAIL=$((FAIL + 1))
    fi
}

# ── Helper: run a test expecting an error (e.g. lite limitations) ────────────
t_expect_error() {
    local name="$1"
    shift
    local resp
    local max_time=30
    for arg in "$@"; do
        case "$arg" in */console/run*) max_time=120 ;; esac
    done
    resp=$(curl -s -w '\n__HTTP_CODE__%{http_code}' --max-time "$max_time" "$@" 2>&1) || true
    local body=$(echo "$resp" | sed '/__HTTP_CODE__/d')

    echo "--- $name (expect error) ---" >> "$OUT"
    echo "$body" >> "$OUT"
    echo "" >> "$OUT"

    if echo "$body" | grep -q '"error"'; then
        PASS=$((PASS + 1))
    else
        echo "UNEXPECTED_PASS  $name  → expected error but got success" >> "$ERRORS_FILE"
        FAIL=$((FAIL + 1))
    fi
}

# ── Helper: skip a test (disabled button in UI) ─────────────────────────────
t_skip() {
    local name="$1"
    echo "--- $name (SKIPPED: disabled in UI) ---" >> "$OUT"
    echo "" >> "$OUT"
    SKIP=$((SKIP + 1))
}

# Unique port allocator (avoid collisions between tests)
# Uses a temp file because $(...) runs in a subshell — variable updates don't persist
PORT_FILE=$(mktemp /tmp/enola_port_XXXX)
echo 22000 > "$PORT_FILE"
alloc_port() {
    local p
    p=$(cat "$PORT_FILE")
    echo $((p + 1)) > "$PORT_FILE"
    echo "$p"
}
alloc_tab_port() {
    alloc_port
}

# Kill stuck enola-cli subprocesses to keep the server responsive
cleanup_subprocs() {
    pkill -f 'enola-cli (update|docs|config|doctor|diag|test|maintenance|setup|quickref|license|uninstall|verify)' 2>/dev/null || true
    sleep 1
}

echo "======================================================================" >> "$OUT"
echo "Enola Web Dashboard — Exhaustive Test (100% coverage)" >> "$OUT"
echo "Date: $(date -u '+%Y-%m-%d %H:%M:%S UTC')" >> "$OUT"
echo "Base URL: $BASE" >> "$OUT"
echo "======================================================================" >> "$OUT"
echo "" >> "$OUT"

# =============================================================================
# TAB 1: SERVICES (auto-load on page open)
# Functions: loadServices() → GET /api/services
# =============================================================================
echo "===== TAB 1: SERVICES =====" >> "$OUT"
t "SERVICES: GET /api/services" -H "Authorization: $TOKEN" "$BASE/services"

# =============================================================================
# TAB 2: TOR
# Buttons: Create (4 types × ssl), Start, Stop, Edit, Rotate, Remove
# Auth: List, Enable, Disable, Generate, Add, Revoke, Rotate
# =============================================================================
echo "===== TAB 2: TOR =====" >> "$OUT"
cleanup_subprocs

# --- loadTor() → GET /api/tor ---
t "TOR: LIST" -H "Authorization: $TOKEN" "$BASE/tor"

# --- torCreate() → POST /api/tor/create ---
# Test all 4 service types, with and without SSL, with and without target_port
P=$(alloc_port)
t "TOR: CREATE web (default vport=80, no ssl)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-web1","service_type":"web","virtual_port":80,"target_port":null,"ssl":false}' "$BASE/tor/create"

P=$(alloc_port)
t "TOR: CREATE web+ssl (vport=443)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-webssl1","service_type":"web","virtual_port":443,"target_port":null,"ssl":true}' "$BASE/tor/create"

P=$(alloc_port)
t "TOR: CREATE raw (vport=80, target_port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-raw1\",\"service_type\":\"raw\",\"virtual_port\":80,\"target_port\":$P,\"ssl\":false}" "$BASE/tor/create"

t "TOR: CREATE static (default)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-static1","service_type":"static","virtual_port":80,"target_port":null,"ssl":false}' "$BASE/tor/create"

t "TOR: CREATE files (default)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-files1","service_type":"files","virtual_port":80,"target_port":null,"ssl":false}' "$BASE/tor/create"

# --- loadTor() → GET /api/tor (verify created) ---
t "TOR: LIST (after creates)" -H "Authorization: $TOKEN" "$BASE/tor"

# --- torAction(name,'start') → POST /api/tor/{name}/start ---
t "TOR: START tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/tx-web1/start"

# --- torAction(name,'stop') → POST /api/tor/{name}/stop ---
t "TOR: STOP tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/tx-web1/stop"

# --- torEdit(name) → GET /api/tor/{name}/detail ---
t "TOR: DETAIL tx-web1" -H "Authorization: $TOKEN" "$BASE/tor/tx-web1/detail"

# --- torEditSave() → POST /api/tor/{name}/edit ---
P=$(alloc_port)
t "TOR: EDIT tx-web1 (auto_ports=true, target_port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"virtual_port\":80,\"nginx_port\":null,\"target_port\":$P,\"auto_ports\":true}" "$BASE/tor/tx-web1/edit"

# --- torRotate(name) → POST /api/tor/{name}/rotate ---
t "TOR: ROTATE tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/tx-web1/rotate"

# --- torAuthList() → GET /api/tor/auth/{service}/list ---
t "TOR AUTH: LIST clients tx-web1" -H "Authorization: $TOKEN" "$BASE/tor/auth/tx-web1/list"

# --- torAuthEnable() → POST /api/tor/auth/{service}/enable ---
t "TOR AUTH: ENABLE tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/auth/tx-web1/enable"

# --- torAuthGenerate() → POST /api/tor/auth/generate ---
t "TOR AUTH: GENERATE client tx-client1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"client":"tx-client1"}' "$BASE/tor/auth/generate"

# --- torAuthAdd() → POST /api/tor/auth/{service}/add ---
# Use a valid 52-char base32 x25519 public key
t "TOR AUTH: ADD client tx-client1 to tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"client":"tx-client1","pubkey":"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST"}' "$BASE/tor/auth/tx-web1/add"

# --- torAuthRevoke() → POST /api/tor/auth/{service}/revoke ---
t "TOR AUTH: REVOKE tx-client1 from tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"client":"tx-client1"}' "$BASE/tor/auth/tx-web1/revoke"

# --- torAuthRotate() → POST /api/tor/auth/{service}/rotate ---
# NOTE: Rotate generates new keys, revokes old client, adds new one.
# Since we revoked tx-client1 above, rotate with a fresh client name.
t "TOR AUTH: ROTATE keys tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"client":"tx-rotated1"}' "$BASE/tor/auth/tx-web1/rotate"

# --- torAuthDisable() → POST /api/tor/auth/{service}/disable ---
t "TOR AUTH: DISABLE tx-web1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/auth/tx-web1/disable"

# --- torAction(name,'remove') → POST /api/tor/{name}/remove ---
for svc in tx-raw1 tx-static1 tx-files1 tx-webssl1 tx-web1; do
    t "TOR: REMOVE $svc" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/tor/$svc/remove"
done

# =============================================================================
# TAB 3: GIT
# Buttons: Create (with/without ssl, ports, admin user), Start, Stop, Publish,
#          Hide, Edit, Delete, Watcher, User List/Create/Delete
# Additional functions: gitStatus, gitRegistration
# =============================================================================
echo "===== TAB 3: GIT =====" >> "$OUT"
cleanup_subprocs

# --- loadGit() → GET /api/git ---
t "GIT: LIST" -H "Authorization: $TOKEN" "$BASE/git"

# --- gitCreate() → POST /api/git/create ---
t "GIT: CREATE default (no ssl, no ports)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-git1","ssl":false,"admin_user":null,"admin_pass":null}' "$BASE/git/create"

P1=$(alloc_tab_port); P2=$(alloc_tab_port)
t "GIT: CREATE with ssl+ports" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-git2\",\"ssl\":true,\"admin_user\":null,\"admin_pass\":null,\"http_port\":$P1,\"ssh_port\":$P2}" "$BASE/git/create"

t "GIT: CREATE with admin user" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-git3","ssl":false,"admin_user":"admin","admin_pass":"pass123"}' "$BASE/git/create"
sleep 2

# --- loadGit() → GET /api/git ---
t "GIT: LIST (after creates)" -H "Authorization: $TOKEN" "$BASE/git"

# --- gitStatus(name) → GET /api/git/{name}/status ---
t "GIT: STATUS tx-git1" -H "Authorization: $TOKEN" "$BASE/git/tx-git1/status"

# --- gitAction(name,'start') → POST /api/git/{name}/start ---
t "GIT: START tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git1/start"

# --- gitAction(name,'stop') → POST /api/git/{name}/stop ---
t "GIT: STOP tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git1/stop"

# --- gitAction(name,'publish') → POST /api/git/{name}/publish {ssl} ---
t "GIT: PUBLISH tx-git1 (ssl=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"ssl":false}' "$BASE/git/tx-git1/publish"

# --- gitAction(name,'hide') → POST /api/git/{name}/hide ---
t "GIT: HIDE tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git1/hide"

# --- gitRegistration(name, enable) → POST /api/git/{name}/registration {enable} ---
t "GIT: REGISTRATION enable tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"enable":true}' "$BASE/git/tx-git1/registration"
t "GIT: REGISTRATION disable tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"enable":false}' "$BASE/git/tx-git1/registration"

# --- gitRegistration status → GET /api/git/{name}/registration/status ---
t "GIT: REGISTRATION STATUS tx-git1" -H "Authorization: $TOKEN" "$BASE/git/tx-git1/registration/status"

# --- gitEditSave() → POST /api/git/{name}/edit ---
P3=$(alloc_port)
t "GIT: EDIT tx-git1 (http_port=$P3, auto_ports=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"http_port\":$P3,\"https_port\":null,\"ssh_port\":null,\"auto_ports\":false}" "$BASE/git/tx-git1/edit"

# --- gitAction(null,'watcher') → POST /api/git/watcher ---
# NOTE: Watcher may succeed or timeout depending on state
t "GIT: WATCHER" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/watcher"

# --- gitUserList() → POST /api/git/user/list ---
# Use tx-git3 which was created with admin credentials
t "GIT USER: LIST tx-git3" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"server":"tx-git3","admin_user":"admin","admin_pass":"pass123"}' "$BASE/git/user/list"

# --- gitUserCreate() → POST /api/git/user/create ---
t "GIT USER: CREATE tx-user1 on tx-git3" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"server":"tx-git3","username":"tx-user1","email":"tx@test.com","password":"txpass123","is_admin":false,"admin_user":"admin","admin_pass":"pass123"}' "$BASE/git/user/create"

# --- gitUserDelete() → POST /api/git/user/delete ---
t "GIT USER: DELETE tx-user1 on tx-git3" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"server":"tx-git3","username":"tx-user1","admin_user":"admin","admin_pass":"pass123"}' "$BASE/git/user/delete"

# --- gitAction(name,'delete') → POST /api/git/{name}/delete ---
t "GIT: DELETE tx-git3" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git3/delete"
t "GIT: DELETE tx-git2" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git2/delete"
t "GIT: DELETE tx-git1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/git/tx-git1/delete"
sleep 2

# =============================================================================
# TAB 4: WORDPRESS
# Buttons: Create (with/without port), Start, Stop, Restart, Publish, Hide,
#          Update, Config, Status, Edit, Delete
# =============================================================================
echo "===== TAB 4: WORDPRESS =====" >> "$OUT"
cleanup_subprocs

# --- loadWp() → GET /api/wp ---
t "WP: LIST" -H "Authorization: $TOKEN" "$BASE/wp"

# --- wpCreate() → POST /api/wp/create ---
t "WP: CREATE default (no port)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-wp1","http_port":null}' "$BASE/wp/create"

P=$(alloc_tab_port)
t "WP: CREATE with port=$P" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-wp2\",\"http_port\":$P}" "$BASE/wp/create"

# --- loadWp() → GET /api/wp ---
t "WP: LIST (after creates)" -H "Authorization: $TOKEN" "$BASE/wp"

# --- wpAction(name,'start') → POST /api/wp/{name}/start ---
t "WP: START tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/start"

# --- wpAction(name,'stop') → POST /api/wp/{name}/stop ---
t "WP: STOP tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/stop"

# --- wpRestart(name) → POST /api/wp/{name}/restart ---
t "WP: RESTART tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/restart"

# --- wpAction(name,'publish') → POST /api/wp/{name}/publish ---
t "WP: PUBLISH tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/publish"

# --- wpAction(name,'hide') → POST /api/wp/{name}/hide ---
t "WP: HIDE tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/hide"

# --- wpUpdate(name) → POST /api/wp/{name}/update ---
t "WP: UPDATE tx-wp1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp1/update"

# --- wpConfig(name) → GET /api/wp/{name}/config ---
t "WP: CONFIG tx-wp1" -H "Authorization: $TOKEN" "$BASE/wp/tx-wp1/config"

# --- wpStatus(name) → GET /api/wp/{name}/status ---
t "WP: STATUS tx-wp1" -H "Authorization: $TOKEN" "$BASE/wp/tx-wp1/status"

# --- wpEditSave() → POST /api/wp/{name}/edit ---
# NOTE: WP edit requires the site to be published (in Tor services)
# Publish tx-wp2 first, then edit
P=$(alloc_tab_port)
t "WP: PUBLISH tx-wp2 (for edit)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp2/publish"
t "WP: EDIT tx-wp2 (http_port=$P, ssl=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"http_port\":$P,\"https_port\":null,\"ssl\":false,\"auto_ports\":false}" "$BASE/wp/tx-wp2/edit"
t "WP: HIDE tx-wp2 (after edit)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/tx-wp2/hide"

# --- wpAction(name,'delete') → POST /api/wp/{name}/delete ---
for w in tx-wp2 tx-wp1; do
    t "WP: DELETE $w" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/wp/$w/delete"
done

# =============================================================================
# TAB 5: CMS
# Buttons: Create (5 types, with port), List, Start, Stop, Publish, Hide,
#          Status, Edit, Delete, Build Strapi Image
# =============================================================================
echo "===== TAB 5: CMS =====" >> "$OUT"
cleanup_subprocs

# --- cmsList() → GET /api/cms/{type}/list (for each type) ---
for type in drupal ghost magnolia strapi wagtail; do
    t "CMS: LIST $type" -H "Authorization: $TOKEN" "$BASE/cms/$type/list"
done

# --- cmsCreate() → POST /api/cms/{type}/create ---
P=$(alloc_tab_port)
t "CMS: CREATE drupal (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-drupal1\",\"http_port\":$P}" "$BASE/cms/drupal/create"

P=$(alloc_tab_port)
t "CMS: CREATE ghost (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-ghost1\",\"http_port\":$P}" "$BASE/cms/ghost/create"

P=$(alloc_tab_port)
t "CMS: CREATE magnolia (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-mag1\",\"http_port\":$P}" "$BASE/cms/magnolia/create"

P=$(alloc_tab_port)
t "CMS: CREATE wagtail (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-wag1\",\"http_port\":$P}" "$BASE/cms/wagtail/create"

# Note: Strapi create may fail if image not built — that's expected
P=$(alloc_tab_port)
t_expect_error "CMS: CREATE strapi (port=$P, image not built)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"name\":\"tx-strapi1\",\"http_port\":$P}" "$BASE/cms/strapi/create"

# Wait for containers to initialize
sleep 3

# --- cmsList() → GET /api/cms/{type}/list (verify) ---
t "CMS: LIST drupal (after create)" -H "Authorization: $TOKEN" "$BASE/cms/drupal/list"

# --- cmsAction(type,name,'start') → POST /api/cms/{type}/{name}/start ---
t "CMS: START drupal tx-drupal1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/cms/drupal/tx-drupal1/start"
sleep 3

# --- cmsAction(type,name,'publish') → POST /api/cms/{type}/{name}/publish ---
t "CMS: PUBLISH drupal tx-drupal1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/cms/drupal/tx-drupal1/publish"

# --- cmsAction(type,name,'hide') → POST /api/cms/{type}/{name}/hide ---
t "CMS: HIDE drupal tx-drupal1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/cms/drupal/tx-drupal1/hide"

# --- cmsStatus(type,name) → GET /api/cms/{type}/{name}/status ---
t "CMS: STATUS drupal tx-drupal1" -H "Authorization: $TOKEN" "$BASE/cms/drupal/tx-drupal1/status"

# --- cmsEdit(type,name) → POST /api/cms/{type}/{name}/edit ---
P=$(alloc_tab_port)
t "CMS: EDIT drupal tx-drupal1 (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"http_port\":$P}" "$BASE/cms/drupal/tx-drupal1/edit"

# --- cmsAction(type,name,'stop') → POST /api/cms/{type}/{name}/stop ---
t "CMS: STOP drupal tx-drupal1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/cms/drupal/tx-drupal1/stop"

# --- strapiBuildImage() → POST /api/cms/strapi/build-image ---
# NOTE: Docker build can take >30s — use 120s timeout for this specific test
resp=$(curl -s -w '\n__HTTP_CODE__%{http_code}' --max-time 120 -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"force":false}' "$BASE/cms/strapi/build-image" 2>&1) || true
code=$(echo "$resp" | grep '__HTTP_CODE__' | sed 's/.*__HTTP_CODE__//')
body=$(echo "$resp" | sed '/__HTTP_CODE__/d')
echo "--- CMS: BUILD STRAPI IMAGE (no force) ---" >> "$OUT"
echo "$body" >> "$OUT"
echo "" >> "$OUT"
if echo "$body" | grep -q '"error"'; then
    echo "FAIL  CMS: BUILD STRAPI IMAGE (no force)  → HTTP $code" >> "$ERRORS_FILE"
    FAIL=$((FAIL + 1))
else
    PASS=$((PASS + 1))
fi

# --- cmsAction(type,name,'delete') → POST /api/cms/{type}/{name}/delete {force:true} ---
# Frontend sends force:true (line 635 in app.js)
for type_name in "drupal:tx-drupal1" "ghost:tx-ghost1" "magnolia:tx-mag1" "wagtail:tx-wag1"; do
    type="${type_name%%:*}"
    name="${type_name##*:}"
    t "CMS: DELETE $type $name (force=true)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
      -d '{"force":true}' "$BASE/cms/$type/$name/delete"
done

# =============================================================================
# TAB 6: FILES
# Buttons: Create (4 combos: auth/ssl), Edit, Fix Perms, Delete
# =============================================================================
echo "===== TAB 6: FILES =====" >> "$OUT"
cleanup_subprocs

# --- loadFiles() → GET /api/files ---
t "FILES: LIST" -H "Authorization: $TOKEN" "$BASE/files"

# --- filesCreate() → POST /api/files/create ---
t "FILES: CREATE default (no auth, no ssl)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-share1","auth":false,"ssl":false}' "$BASE/files/create"

t "FILES: CREATE with auth" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-share2","auth":true,"ssl":false}' "$BASE/files/create"

t "FILES: CREATE with ssl" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-share3","auth":false,"ssl":true}' "$BASE/files/create"

t "FILES: CREATE with auth+ssl" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"tx-share4","auth":true,"ssl":true}' "$BASE/files/create"

# --- loadFiles() → GET /api/files ---
t "FILES: LIST (after creates)" -H "Authorization: $TOKEN" "$BASE/files"

# --- filesEditSave() → POST /api/files/{name}/edit ---
P=$(alloc_port)
t "FILES: EDIT tx-share1 (port=$P)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"port\":$P}" "$BASE/files/tx-share1/edit"

# --- filesFixPerms(name) → POST /api/files/{name}/fix-perms ---
t "FILES: FIX-PERMS tx-share1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/files/tx-share1/fix-perms"

# --- filesDelete(name) → POST /api/files/{name}/delete ---
for f in tx-share2 tx-share3 tx-share4 tx-share1; do
    t "FILES: DELETE $f" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/files/$f/delete"
done

# =============================================================================
# TAB 7: PORTS (auto-load)
# Functions: loadPorts() → GET /api/ports
# =============================================================================
echo "===== TAB 7: PORTS =====" >> "$OUT"
cleanup_subprocs
t "PORTS: LIST" -H "Authorization: $TOKEN" "$BASE/ports"

# =============================================================================
# TAB 8: SECURITY (Firewall + AppArmor)
# Firewall: Status, Setup (with/without ssh-port), Allow (tcp/udp/both, from),
#           Deny (tcp/udp/both)
# AppArmor: Status, Setup (enforce/complain, force), Mode (enforce/complain/disable, profile)
# =============================================================================
echo "===== TAB 8: SECURITY — FIREWALL =====" >> "$OUT"
cleanup_subprocs

# --- loadFirewall() → GET /api/firewall/status ---
t "FW: STATUS" -H "Authorization: $TOKEN" "$BASE/firewall/status"

# --- firewallSetup() → POST /api/firewall/setup ---
t "FW: SETUP default (no ssh_port)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/firewall/setup"
t "FW: SETUP ssh_port=22" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"ssh_port":22}' "$BASE/firewall/setup"

# --- firewallAllow() → POST /api/firewall/allow ---
t "FW: ALLOW 9999 tcp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"port":9999,"proto":"tcp","from":null}' "$BASE/firewall/allow"
t "FW: ALLOW 9998 udp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"port":9998,"proto":"udp","from":null}' "$BASE/firewall/allow"
t "FW: ALLOW 9997 tcp from 10.0.0.0/8" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"port":9997,"proto":"tcp","from":"10.0.0.0/8"}' "$BASE/firewall/allow"

# --- firewallDeny() → POST /api/firewall/deny ---
t "FW: DENY 9999 tcp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"port":9999,"proto":"tcp"}' "$BASE/firewall/deny"
t "FW: DENY 9998 udp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"port":9998,"proto":"udp"}' "$BASE/firewall/deny"

echo "===== TAB 8: SECURITY — APPARMOR =====" >> "$OUT"
cleanup_subprocs

# --- loadAppArmor() → GET /api/apparmor/status ---
t "AA: STATUS" -H "Authorization: $TOKEN" "$BASE/apparmor/status"

# --- apparmorSetup() → POST /api/apparmor/setup ---
t "AA: SETUP enforce (no force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"enforce","force":false}' "$BASE/apparmor/setup"
t "AA: SETUP complain (no force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"complain","force":false}' "$BASE/apparmor/setup"
t "AA: SETUP enforce+force" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"enforce","force":true}' "$BASE/apparmor/setup"

# --- apparmorMode() → POST /api/apparmor/mode ---
t "AA: MODE enforce" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"enforce","profile":null}' "$BASE/apparmor/mode"
t "AA: MODE complain" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"complain","profile":null}' "$BASE/apparmor/mode"
t "AA: MODE disable" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"mode":"disable","profile":null}' "$BASE/apparmor/mode"

# =============================================================================
# TAB 9: VPN
# Buttons: Create (with/without port, subnet, autostart), Start, Stop, Delete
# Peer: Add, Add by Pubkey, Remove
# =============================================================================
echo "===== TAB 9: VPN =====" >> "$OUT"
cleanup_subprocs

# --- loadVpn() → GET /api/vpn/list ---
t "VPN: LIST" -H "Authorization: $TOKEN" "$BASE/vpn/list"

# --- vpnCreate() → POST /api/vpn/create ---
# NOTE: Frontend sends {interface, port, subnet, autostart} (NOT "name")
# Clean up any leftover interface from previous runs
sudo wg-quick down txwg1 2>/dev/null || true
sudo rm -f /etc/wireguard/txwg1.conf 2>/dev/null || true
P=$(alloc_tab_port)
t "VPN: CREATE tx-wg1 (port=$P, subnet=10.8.0.0/24, autostart=true)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"interface\":\"txwg1\",\"port\":$P,\"subnet\":\"10.8.0.0/24\",\"autostart\":true}" "$BASE/vpn/create"

# --- loadVpn() → GET /api/vpn/list ---
t "VPN: LIST (after create)" -H "Authorization: $TOKEN" "$BASE/vpn/list"

# --- VPN status (loadVpn calls GET /api/vpn/status/{iface}) ---
t "VPN: STATUS txwg1" -H "Authorization: $TOKEN" "$BASE/vpn/status/txwg1"

# --- vpnAction(iface,'start') → POST /api/vpn/{iface}/start ---
# NOTE: autostart=true in create already starts it, so stop first then start
sudo wg-quick down txwg1 2>/dev/null || true
sleep 1
t "VPN: START txwg1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/vpn/txwg1/start"

# --- vpnAction(iface,'stop') → POST /api/vpn/{iface}/stop ---
t "VPN: STOP txwg1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/vpn/txwg1/stop"

# --- vpnPeerAdd() → POST /api/vpn/peer/add ---
# NOTE: VPN interface must be up for peer operations — start it first
sudo wg-quick up txwg1 2>/dev/null || true
sleep 1
t "VPN PEER: ADD peer1 to txwg1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"interface":"txwg1","peer_name":"tx-peer1","endpoint":null,"dns":null,"psk":false,"ip":"10.8.0.3"}' "$BASE/vpn/peer/add"

# --- vpnPeerAddPubkey() → POST /api/vpn/peer/add-pubkey ---
t "VPN PEER: ADD-PUBKEY peer2 to txwg1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"interface":"txwg1","peer_name":"tx-peer2","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","ip":"10.8.0.5"}' "$BASE/vpn/peer/add-pubkey"

# --- vpnPeerRemove() → POST /api/vpn/peer/remove ---
t "VPN PEER: REMOVE from txwg1" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"interface":"txwg1","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}' "$BASE/vpn/peer/remove"

# --- vpnAction(iface,'delete') → POST /api/vpn/{iface}/delete {sync_firewall:false} ---
t "VPN: DELETE txwg1 (sync_firewall=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"sync_firewall":false}' "$BASE/vpn/txwg1/delete"

# =============================================================================
# TAB 10: LOGS
# Buttons: View (4 sources × lines), Install Logs, Smoke-Test Logs
# =============================================================================
echo "===== TAB 10: LOGS =====" >> "$OUT"
cleanup_subprocs

# --- loadLogSources() → GET /api/logs/sources ---
t "LOGS: SOURCES" -H "Authorization: $TOKEN" "$BASE/logs/sources"

# --- logsView() → GET /api/logs/view?source=X&lines=Y ---
for src in system tor nginx docker; do
    t "LOGS: VIEW $src 20 lines" -H "Authorization: $TOKEN" "$BASE/logs/view?source=$src&lines=20"
done

# --- logsInstall() → GET /api/logs/install ---
t "LOGS: INSTALL" -H "Authorization: $TOKEN" "$BASE/logs/install"

# --- logsSmokeTest() → GET /api/logs/smoke-test ---
t "LOGS: SMOKE-TEST" -H "Authorization: $TOKEN" "$BASE/logs/smoke-test"

# =============================================================================
# TAB 11: MAINTENANCE
# Buttons: Status, Timer Status, Enable/Disable Checks, Backup, Smoke Test (disabled),
#          SSH Config, SSH Harden PQC (force, dry_run), Cleanup (3 targets × dry_run)
# =============================================================================
echo "===== TAB 11: MAINTENANCE =====" >> "$OUT"
cleanup_subprocs

# --- maintStatus() → GET /api/maintenance/status ---
t "MAINT: STATUS" -H "Authorization: $TOKEN" "$BASE/maintenance/status"

# --- maintTimerStatus() → GET /api/maintenance/timer-status ---
t "MAINT: TIMER STATUS" -H "Authorization: $TOKEN" "$BASE/maintenance/timer-status"

# --- maintEnableChecks() → POST /api/maintenance/enable-checks ---
t "MAINT: ENABLE CHECKS" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/maintenance/enable-checks"

# --- maintDisableChecks() → POST /api/maintenance/disable-checks ---
t "MAINT: DISABLE CHECKS" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/maintenance/disable-checks"

# --- maintSshConfig() → GET /api/maintenance/ssh-config ---
t "MAINT: SSH CONFIG" -H "Authorization: $TOKEN" "$BASE/maintenance/ssh-config"

# --- maintSshHardenPqc() → POST /api/maintenance/ssh-harden-pqc ---
t "MAINT: SSH HARDEN PQC (dry_run, no force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"force":false,"dry_run":true}' "$BASE/maintenance/ssh-harden-pqc"
t "MAINT: SSH HARDEN PQC (dry_run+force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"force":true,"dry_run":true}' "$BASE/maintenance/ssh-harden-pqc"

# --- maintBackup() → POST /api/maintenance/backup ---
t "MAINT: BACKUP" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/maintenance/backup"

# --- maintCleanup() → POST /api/maintenance/cleanup ---
# Only 3 targets: all, docker, logs (temp and backups removed from UI)
for target in all docker logs; do
    t "MAINT: CLEANUP $target (dry_run, keep_days=30)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
      -d "{\"target\":\"$target\",\"keep_days\":30,\"dry_run\":true}" "$BASE/maintenance/cleanup"
done

# --- maintSmokeTest() → POST /api/maintenance/smoke-test (disabled in lite) ---
t_skip "MAINT: SMOKE TEST (disabled in lite)"

# =============================================================================
# TAB 12: DIAGNOSTICS
# Buttons: Summary, NGINX, Tor, SSH, WordPress, WP Sync, NGINX Test, Resources
# =============================================================================
echo "===== TAB 12: DIAGNOSTICS =====" >> "$OUT"
cleanup_subprocs

# --- diagSummary() → GET /api/diag/summary ---
t "DIAG: SUMMARY" -H "Authorization: $TOKEN" "$BASE/diag/summary"

# --- diagNginx() → GET /api/diag/nginx ---
t "DIAG: NGINX" -H "Authorization: $TOKEN" "$BASE/diag/nginx"

# --- diagTor() → GET /api/diag/tor ---
t "DIAG: TOR" -H "Authorization: $TOKEN" "$BASE/diag/tor"

# --- diagSsh() → GET /api/diag/ssh ---
t "DIAG: SSH" -H "Authorization: $TOKEN" "$BASE/diag/ssh"

# --- diagWordpress() → GET /api/diag/wordpress ---
t "DIAG: WORDPRESS" -H "Authorization: $TOKEN" "$BASE/diag/wordpress"

# --- diagWpSync() → GET /api/diag/wp-sync ---
t "DIAG: WP-SYNC" -H "Authorization: $TOKEN" "$BASE/diag/wp-sync"

# --- diagNginxTest() → GET /api/diag/nginx-test ---
t "DIAG: NGINX TEST" -H "Authorization: $TOKEN" "$BASE/diag/nginx-test"

# --- diagResources() → GET /api/diag/resources ---
t "DIAG: RESOURCES" -H "Authorization: $TOKEN" "$BASE/diag/resources"

# =============================================================================
# TAB 13: TEST
# Buttons: Run Tests (with/without filter), List Tests, Benchmark (disabled),
#          Results, Clean
# =============================================================================
echo "===== TAB 13: TEST =====" >> "$OUT"
cleanup_subprocs

# --- testList() → GET /api/test/list ---
t "TEST: LIST" -H "Authorization: $TOKEN" "$BASE/test/list"

# --- testRun() → POST /api/test/run ---
t "TEST: RUN (no filter)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"filter":null}' "$BASE/test/run"
t "TEST: RUN (filter='tor')" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"filter":"tor"}' "$BASE/test/run"

# --- testBenchmark() → POST /api/test/benchmark (disabled in lite) ---
t_skip "TEST: BENCHMARK (disabled in lite)"

# --- testResults() → GET /api/test/results ---
t "TEST: RESULTS" -H "Authorization: $TOKEN" "$BASE/test/results"

# --- testClean() → POST /api/test/clean ---
t "TEST: CLEAN" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{}' "$BASE/test/clean"

# =============================================================================
# TAB 14: DOCTOR
# Buttons: Run Diagnostics, Security Audit
# =============================================================================
echo "===== TAB 14: DOCTOR =====" >> "$OUT"
cleanup_subprocs

# --- doctorRun() → GET /api/doctor ---
t "DOCTOR: RUN" -H "Authorization: $TOKEN" "$BASE/doctor"

# --- doctorSecurity() → GET /api/doctor/security ---
t "DOCTOR: SECURITY" -H "Authorization: $TOKEN" "$BASE/doctor/security"

# =============================================================================
# TAB 15: SETUP
# Buttons: Run Setup (all, vpn, security, pqc-tls)
# NOTE: pqc-tls uses SSE streaming — tested separately, not via simple curl
# =============================================================================
echo "===== TAB 15: SETUP =====" >> "$OUT"
cleanup_subprocs

# --- setupRun() → POST /api/setup ---
t "SETUP: all=true" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"all":true,"vpn":false,"security":false,"pqc_tls":false}' "$BASE/setup"
t "SETUP: vpn=true" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"all":false,"vpn":true,"security":false,"pqc_tls":false}' "$BASE/setup"
t "SETUP: security=true" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"all":false,"vpn":false,"security":true,"pqc_tls":false}' "$BASE/setup"

# PQC TLS uses SSE (EventSource) — test with curl streaming
t "SETUP: PQC TLS (SSE stream)" -H "Authorization: $TOKEN" --max-time 10 "$BASE/setup/pqc-tls"

# =============================================================================
# TAB 16: SYSTEM
# Buttons: Quick Reference, License, Config Show, Config Validate (reachable),
#          Verify (PQC), Uninstall (yes, keep-data, remove-deps, force, only)
# =============================================================================
echo "===== TAB 16: SYSTEM =====" >> "$OUT"
cleanup_subprocs

# --- sysQuickref() → GET /api/quickref ---
t "SYS: QUICKREF" -H "Authorization: $TOKEN" "$BASE/quickref"

# --- sysLicense() → GET /api/license ---
t "SYS: LICENSE" -H "Authorization: $TOKEN" "$BASE/license"

# --- sysConfigShow() → GET /api/config/show ---
t "SYS: CONFIG SHOW" -H "Authorization: $TOKEN" "$BASE/config/show"

# --- sysConfigValidate() → POST /api/config/validate ---
t "SYS: CONFIG VALIDATE (reachable=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"reachable":false}' "$BASE/config/validate"
t "SYS: CONFIG VALIDATE (reachable=true)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"reachable":true}' "$BASE/config/validate"

# --- sysVerify() → POST /api/verify ---
t "SYS: VERIFY (non-existent file)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"file":"/tmp/nonexistent.tar.gz","pqsig":null,"pubkey":null}' "$BASE/verify"

# --- sysUninstall() → POST /api/uninstall ---
# NOTE: We test with keep_data=true to avoid actually uninstalling
t "SYS: UNINSTALL (yes=true, keep_data=true, dry run)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"yes":true,"keep_data":true,"remove_deps":false,"force":false,"only":null}' "$BASE/uninstall"

# =============================================================================
# TAB 17: UPDATE
# Buttons: Check (force/no-force), Feed Schema, Verify Feed, Download (4 flags),
#          Apply (binary, allow-unsigned)
# =============================================================================
echo "===== TAB 17: UPDATE =====" >> "$OUT"
cleanup_subprocs

# --- updCheck() → POST /api/update/check ---
t "UPDATE: CHECK (no force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"force":false}' "$BASE/update/check"
t "UPDATE: CHECK (force=true)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"force":true}' "$BASE/update/check"

# --- updSchema() → GET /api/update/schema ---
t "UPDATE: SCHEMA" -H "Authorization: $TOKEN" "$BASE/update/schema"

# --- updVerifyFeed() → POST /api/update/verify-feed ---
t "UPDATE: VERIFY FEED (dummy URL)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"source":"https://example.com/feed.json","signature":null}' "$BASE/update/verify-feed"

# --- updDownload() → POST /api/update/download ---
t "UPDATE: DOWNLOAD (dry_run, no yes)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"yes":false,"dry_run":true,"force":false,"allow_unsigned":false}' "$BASE/update/download"
t "UPDATE: DOWNLOAD (yes+dry_run)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"yes":true,"dry_run":true,"force":false,"allow_unsigned":false}' "$BASE/update/download"
t "UPDATE: DOWNLOAD (yes+dry_run+force)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"yes":true,"dry_run":true,"force":true,"allow_unsigned":false}' "$BASE/update/download"
t "UPDATE: DOWNLOAD (yes+dry_run+allow_unsigned)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"yes":true,"dry_run":true,"force":false,"allow_unsigned":true}' "$BASE/update/download"

# --- updApply() → POST /api/update/apply ---
# NOTE: Use dry_run via dummy binary path to avoid actual update
t "UPDATE: APPLY (dummy binary, allow_unsigned=false)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"binary":"/tmp/nonexistent.bin","allow_unsigned":false}' "$BASE/update/apply"
t "UPDATE: APPLY (dummy binary, allow_unsigned=true)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"binary":"/tmp/nonexistent.bin","allow_unsigned":true}' "$BASE/update/apply"

# =============================================================================
# TAB 18: DOCS
# Buttons: Show (8 topics + search with filter)
# =============================================================================
echo "===== TAB 18: DOCS =====" >> "$OUT"
cleanup_subprocs

# --- docsShow() → GET /api/docs/{topic} ---
for topic in quickstart commands concepts faq examples quantum-security verify-downloads; do
    t "DOCS: $topic" -H "Authorization: $TOKEN" "$BASE/docs/$topic"
done

# --- docsShow() with filter → GET /api/docs/{topic}/{filter} ---
t "DOCS: quickstart with filter 'tor'" -H "Authorization: $TOKEN" "$BASE/docs/quickstart/tor"
t "DOCS: commands with filter 'vpn'" -H "Authorization: $TOKEN" "$BASE/docs/commands/vpn"

# --- docsShow() search → GET /api/docs/search/{term} ---
t "DOCS: search 'tor'" -H "Authorization: $TOKEN" "$BASE/docs/search/tor"
t "DOCS: search 'vpn'" -H "Authorization: $TOKEN" "$BASE/docs/search/vpn"
t "DOCS: search 'firewall'" -H "Authorization: $TOKEN" "$BASE/docs/search/firewall"

# =============================================================================
# TAB 19: CONSOLE
# Presets: 33 presets in dropdown + free-form commands with flags
# =============================================================================
echo "===== TAB 19: CONSOLE =====" >> "$OUT"
cleanup_subprocs

# --- consoleRun() → POST /api/console/run {args, timeout_secs} ---
# Read-only presets (safe)
for cmd in "tor list" "git list" "wp list" "files list" "vpn list" "firewall status" "apparmor status" "ports list" "doctor" "logs list" "maintenance status" "diag summary" "test list" "quickref" "license" "config-show" "config-validate" "update check" "update schema" "docs quickstart"; do
    # Build JSON args array properly
    args_json=$(echo "$cmd" | awk '{for(i=1;i<=NF;i++) printf "\"%s\"%s", $i, (i<NF?",":"")}')
    t "CONSOLE: $cmd" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
      -d "{\"args\":[$args_json],\"timeout_secs\":120}" "$BASE/console/run"
done

# Console: doctor --security
t "CONSOLE: doctor --security" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["doctor","--security"],"timeout_secs":120}' "$BASE/console/run"

# Console: create commands with flags
t "CONSOLE: tor create --name tx-ctor --service-type web" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["tor","create","--name","tx-ctor","--service-type","web"],"timeout_secs":60}' "$BASE/console/run"
t "CONSOLE: tor list (after create)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["tor","list"],"timeout_secs":60}' "$BASE/console/run"
t "CONSOLE: tor remove tx-ctor" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["tor","remove","tx-ctor"],"timeout_secs":60}' "$BASE/console/run"

# Console: ports check with flag
t "CONSOLE: ports check --port 8082" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["ports","check","--port","8082"],"timeout_secs":60}' "$BASE/console/run"

# Console: firewall setup with flag
t "CONSOLE: firewall setup --ssh-port 22 --force" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["firewall","setup","--ssh-port","22","--force"],"timeout_secs":60}' "$BASE/console/run"

# Console: apparmor setup with flag
t "CONSOLE: apparmor setup --mode enforce --force" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["apparmor","setup","--mode","enforce","--force"],"timeout_secs":60}' "$BASE/console/run"

# Console: update check --json
t "CONSOLE: update check --json" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["update","check","--json"],"timeout_secs":60}' "$BASE/console/run"

# Console: drupal create with flags
P=$(alloc_port)
t "CONSOLE: drupal create --name tx-cdrupal --http-port $P" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"args\":[\"drupal\",\"create\",\"--name\",\"tx-cdrupal\",\"--http-port\",\"$P\"],\"timeout_secs\":120}" "$BASE/console/run"
t "CONSOLE: drupal delete tx-cdrupal --force" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["drupal","delete","tx-cdrupal","--force"],"timeout_secs":60}' "$BASE/console/run"

# Console: git create with flags
t "CONSOLE: git create --name tx-cgit --ssl" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["git","create","--name","tx-cgit","--ssl"],"timeout_secs":120}' "$BASE/console/run"
t "CONSOLE: git delete tx-cgit" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["git","delete","tx-cgit"],"timeout_secs":60}' "$BASE/console/run"

# Console: wp create with flags
t "CONSOLE: wp create --name tx-cwp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["wp","create","--name","tx-cwp"],"timeout_secs":120}' "$BASE/console/run"
t "CONSOLE: wp delete tx-cwp" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["wp","delete","tx-cwp"],"timeout_secs":60}' "$BASE/console/run"

# Console: files create with flags
t "CONSOLE: files create --name tx-cfiles --auth" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["files","create","--name","tx-cfiles","--auth"],"timeout_secs":120}' "$BASE/console/run"
t "CONSOLE: files delete tx-cfiles" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["files","delete","tx-cfiles"],"timeout_secs":60}' "$BASE/console/run"

# Console: vpn create with flags
t "CONSOLE: vpn create tx-cwg --port 51850" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["vpn","create","tx-cwg","--port","51850"],"timeout_secs":60}' "$BASE/console/run"
t "CONSOLE: vpn delete tx-cwg" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["vpn","delete","tx-cwg"],"timeout_secs":60}' "$BASE/console/run"

# Console: setup --all
t "CONSOLE: setup --all" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["setup","--all"],"timeout_secs":120}' "$BASE/console/run"

# Console: git watcher (skip — runs in foreground indefinitely, not suitable for automated test)
t_skip "CONSOLE: git watcher (foreground process)"

# Console: update verify-feed (preset)
t "CONSOLE: update verify-feed" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["update","verify-feed"],"timeout_secs":60}' "$BASE/console/run"

# Console: update download --yes (preset, dry-run to avoid actual download)
t "CONSOLE: update download --yes --dry-run" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["update","download","--yes","--dry-run"],"timeout_secs":60}' "$BASE/console/run"

# --- Negative tests: invalid commands must return HTTP 400 ---
t_expect_error "CONSOLE NEG: cms create (command does not exist)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["cms","create","--type","drupal","--name","tx-neg"],"timeout_secs":60}' "$BASE/console/run"

t_expect_error "CONSOLE NEG: vpn create --interface (obsolete flag)" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["vpn","create","--interface","wg0","--port","51850"],"timeout_secs":60}' "$BASE/console/run"

t_expect_error "CONSOLE NEG: nonexistent command" -X POST -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"args":["comando_falso","list"],"timeout_secs":60}' "$BASE/console/run"

# =============================================================================
# SUMMARY
# =============================================================================
echo "" >> "$OUT"
echo "======================================================================" >> "$OUT"
echo "SUMMARY" >> "$OUT"
echo "======================================================================" >> "$OUT"
echo "PASS: $PASS" >> "$OUT"
echo "FAIL: $FAIL" >> "$OUT"
echo "SKIP: $SKIP (disabled in UI)" >> "$OUT"
echo "TOTAL: $((PASS + FAIL + SKIP))" >> "$OUT"
echo "" >> "$OUT"

if [ "$FAIL" -gt 0 ]; then
    echo "ERRORS (see $ERRORS_FILE):" >> "$OUT"
    cat "$ERRORS_FILE" >> "$OUT"
fi

echo ""
echo "======================================================================"
echo "EXHAUSTIVE TEST COMPLETE"
echo "  PASS: $PASS"
echo "  FAIL: $FAIL"
echo "  SKIP: $SKIP (disabled in UI)"
echo "  TOTAL: $((PASS + FAIL + SKIP))"
echo ""
echo "  Full log:   $OUT"
echo "  Errors:     $ERRORS_FILE"
if [ "$FAIL" -gt 0 ]; then
    echo ""
    echo "  ⚠  $FAIL failures detected:"
    cat "$ERRORS_FILE"
fi
echo "======================================================================"

# Cleanup temp files
rm -f "$PORT_FILE" 2>/dev/null
