#!/usr/bin/env bash
# demo_tor.sh — Flujo real de Tor para GIF de demostración.
# Acepta licencia, crea hidden service, lo lista, rota, y lo borra.
# No interactivo — usa sleep entre pasos para grabación con asciinema.
set -euo pipefail

CLI="enola-cli"
TOR_NAME="demo-tor"
DELAY=5

# Colores ANSI
CYAN='\033[1;36m'
GREEN='\033[1;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
RESET='\033[0m'

# Función para mostrar el comando en color antes de ejecutarlo
show_cmd() {
    echo -e "${CYAN}❯ ${BOLD}$*${RESET}"
    echo ""
    sleep 1
}

# Función para separar secciones
separator() {
    echo ""
    echo -e "${YELLOW}──────────────────────────────────────────────────────────${RESET}"
    echo ""
    sleep 1
}

# Cleanup previo (idempotente)
sudo -E "$CLI" tor remove "$TOR_NAME" --force 2>/dev/null || true

# Reset licencia para que aparezca el prompt
rm -f ~/.enola/license_accepted.json 2>/dev/null || true

clear
echo -e "${GREEN}${BOLD}🧅 Enola CLI — Tor Hidden Services Demo${RESET}"
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════${RESET}"
echo ""
sleep $DELAY

# ── Aceptación de licencia ─────────────────────────────────────────────
echo -e "${GREEN}${BOLD}▶ Paso 1: Aceptación de licencia${RESET}"
echo ""
sleep 1
echo -e "${CYAN}❯ ${BOLD}enola-cli doctor${RESET}"
echo ""
sleep 1
echo "acepto" | script -qc "enola-cli doctor" /dev/null 2>&1 | sed -n '1,22p'
echo ""
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 2: Listar servicios Tor (lista vacía)${RESET}"
echo ""
show_cmd "sudo enola-cli tor list"
sudo -E "$CLI" tor list
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 3: Crear hidden service${RESET}"
echo ""
show_cmd "sudo enola-cli tor create --name $TOR_NAME --service-type raw --virtual-port 80 --target-port 18080"
sudo -E "$CLI" tor create --name "$TOR_NAME" --service-type raw --virtual-port 80 --target-port 18080
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 4: Ver servicio creado${RESET}"
echo ""
show_cmd "sudo enola-cli tor list"
sudo -E "$CLI" tor list
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 5: Iniciar servicio${RESET}"
echo ""
show_cmd "sudo enola-cli tor start $TOR_NAME"
sudo -E "$CLI" tor start "$TOR_NAME"
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 6: Rotar identidad .onion${RESET}"
echo ""
show_cmd "sudo enola-cli tor rotate $TOR_NAME"
sudo -E "$CLI" tor rotate "$TOR_NAME"
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 7: Ver servicio con .onion activo${RESET}"
echo ""
show_cmd "sudo enola-cli tor list"
sudo -E "$CLI" tor list
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 8: Ver puertos en uso${RESET}"
echo ""
show_cmd "sudo enola-cli ports list"
sudo -E "$CLI" ports list
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 9: Detener servicio${RESET}"
echo ""
show_cmd "sudo enola-cli tor stop $TOR_NAME"
sudo -E "$CLI" tor stop "$TOR_NAME"
separator
sleep $DELAY

clear
echo -e "${GREEN}${BOLD}▶ Paso 10: Eliminar servicio (cleanup)${RESET}"
echo ""
show_cmd "sudo enola-cli tor remove $TOR_NAME --force"
sudo -E "$CLI" tor remove "$TOR_NAME" --force
echo ""
echo -e "${GREEN}${BOLD}✅ Demo completa — servicio creado y eliminado correctamente.${RESET}"
sleep $DELAY
