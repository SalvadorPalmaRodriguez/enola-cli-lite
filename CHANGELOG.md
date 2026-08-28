# Changelog

All notable changes to Enola CLI are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.2-alpha] — 2026-08-29

### Changed
- License: removed fork prohibition clause (§2.3) to align with GitHub Terms of Service. GitHub TOS grants fork rights for public repos; the clause was unenforceable on this platform. Competing use remains prohibited by §2.4 (was §2.5). Renumbered §2.4→2.3, §2.5→2.4, §2.6→2.5 in both English and Spanish.
- License version bump triggers re-acceptance for existing users (build-time hash verification).

### Added
- AI Usage policy block in `llms.txt` and `llms-full.txt`: indexing permitted but does not constitute a license grant; AI training for competing products subject to LICENSE §2.4; attribution requested.
- `README.md` and `README.es.md`: explicit AI readability clarification with bilingual parity.

### Removed
- `MEGAPLAN.md` (internal planning document, not intended for public repo).

## [0.1.1-alpha] — 2026-08-22

### Changed
- Version unified to 0.1.0-alpha (fixes inconsistent version references across docs and Cargo.toml)
- Rust badge updated: 1.75+ → 1.96 (matches rust-toolchain.toml)
- PQC keypair regenerated — new `pqc_sign.pub` (previous private key was lost; no prior releases to invalidate)

### Added
- English-first `README.md` with language selector to `README.es.md`
- `llms.txt` + `llms-full.txt` for AI crawler indexing (llmstxt.org standard)
- `SECURITY.md`, `CHANGELOG.md`, `CONTRIBUTING.md` at repo root
- GitHub issue templates (`bug_report.yml` + `config.yml`) with security email redirect
- English translations of 6 key docs in `docs/en/` (quickstart, commands, concepts, faq, security-model, verify-downloads)
- GitHub Pages setup (`docs/index.md` + `docs/_config.yml`) with Jekyll theme cayman
- GitHub repo metadata: description, homepage URL, 18 topics
- Missing `git status` subcommand documentation in `docs/user/git/commands-git.md`

### Fixed
- `install.sh`: `BASE_URL` placeholder → GitHub Releases URL (`https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download`)
- `bump_version.sh`: regex now supports SemVer pre-release suffixes (`-alpha`, `-beta`, etc.)

## [0.1.0-alpha] — 2026-08

First public alpha release (Phase 1: standalone, no authentication).

### Added

- **Tor hidden services** — create/manage `.onion` services (web, static, files, raw TCP) with automatic Nginx wiring, identity rotation and x25519 client authorization (incl. key rotation).
- **Git hosting** — Forgejo servers with HTTP/SSH-over-Tor, user management, self-registration control and pipeline watcher.
- **Six CMS modules** — WordPress, Drupal, Ghost, Magnolia, Strapi, Wagtail: Dockerized, localhost-only, one-command Tor publishing.
- **File sharing** — anonymous Nginx autoindex shares over `.onion` with optional HTTPS (TLSv1.3) and Tor client auth.
- **WireGuard VPN** — interfaces and peer management, optional preshared keys (post-quantum resistance), UFW sync.
- **Firewall & sandboxing** — UFW setup with DOCKER-USER chain, AppArmor base and per-service profiles (complain/enforce).
- **Post-quantum security** — ML-DSA-65 (FIPS 204) release signatures with offline verification (`enola-cli verify`), optional ML-KEM hybrid TLS stack (`setup --pqc-tls`), SSH PQC hardening (`maintenance ssh-harden-pqc`).
- **Updates** — signed advisory feed, `update check/download/apply` with SHA256 + minisign verification and stable exit codes.
- **Operations** — maintenance (backup, cleanup, health checks), diagnostics, system tests, log viewer, port inspector.
- **UX** — embedded offline docs (`enola-cli docs`), local web dashboard (`enola-cli web`), Docker quick reference, centralized config with `config-show`/`config-validate`, JSON output mode.
- **Install/uninstall** — verified installer script and clean sectioned uninstaller (dry-run by default).

[0.1.2-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.2-alpha
[0.1.1-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.1-alpha
[0.1.0-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.0-alpha
