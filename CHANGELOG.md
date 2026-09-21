# Changelog

All notable changes to Enola CLI are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- **`update apply --binary` required signature verification** — applying a
  binary by explicit path no longer skips minisign verification. A manual
  `--binary` path is now treated as unverified and requires `--allow-unsigned`
  (or `ENOLA_ALLOW_UNSIGNED_UPDATE=1`). `download --yes` keeps the verified
  flow by passing `signature_verified` through.
- **Re-verify staged binary at apply time (anti-TOCTOU)** — `apply` now
  recomputes the SHA256 of the staged binary and aborts if it differs from the
  hash recorded by `update download`, closing the window where a writable
  `~/.enola/downloads/` could swap the binary between download and apply.
- **Forgejo admin password no longer passed in argv (CWE-214/522)** — the
  initial admin is now created with `forgejo admin user create --random-password`
  (the generated password is shown once and must be changed on first login)
  instead of `--password <plaintext>`, which was visible in `/proc/<pid>/cmdline`,
  `ps aux`, and Docker's exec log. `git user create --password` is now optional
  and, when omitted, is read interactively so it does not linger in shell history.

## [0.5.0-alpha] — 2026-09-20

### Added
- **`plan` dry-run command** — `plan <wp|git|tor> create` shows what a `create`
  would do (ports, Docker containers, filesystem paths, UFW rules, AppArmor
  profile, risk level) without executing anything. Text output or
  `--format json`.
- **Post-quantum `.pqsig` signatures (ML-DSA-65)** — verified by `install.sh`
  and by `update download`/`update apply`; the installed binary's signature is
  kept at `share/enola/cli.pqsig` for later re-verification with
  `enola-cli verify --pqsig`.
- **Configurable backup retention** — `maintenance backup-config [--max-backups N]`
  shows or persists the retention policy to `~/.enola/config.toml` (`[backup].max_backups`),
  and `maintenance backup --keep N` overrides it for a single run.
  Resolution order: `--keep` > `ENOLA_MAX_BACKUPS` > config.toml > default (5).
- **REST endpoints for `plan`** — the web dashboard now exposes
  `POST /api/plan/{wp,git,tor}/create`; each returns the serialized
  `ServicePlan` (same JSON shape as `--format json`). Dry-run only, 0 side
  effects.
- **Backup retention in the web GUI** — `GET`/`POST /api/maintenance/backup-config`
  expose the retention policy, `POST /api/maintenance/backup` accepts an
  optional `{keep}` body, and the Maintenance tab has inputs for both.
- **Console presets and help** — `/api/console/help` now lists `plan` and
  `web`, and the console preset dropdown includes the three `plan * create`
  commands.

### Changed
- **README repositioned** (EN + ES) as "Private Infrastructure for Linux".
- **WordPress backend port range** changed from 8000-9999 to 8080-9000,
  affecting the auto-assigned port in `wp create`.
- **Magnolia minimum RAM** lowered from 4 GB to 1.5 GB (warns if <1536 MB
  available; 4 GB remains the recommended value).

### Fixed
- **`plan git` showed a dedicated Docker network** (`enola_net_<name>`) that
  `git create` does not create — Forgejo runs on the default Docker bridge.
- **`plan git --ssl` and `plan tor --service-type`/`--ssl` were ignored** —
  the plan now reflects the auto-assigned HTTPS port, the self-signed
  certificate, the Nginx site and the real per-type Tor service paths
  (raw/web/static/files), and warns about flags that `create` would ignore.
- **`plan tor` advertised an AppArmor profile** (`enola-tor`, complain) that
  `tor create` never creates — only `wp create` and `git create` apply a
  per-service profile; the plan now shows `(none)`.
- **Backup rotation now sweeps legacy filenames** — `.tar.gz`/`.bak` backups
  with older timestamp formats were never listed nor rotated and accumulated
  indefinitely on existing installations.
- **`maintenance backup --keep 0`** now returns an error instead of silently
  clamping to 1, matching `backup-config --max-backups 0`.
- **Backup rotation now applies to archive backups** — `maintenance backup` and
  archive-based backups previously never rotated, so `*.tar.gz` archives
  accumulated unbounded in `/var/backups/enola-server/`.
- **System backup preserves absolute paths** — multi-path backups are archived
  relative to `/` so restores land each entry back at its original location.
- **Config resolution under sudo** — `~/.enola/config.toml` now resolves to the
  invoking user's home (`SUDO_USER`) instead of `/root/.enola/`, and files
  written as root are chowned back to the invoking user.

## [0.4.0-alpha] — 2026-09-13

### Added
- **SSH commands** — `ssh add-key` (atomic, deduplicated append to `~/.ssh/authorized_keys`) and `ssh deploy-hidden` (publish SSH only via `.onion`, no public port exposure).
- **Tor client auth rotation without server-side private key** — `tor auth rotate` now lets the client generate a new keypair and send only the public key to the operator; the private key never reaches the server.
- **Local/tarball mode in `install.sh`** — when a binary sits next to `install.sh` (extracted client tarball), the installer installs from it without downloading; `ENOLA_INSTALL_FORCE_DOWNLOAD=1` forces the remote path.
- **Anti-leak scanning of staged blobs** — the `pre-commit` hook now inspects the staged blob of every staged file (not the working tree), closing the gap where a leaked secret was staged but the working copy reverted.
- **Audit integration in git hooks** — `pre-push` and `release.sh` now run `run_all.sh --profile cli --strict`; the pre-push transcript is saved without ANSI and distinguishes exit 1 (errors) from exit 2 (warnings, which also block in `--strict`), naming the culpable check via `AUDIT_RESULT` sentinels.

### Fixed
- **aarch64 build** — `drop_privs.rs` used `vec![0i8; 4096]` for `getpwuid_r`/`getpwnam_r`; on aarch64 `libc::c_char` is `u8`, so the build advertised by `install.sh`/`release.sh` did not compile (E0308). Corrected to `vec![0 as libc::c_char; 4096]`.
- **Tor client auth input validation** — `client_name` was not sanitized, allowing path traversal via `authorized_clients/{client}.auth`. Whitelist `[A-Za-z0-9._-]` (no leading `.`, max 64 chars) now applied to `add`/`revoke`/`rotate`; pubkey validated as base32 (A-Z, 2-7); `rotate` is now strict (returns `NotFound` if the client does not exist and does not toggle service auth state).
- **Flaky CMS adapter tests** — removed `std::env::set_var` usage that caused race conditions in parallel test execution.

## [0.3.0-alpha] — 2026-09-05

### Added
- **VPN over Tor** — WireGuard interfaces can now route through Tor via a socat UDP↔TCP bridge, hiding the VPN endpoint behind an `.onion` service (`vpn create --tor`).
- **Atomic writes (TOCTOU remediation)** — secrets and config files (TLS cert/key, git credentials, CMS entrypoints, SSH keys, fwknop config) are now written atomically (temp file + rename, 0600) to close TOCTOU windows where a partial file could be read mid-write.
- **Docs↔code consistency audit** — 16 discrepancies between documentation and actual code behavior were corrected across 17 doc files.
- **Interactive prompt in `release_check.sh`** for release review.

### Changed
- **Privatized `scripts/dev/`** — internal dev tooling is now gitignored to keep the public repo focused on user-facing material.
- **Release pipeline** — `gh release create` no longer passes `--prerelease`, so alpha releases resolve as GitHub "Latest" and the documented one-liner install URL (`releases/latest/download/install.sh`) returns 200 instead of 404 while no stable release exists.

## [0.2.0-alpha] — 2026-08-30

### Added
- **Automatic version synchronization** — `sync_version.sh` keeps `Cargo.toml`, `llms.txt`, `llms-full.txt`, docs and installer metadata in lockstep; `sync_version.sh --check` validates coherence (now wired into audit check 19).
- **`enola-context-gen`** — auxiliary binary that generates the AI/session context snapshot consumed by `docs/dev/session_start.sh`.
- **`release_check.sh`** — pre-release sanity script that reviews the release artifacts before publishing.
- **Demo GIF** of the Tor service creation flow (asciinema → GIF) added to the README (EN + ES) and the GitHub Pages home.
- **DOC-SYNC markers** in top-level command docs to flag sections that must be updated when configuration sources change (per AGENTS.md §9).

### Changed
- **Release feed repaired** — the advisory feed was broken; the versioning system now generates a coherent feed signed with minisign.
- **`pre-commit` hook** now runs `cargo fmt --check`; remaining clippy lints silenced to keep `pre-push` green.

## [0.1.2-alpha] — 2026-08-29

### Added
- AI Usage policy block in `llms.txt` and `llms-full.txt`: indexing permitted but does not constitute a license grant; AI training for competing products subject to LICENSE §2.4; attribution requested.
- `README.md` and `README.es.md`: explicit AI readability clarification with bilingual parity.

### Changed
- License: removed fork prohibition clause (§2.3) to align with GitHub Terms of Service. GitHub TOS grants fork rights for public repos; the clause was unenforceable on this platform. Competing use remains prohibited by §2.4 (was §2.5). Renumbered §2.4→2.3, §2.5→2.4, §2.6→2.5 in both English and Spanish.
- License version bump triggers re-acceptance for existing users (build-time hash verification).

### Removed
- `MEGAPLAN.md` (internal planning document, not intended for public repo).

## [0.1.1-alpha] — 2026-08-22

### Added
- English-first `README.md` with language selector to `README.es.md`
- `llms.txt` + `llms-full.txt` for AI crawler indexing (llmstxt.org standard)
- `SECURITY.md`, `CHANGELOG.md`, `CONTRIBUTING.md` at repo root
- GitHub issue templates (`bug_report.yml` + `config.yml`) with security email redirect
- English translations of 6 key docs in `docs/en/` (quickstart, commands, concepts, faq, security-model, verify-downloads)
- GitHub Pages setup (`docs/index.md` + `docs/_config.yml`) with Jekyll theme cayman
- GitHub repo metadata: description, homepage URL, 18 topics
- Missing `git status` subcommand documentation in `docs/user/git/commands-git.md`

### Changed
- Version unified to 0.1.0-alpha (fixes inconsistent version references across docs and Cargo.toml)
- Rust badge updated: 1.75+ → 1.96 (matches rust-toolchain.toml)
- PQC keypair regenerated — new `pqc_sign.pub` (previous private key was lost; no prior releases to invalidate)

### Fixed
- `install.sh`: `BASE_URL` placeholder → GitHub Releases URL (`https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download`)
- `bump_version.sh`: regex now supports SemVer pre-release suffixes (`-alpha`, `-beta`, etc.)

## [0.1.0-alpha] — 2026-08-22

First public alpha release.

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

[Unreleased]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/compare/v0.4.0-alpha...HEAD
[0.4.0-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.4.0-alpha
[0.3.0-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.3.0-alpha
[0.2.0-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.2.0-alpha
[0.1.2-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.2-alpha
[0.1.1-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.1-alpha
[0.1.0-alpha]: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/tag/v0.1.0-alpha
