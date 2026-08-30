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
│   ├── release.sh                 — Release completo: build → firmas → feed → tag → GitHub
│   ├── release_check.sh           — Analiza commits sin releasear y recomienda bump (semver)
│   ├── sync_version.sh            — Propaga la versión de Cargo.toml a docs/README (NO toca el feed)
│   ├── test_web_dashboard.sh      — Test del web dashboard
│   └── generate_third_party_licenses.sh — Genera THIRD_PARTY_LICENSES.txt
├── git/                ← Hooks git (instalar con install-hooks.sh)
│   ├── pre-commit          — Verifica versión + anti-leak operator
│   ├── pre-push            — fmt + clippy + test + versión
│   └── install-hooks.sh    — Instala los hooks en .git/hooks/
└── supply-chain-check.sh   — Verificación de cadena de suministro
```

## Uso rápido

```bash
# Instalación (usuario final)
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash

# Desinstalación
bash scripts/ops/uninstall.sh --yes
```

## Análisis de release (release_check.sh)

`release_check.sh` analiza los commits desde el último tag y recomienda si
es necesario un bump de versión (semver estricto):

```bash
bash scripts/dev/release_check.sh
```

Qué hace:
1. Detecta commits sin releasear (`git log v{VERSION}..HEAD`).
2. Categoriza por prefijo conventional-commit (`feat:`→minor, `fix:`→patch,
   `docs:`/`chore:`/`style:`→sin bump, `BREAKING CHANGE`→major).
3. Verifica completitud del release actual (tag + GitHub release + feed).
4. Emite recomendación con la próxima versión sugerida.

## Proceso de release

`release.sh` orquesta el ciclo completo. `sync_version.sh` solo propaga la
cadena de versión en docs/README; el feed (`feed/advisories.json`) está
firmado con minisign y **solo** lo actualiza y re-firma `release.sh`
(vía `sync_version.sh --release-feed`).

```bash
# Release de la versión actual de Cargo.toml
bash scripts/dev/release.sh

# Bump + release
bash scripts/dev/release.sh --bump 0.1.3-alpha

# Preparar y firmar artefactos SIN publicar (no commit/tag/push/gh)
bash scripts/dev/release.sh --dry-run
```

Pasos que ejecuta (pide la passphrase minisign y la de GPG interactivamente):

1. `sync_version.sh` (o `--bump X.Y.Z`) — propaga la versión.
2. Build reproducible (`CARGO_TARGET_DIR` neutro + `--remap-path-prefix`) y
   chequeo de filtración de paths personales.
3. Artefactos: tarball client (binario + `install.sh` + claves públicas),
   binario crudo para `install.sh`, `.sha256`, SBOM SPDX y fichero `LATEST`.
4. `sync_version.sh --release-feed` — actualiza `latest` + `published_at`.
5. Firmas: minisign (tarball, binario, feed) + ML-DSA-65 (`enola-sign-pqc`).
6. Verificación de TODAS las firmas antes de publicar.
7. `git commit -S` + `git tag -s vX.Y.Z` + push.
8. `gh release create` con todos los assets.

Requisitos: `gh` autenticado, `minisign` (clave en `~/.minisign/enola.key`),
clave PQC en `~/.enola/pqc_signing.key` y `cargo-sbom`.
