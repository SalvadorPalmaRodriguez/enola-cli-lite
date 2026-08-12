#!/usr/bin/env bash
# uninstall.sh — Remove enola-cli and all associated resources
#
# Usage:
#   bash scripts/ops/uninstall.sh [OPTIONS]
#
# Options:
#   --yes         Execute removal (default: dry-run, just show what would be removed)
#   --keep-data   Preserve /srv/enola-* data directories
#   --only=X,Y    Only process specified sections (comma-separated)
#   --force       Continue on non-critical errors
#   --remove-deps Also uninstall third-party deps that Enola installed (Docker, Tor, etc.)
#                 Only removes deps that Enola actually installed (per manifest).
#                 Deps the user had before Enola are NEVER removed.
#
# Sections: binary, config, services, tor, nginx, systemd, apparmor, docker, ufw, data, deps
#
# Part of enola-cli (UNINSTALL-FIX-001)
set -euo pipefail

# ─── Globals ──────────────────────────────────────────────────────────────────
DRY_RUN=true
KEEP_DATA=false
FORCE=false
REMOVE_DEPS=false
ONLY_SECTIONS=""
BINARY_PATH="/usr/local/bin/enola-cli"
SHARE_DIR="/usr/local/share/enola"
# Try SUDO_USER home first, then HOME, then /root
if [ -n "${SUDO_USER:-}" ] && [ "${SUDO_USER:-}" != "root" ]; then
    CONFIG_DIR="/home/${SUDO_USER}/.enola"
else
    CONFIG_DIR="${HOME}/.enola"
fi
DATA_DIR="/srv"

ALL_SECTIONS="binary config services tor nginx systemd apparmor docker ufw data deps"

# ─── Manifest ─────────────────────────────────────────────────────────────────
# If install.sh created a manifest, use it as source of truth for paths and deps.
# Format: key|value per line. Keys: binary, share_dir, config_dir, opt_dir, dep_installed
MANIFEST="${SHARE_DIR}/manifest"
manifest_get() {
    [ -f "$MANIFEST" ] || return 1
    grep -m1 "^$1|" "$MANIFEST" 2>/dev/null | cut -d'|' -f2-
}
manifest_get_all() {
    [ -f "$MANIFEST" ] || return 1
    grep "^$1|" "$MANIFEST" 2>/dev/null | cut -d'|' -f2-
}

# Override paths from manifest if available
if [ -f "$MANIFEST" ]; then
    MANIFEST_BINARY="$(manifest_get binary)"
    MANIFEST_SHARE="$(manifest_get share_dir)"
    MANIFEST_CONFIG="$(manifest_get config_dir)"
    MANIFEST_OPT="$(manifest_get opt_dir)"
    [ -n "$MANIFEST_BINARY" ] && BINARY_PATH="$MANIFEST_BINARY"
    [ -n "$MANIFEST_SHARE" ] && SHARE_DIR="$MANIFEST_SHARE"
    [ -n "$MANIFEST_CONFIG" ] && CONFIG_DIR="$MANIFEST_CONFIG"
fi

# ─── Helpers ──────────────────────────────────────────────────────────────────
log()  { echo "[uninstall] $*"; }
warn() { echo "[uninstall] WARNING: $*" >&2; }
err()  { echo "[uninstall] ERROR: $*" >&2; }

run() {
    if $DRY_RUN; then
        echo "  [dry-run] $*"
    else
        echo "  [exec] $*"
        "$@" 2>&1 || {
            local rc=$?
            if $FORCE; then
                warn "Command failed (rc=$rc) but --force is set, continuing."
            else
                err "Command failed (rc=$rc). Use --force to continue past errors."
                exit $rc
            fi
        }
    fi
}

run_rm() {
    local target="$1"
    if [ -e "$target" ] || [ -L "$target" ]; then
        run rm -rf "$target"
    elif $DRY_RUN; then
        echo "  [dry-run] rm -rf $target (not found, skipping)"
    fi
}

has_section() {
    local check="$1"
    if [ -z "$ONLY_SECTIONS" ]; then
        return 0
    fi
    echo ",$ONLY_SECTIONS," | grep -q ",$check,"
}

# ─── Parse args ───────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        --yes)         DRY_RUN=false; shift ;;
        --keep-data)   KEEP_DATA=true; shift ;;
        --force)       FORCE=true; shift ;;
        --remove-deps) REMOVE_DEPS=true; shift ;;
        --only=*)      ONLY_SECTIONS="${1#--only=}"; shift ;;
        --only)        ONLY_SECTIONS="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: bash scripts/ops/uninstall.sh [--yes] [--keep-data] [--only=SECTIONS] [--force] [--remove-deps]"
            echo ""
            echo "Default mode is dry-run (no changes made). Use --yes to execute."
            echo "Sections: $ALL_SECTIONS"
            echo ""
            echo "--remove-deps: Also uninstall third-party deps that Enola installed."
            echo "  Only removes deps marked as 'dep_installed' in the manifest."
            echo "  Deps the user had before Enola are NEVER removed."
            exit 0
            ;;
        *) err "Unknown option: $1"; exit 1 ;;
    esac
done

# ─── Banner ───────────────────────────────────────────────────────────────────
if $DRY_RUN; then
    echo "══════════════════════════════════════════════════════════════"
    echo "  enola-cli uninstall — DRY RUN (no changes will be made)"
    echo "  Use --yes to execute removal."
    echo "══════════════════════════════════════════════════════════════"
else
    echo "══════════════════════════════════════════════════════════════"
    echo "  enola-cli uninstall — EXECUTING"
    echo "══════════════════════════════════════════════════════════════"
fi
if [ -f "$MANIFEST" ]; then
    echo "  Manifest: $MANIFEST"
else
    echo "  Manifest: not found (using heuristic detection)"
fi
if $REMOVE_DEPS; then
    echo "  --remove-deps: YES (will uninstall deps Enola installed)"
fi
echo ""

# ─── Section: binary ──────────────────────────────────────────────────────────
if has_section binary; then
    log "Section: binary"
    if [ -f "$BINARY_PATH" ]; then
        run_rm "$BINARY_PATH"
    else
        echo "  Binary not found at $BINARY_PATH (already removed?)"
    fi
    if [ -d "$SHARE_DIR" ]; then
        run_rm "$SHARE_DIR"
    fi
    # Remove /opt/enola (postinstall_deps.sh installed by installer)
    opt_dir="/opt/enola"
    if [ -n "${MANIFEST_OPT:-}" ]; then
        opt_dir="$MANIFEST_OPT"
    fi
    if [ -d "$opt_dir" ]; then
        run_rm "$opt_dir"
    fi
    echo ""
fi

# ─── Section: config ──────────────────────────────────────────────────────────
if has_section config; then
    log "Section: config"
    if [ -d "$CONFIG_DIR" ]; then
        run_rm "$CONFIG_DIR"
    else
        echo "  Config dir not found at $CONFIG_DIR"
    fi
    echo ""
fi

# ─── Section: services (Tor + Nginx + Docker cleanup) ─────────────────────────
if has_section services; then
    log "Section: services (stopping running services)"
    if command -v docker &>/dev/null; then
        echo "  Stopping enola Docker containers..."
        local_containers=$(docker ps -a --filter "name=enola-" --filter "name=wp-" --filter "name=db-" --filter "name=strapi-" --filter "name=wagtail-" --format '{{.Names}}' 2>/dev/null || true)
        if [ -n "$local_containers" ]; then
            for c in $local_containers; do
                run docker stop "$c"
                run docker rm "$c"
            done
        else
            echo "  No enola Docker containers found."
        fi
    else
        echo "  Docker not installed, skipping container cleanup."
    fi
    echo ""
fi

# ─── Section: tor ─────────────────────────────────────────────────────────────
if has_section tor; then
    log "Section: tor"
    TORRC="/etc/tor/torrc"
    tor_services="$(manifest_get_all tor_service 2>/dev/null || true)"
    if [ -f "$TORRC" ] || [ -n "$tor_services" ]; then
        if $DRY_RUN; then
            echo "  [dry-run] Remove Tor hidden service configs"
            [ -n "$tor_services" ] && echo "  [dry-run] Tor services from manifest: $tor_services"
        else
            # Remove specific service dirs and configs from manifest
            if [ -n "$tor_services" ]; then
                for svc in $tor_services; do
                    svc_dir="/var/lib/tor/enola_${svc}"
                    [ -d "$svc_dir" ] && run_rm "$svc_dir"
                    conf_file="/etc/tor/enola.d/${svc}.conf"
                    [ -f "$conf_file" ] && run_rm "$conf_file"
                    disabled_conf="/etc/tor/enola.d/${svc}.conf.disabled"
                    [ -f "$disabled_conf" ] && run_rm "$disabled_conf"
                done
            else
                # Fallback: glob-based cleanup
                for d in /var/lib/tor/enola_*; do
                    [ -d "$d" ] && run_rm "$d"
                done
            fi
            # Remove enola.d directory if exists
            if [ -d "/etc/tor/enola.d" ]; then
                run_rm "/etc/tor/enola.d"
            fi
            # Clean torrc: remove enola-cli managed blocks
            if [ -f "$TORRC" ]; then
                cp "$TORRC" "${TORRC}.bak"
                sed -i '/# enola-cli:begin/,/# enola-cli:end/d' "$TORRC"
                sed -i '/\/srv\/enola-/d' "$TORRC"
                sed -i '/enola\.d/d' "$TORRC"
                rm -f "${TORRC}.bak"
            fi
            echo "  Cleaned Tor configs"
        fi
    else
        echo "  No torrc found and no Tor services in manifest"
    fi
    echo ""
fi

# ─── Section: nginx ───────────────────────────────────────────────────────────
if has_section nginx; then
    log "Section: nginx"
    NGINX_SITES="/etc/nginx/sites-enabled"
    NGINX_SITES_AVAIL="/etc/nginx/sites-available"
    NGINX_CONF_D="/etc/nginx/conf.d"
    nginx_configs="$(manifest_get_all nginx_config 2>/dev/null || true)"
    if [ -n "$nginx_configs" ]; then
        for cfg in $nginx_configs; do
            for dir in "$NGINX_SITES" "$NGINX_SITES_AVAIL" "$NGINX_CONF_D"; do
                [ -f "$dir/$cfg" ] && run_rm "$dir/$cfg"
                [ -f "$dir/$cfg.conf" ] && run_rm "$dir/$cfg.conf"
            done
        done
    fi
    # Fallback: glob-based cleanup for proxy_* and fileserver_* configs
    if [ -d "$NGINX_SITES" ]; then
        for f in "$NGINX_SITES"/proxy_* "$NGINX_SITES"/fileserver_*; do
            [ -f "$f" ] && run_rm "$f"
        done
    fi
    if [ -d "$NGINX_SITES_AVAIL" ]; then
        for f in "$NGINX_SITES_AVAIL"/proxy_* "$NGINX_SITES_AVAIL"/fileserver_*; do
            [ -f "$f" ] && run_rm "$f"
        done
    fi
    if [ -d "$NGINX_CONF_D" ]; then
        for f in "$NGINX_CONF_D"/proxy_*; do
            [ -f "$f" ] && run_rm "$f"
        done
    fi
    if ! $DRY_RUN; then
        if command -v nginx &>/dev/null; then
            run nginx -t 2>/dev/null && run systemctl reload nginx 2>/dev/null || true
        fi
    fi
    echo ""
fi

# ─── Section: systemd ─────────────────────────────────────────────────────────
if has_section systemd; then
    log "Section: systemd"
    # Remove enola timers
    for timer in $(systemctl list-unit-files --type=timer 2>/dev/null | grep 'enola-' | awk '{print $1}' || true); do
        run systemctl stop "$timer" 2>/dev/null || true
        run systemctl disable "$timer" 2>/dev/null || true
        run_rm "/etc/systemd/system/${timer}"
    done
    # Remove VPN services (wg-quick@) from manifest
    vpn_services="$(manifest_get_all vpn_service 2>/dev/null || true)"
    if [ -n "$vpn_services" ]; then
        for svc in $vpn_services; do
            run systemctl stop "$svc" 2>/dev/null || true
            run systemctl disable "$svc" 2>/dev/null || true
        done
    fi
    if ! $DRY_RUN; then
        run systemctl daemon-reload 2>/dev/null || true
    fi
    echo ""
fi

# ─── Section: apparmor ────────────────────────────────────────────────────────
if has_section apparmor; then
    log "Section: apparmor"
    AA_DIR="/etc/apparmor.d"
    # Use manifest to get exact profile names
    aa_profiles="$(manifest_get_all apparmor_profile 2>/dev/null || true)"
    if [ -n "$aa_profiles" ]; then
        for profile in $aa_profiles; do
            profile_path="$AA_DIR/$profile"
            [ -f "$profile_path" ] || continue
            if ! $DRY_RUN; then
                run apparmor_parser -r "$profile_path" 2>/dev/null || true
            fi
            run_rm "$profile_path"
        done
    fi
    # Fallback: remove any remaining enola-* profiles and binary profile
    if [ -d "$AA_DIR" ]; then
        for profile in "$AA_DIR"/enola-* "$AA_DIR"/usr.local.bin.enola-cli; do
            [ -f "$profile" ] || continue
            if ! $DRY_RUN; then
                run apparmor_parser -r "$profile" 2>/dev/null || true
            fi
            run_rm "$profile"
        done
    fi
    echo ""
fi

# ─── Section: docker ──────────────────────────────────────────────────────────
if has_section docker; then
    log "Section: docker (removing enola containers and images)"
    if command -v docker &>/dev/null; then
        # Use manifest to get exact container names, fall back to filter-based detection
        manifest_containers="$(manifest_get_all docker_container 2>/dev/null || true)"
        if [ -n "$manifest_containers" ]; then
            for c in $manifest_containers; do
                if docker ps -a --format '{{.Names}}' 2>/dev/null | grep -qx "$c"; then
                    run docker rm -f "$c"
                fi
            done
        fi
        # Fallback: filter-based detection for any containers not in manifest
        containers=$(docker ps -a --filter "name=enola-" --filter "name=wp-" --filter "name=db-" --filter "name=strapi-" --filter "name=wagtail-" --filter "name=ghost-" --filter "name=magnolia-" --filter "name=drupal-" --format '{{.Names}}' 2>/dev/null || true)
        if [ -n "$containers" ]; then
            for c in $containers; do
                echo "$manifest_containers" | grep -qx "$c" && continue
                run docker rm -f "$c"
            done
        fi
        # Remove enola-related images
        images=$(docker images --filter "reference=enola-*" --format '{{.Repository}}:{{.Tag}}' 2>/dev/null || true)
        if [ -n "$images" ]; then
            for img in $images; do
                run docker rmi "$img"
            done
        fi
        # Use manifest to get exact network names, fall back to filter-based detection
        manifest_networks="$(manifest_get_all docker_network 2>/dev/null || true)"
        if [ -n "$manifest_networks" ]; then
            for net in $manifest_networks; do
                run docker network rm "$net" 2>/dev/null || true
            done
        fi
        # Fallback: filter-based network detection
        for net in $(docker network ls --filter "name=enola-" --format '{{.Name}}' 2>/dev/null || true); do
            echo "$manifest_networks" | grep -qx "$net" && continue
            run docker network rm "$net"
        done
        for net in $(docker network ls --filter "name=strapi-" --filter "name=wagtail-" --format '{{.Name}}' 2>/dev/null || true); do
            echo "$manifest_networks" | grep -qx "$net" && continue
            run docker network rm "$net"
        done
        # Remove enola Docker volumes (named volumes with enola prefix)
        for vol in $(docker volume ls --filter "name=enola-" --format '{{.Name}}' 2>/dev/null || true); do
            run docker volume rm "$vol"
        done
    else
        echo "  Docker not installed, skipping."
    fi
    echo ""
fi

# ─── Section: ufw ─────────────────────────────────────────────────────────────
if has_section ufw; then
    log "Section: ufw (removing enola-cli rules)"
    if command -v ufw &>/dev/null; then
        if $DRY_RUN; then
            echo "  [dry-run] Remove UFW rules from manifest and comment markers"
        else
            # Use manifest to get exact port numbers for removal
            ufw_ports="$(manifest_get_all ufw_rule 2>/dev/null || true)"
            if [ -n "$ufw_ports" ]; then
                for port in $ufw_ports; do
                    run ufw --force delete allow from 127.0.0.1 to any port "$port" proto tcp 2>/dev/null || true
                    run ufw --force delete allow "$port"/udp 2>/dev/null || true
                done
            fi
            # Also delete rules by comment marker (legacy/fallback)
            ufw_rules=$(ufw status numbered 2>/dev/null | grep '# enola-cli' | awk -F'[][]' '{print $2}' | sort -rn || true)
            if [ -n "$ufw_rules" ]; then
                for num in $ufw_rules; do
                    run ufw delete "$num"
                done
            fi
            if [ -z "$ufw_ports" ] && [ -z "$ufw_rules" ]; then
                echo "  No enola-cli UFW rules found."
            fi
        fi
    else
        echo "  UFW not installed, skipping."
    fi
    echo ""
fi

# ─── Section: data ────────────────────────────────────────────────────────────
if has_section data; then
    if $KEEP_DATA; then
        log "Section: data (--keep-data set, preserving all data)"
    else
        log "Section: data (removing /srv/enola-*, VPN configs, SSL certs)"
        for d in /srv/enola-*; do
            [ -d "$d" ] && run_rm "$d"
        done
        # Remove VPN config files from manifest
        vpn_configs="$(manifest_get_all vpn_config 2>/dev/null || true)"
        if [ -n "$vpn_configs" ]; then
            for iface in $vpn_configs; do
                [ -f "/etc/wireguard/${iface}.conf" ] && run_rm "/etc/wireguard/${iface}.conf"
            done
        fi
        # Remove SSL certs/keys from manifest
        ssl_certs="$(manifest_get_all ssl_cert 2>/dev/null || true)"
        ssl_keys="$(manifest_get_all ssl_key 2>/dev/null || true)"
        for cert in $ssl_certs; do
            [ -f "$cert" ] && run_rm "$cert"
        done
        for key in $ssl_keys; do
            [ -f "$key" ] && run_rm "$key"
        done
    fi
    echo ""
fi

# ─── Summary ──────────────────────────────────────────────────────────────────
echo "══════════════════════════════════════════════════════════════"
if $DRY_RUN; then
    echo "  DRY RUN complete. No changes were made."
    echo "  Run with --yes to execute removal."
else
    echo "  Uninstall complete."
    if $KEEP_DATA; then
        echo "  Data was preserved (--keep-data)."
    fi
fi
echo "══════════════════════════════════════════════════════════════"

# ─── Section: deps (third-party dependency removal) ───────────────────────────
# Only runs with --remove-deps. Only removes deps that Enola installed (per manifest).
# Deps the user had before Enola are NEVER touched.
if has_section deps; then
    if ! $REMOVE_DEPS; then
        if $DRY_RUN || [ -z "$ONLY_SECTIONS" ]; then
            # Show info in dry-run or full uninstall, but don't act
            if [ -f "$MANIFEST" ]; then
                deps_installed=$(manifest_get_all dep_installed || true)
                if [ -n "$deps_installed" ]; then
                    echo ""
                    log "Section: deps (info — use --remove-deps to uninstall)"
                    echo "  Enola installed these deps:"
                    for d in $deps_installed; do
                        echo "    - $d"
                    done
                    echo "  To remove them: add --remove-deps"
                    echo "  Deps you had before Enola are NOT listed and will NOT be removed."
                else
                    echo ""
                    log "Section: deps (info)"
                    echo "  No deps were installed by Enola (all were pre-existing)."
                fi
            else
                echo ""
                log "Section: deps (info — no manifest found)"
                echo "  Without a manifest, --remove-deps cannot determine which deps"
                echo "  Enola installed vs which you already had. Nothing will be removed."
            fi
        fi
    else
        echo ""
        log "Section: deps (--remove-deps active)"
        if [ ! -f "$MANIFEST" ]; then
            warn "No manifest found. Cannot determine which deps Enola installed."
            warn "Refusing to remove deps without manifest (safety: protect user's tools)."
            echo ""
        else
            deps_installed=$(manifest_get_all dep_installed || true)
            if [ -z "$deps_installed" ]; then
                echo "  No deps were installed by Enola. Nothing to remove."
            else
                echo "  Deps installed by Enola (will be removed):"
                for d in $deps_installed; do
                    echo "    - $d"
                done
                echo ""
                # Detect package manager
                if command -v apt-get &>/dev/null; then
                    PKG_MGR="apt-get"
                    PKG_REMOVE="apt-get remove -y"
                elif command -v dnf &>/dev/null; then
                    PKG_MGR="dnf"
                    PKG_REMOVE="dnf remove -y"
                elif command -v yum &>/dev/null; then
                    PKG_MGR="yum"
                    PKG_REMOVE="yum remove -y"
                elif command -v pacman &>/dev/null; then
                    PKG_MGR="pacman"
                    PKG_REMOVE="pacman -R --noconfirm"
                else
                    err "No package manager detected (apt/dnf/yum/pacman)."
                    err "Remove deps manually: $deps_installed"
                    exit 1
                fi
                echo "  Package manager: $PKG_MGR"
                echo ""
                for d in $deps_installed; do
                    if command -v "$d" >/dev/null 2>&1; then
                        run $PKG_REMOVE "$d"
                    else
                        echo "  $d already removed, skipping."
                    fi
                done
                ok "Deps installed by Enola have been removed."
                echo "  Deps you had before Enola were NOT touched."
            fi
        fi
    fi
fi
