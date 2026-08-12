# 🛠️ Scripts — Enola CLI

> Scripts del producto (instalación y operaciones).

## Estructura

```
scripts/
├── deploy/             ← Instalación del usuario
│   └── install.sh          — Instalador nativo Linux: descarga, SHA256, minisign
├── ops/                ← Operaciones en máquina real
│   ├── install_pqc_tls_stack.sh — Instalación de stack TLS post-cuántico
│   └── uninstall.sh        — Desinstalación de Enola CLI
├── dev/                ← Desarrollo y pruebas
│   ├── test_web_dashboard.sh      — Test del web dashboard
│   └── generate_third_party_licenses.sh — Genera THIRD_PARTY_LICENSES.txt
└── supply-chain-check.sh   — Verificación de cadena de suministro
```

## Uso rápido

```bash
# Instalación (usuario final)
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash

# Desinstalación
bash scripts/ops/uninstall.sh --yes
