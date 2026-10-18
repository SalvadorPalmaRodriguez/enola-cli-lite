# Changelog

All notable changes to Enola CLI are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Breaking
- **Web API destructive confirmation gate (SEC-005)** — destructive endpoints
  (`/api/tor/{name}/rotate`, `/api/tor/{name}/remove`, `/api/git/{name}/delete`,
  `/api/git/user/delete`, `/api/wp/{name}/delete`, `/api/cms/{kind}/{name}/delete`,
  `/api/files/{name}/delete`, `/api/vpn/{name}/delete`, `/api/vpn/peer/remove`,
  `/api/maintenance/cleanup`, `/api/uninstall`) now require `{"confirm":"<name>"}`
  in the request body and return HTTP 409 without it. All destructive actions in
  the web UI (`app.js`) prompt with a confirmation modal before sending `confirm`
  (C-05).
- **Non-TTY preflight gate (SEC-009)** — when running without a TTY as root,
  dependency preflight no longer runs `apt-get install -y` implicitly unless
  `ENOLA_AUTO_SETUP=1` is set. `/api/console/run` propagates `ENOLA_MCP_CHILD=1`
  to prevent implicit package installation from the web console.
- **`[update]` trust anchors under `sudo` / `euid 0` (SEC-003, C-06, C-08)** —
  when running as root, `[update]` (`feed_url`, `signature_url`, `minisign_pubkey`)
  is read exclusively from root-owned `/etc/enola/config.toml` (`!group/other-writable`);
  `~/.enola/config.toml` and non-root-owned `trusted_minisign_keys.json` are
  ignored. Environment overrides `ENOLA_UPDATE_FEED_URL`,
  `ENOLA_UPDATE_SIGNATURE_URL` and `ENOLA_UPDATE_MINISIGN_PUBKEY` are ignored
  under `euid 0` unless `ENOLA_UPDATE_ENV_ANCHORS=1` is also set.
- **`git user create` default password mode (SEC-006, C-02, C-03, C-04)** —
  omitting `--password` on the CLI now defaults to `--random-password` +
  `--must-change-password=true` (matching the web API) so no password traverses
  process `argv` or `/proc/*/cmdline`; explicit `--password` remains supported for
  scripts with a stderr warning. Explicit `--admin-user`/`--admin-pass` flags are
  now verified against the bcrypt hash in `.enola-admin-creds` when present.

### Security
- **PQC TLS installer staging (SEC-001)** — shared `pqc_tls::stage_installer()`
  writes the embedded script into a private `0700` tempdir with `O_EXCL` (`0700`)
   across both CLI and web SSE routes, eliminating the fixed `/tmp` path and
  redundant `sudo` invocation.
- **Least privilege for read-only subprocesses (SEC-002, SEC-004)** — `cargo test`,
  `cargo clean`, `curl`, `ssh`, `ps`, `which`, `openssl` and `nginx -V` drop
  privileges to `SUDO_UID`/`SUDO_GID` (including supplementary groups via
  `setgroups`) via `command_as_invoking_user`, enforced by audit check
  `24_drop_privs.sh`.
- **Web dashboard token persistence (F-06 / N-07)** — the web token is written
  atomically (`0600`, chowned to the invoking user) to `~/.enola/web-token`;
  stderr prints only the file path so `journald` no longer records the bearer
  secret.
- **Orphan Tor termination via `pidfd` (SEC-011 / C-01)** —
  `kill_orphan_tor_process` pins the candidate process with `pidfd_open`,
  re-verifies `/proc/<pid>/comm == "tor"` and signals via `pidfd_send_signal`
  (with `ENOSYS` fallback to `libc::kill`), eliminating `Command::new("kill")`
  and the PID-reuse race window.
- **VPN/Tor socat systemd unit hardening (SEC-007 / N-09)** — adds
  `DynamicUser=yes`, `NoNewPrivileges=true`, `ProtectSystem=strict`,
  `ProtectHome=true`, `PrivateTmp=true` and conditional
  `AmbientCapabilities=CAP_NET_BIND_SERVICE` only for ports `< 1024`.
- **MCP argv redaction and audit ordering (F-12, F-14 / N-11)** — `enola_run_command`
  redacts sensitive flag values in returned `argv`, and logs `requested` after
  argument validation (recording `invalid` on validation errors).

### Removed
- Unreferenced scaffolding modules (`app_deployer`, `git/client` + `ports/git`,
  `document_extractor` + `ports/document`, `safe_args`) removed (SEC-008 / N-10 / F-26).

## [0.8.0-alpha] — 2026-10-15

### Changed
- **MCP security hardening (MCP-009)** — audit-driven reinforcement of the
  stdio MCP server:
  - New `Destructive` tool class: builtins `wp_publish`, `wp_hide`,
    `tor_create`, `tor_remove`, `files_create`, and — via
    `enola_run_command` — every `* delete|hide`, `tor create|edit|rotate`,
    `tor auth add|generate|disable|revoke|rotate`, `vpn create`,
    `vpn peer add|add-pubkey|remove`, `* publish`, `files create`,
    `firewall setup|allow|deny`, `apparmor setup|mode`,
    `git create|registration`, `git user create|delete`,
    `maintenance backup|cleanup|disable-checks|ssh-harden-pqc` and
    `update download`. Destructive calls require `--allow-write` +
    allowlist + `force: true` (where the leaf declares it) **and** a
    two-step confirmation: the first call returns a `confirm_token`
    (single-use, 300 s TTL, bound to tool + exact arguments, wiped on
    restart); the second call with that token executes.
  - `enola_run_command` never dispatches the `mcp`, `ssh`, `uninstall`,
    `web` or `setup` families, nor the `update apply`, `test run` and
    `test benchmark` paths — under any policy or allowlist. The `--binary`
    and `--allow-unsigned` flags of `update` are not expressible, and any
    flag value starting with `-` is rejected.
  - Dispatched subprocesses run with a cleared environment plus an
    explicit allowlist — the server's `ENOLA_*` vars are not inherited.
  - `update verify-feed` reclassified as mutating (its `--source` reads
    arbitrary local files / fetches arbitrary http(s) URLs).
  - `try_run: true` simulates any mutating/destructive tool with the same
    validation and policy but zero side effects (`wp_create`/`tor_create`
    return the real planner output; `enola_run_command` returns the exact
    `argv`).
  - Strict input schemas: every tool declares `additionalProperties: false`
    plus `maxLength`/`minLength`/`minimum`/`maximum`/`maxItems` bounds.
  - `files_create` restricts `share_path` to `/srv/enola-files/**` (stricter
    than the interactive CLI).
  - Audit log gains states (`rejected`/`simulated`/`confirmation_requested`/
    `authorized`/`ok`/`error`), per-operation ids and outcome truncation;
    the file is created `0600` and its directory `0700`.
  - At euid 0, the `[mcp]` config section is read only from root-owned
    `/etc/enola/config.toml`.
  - Allowlist entries for leaf commands use the bare family name
    (`doctor`, not `doctor:`).
  - **Migration**: MCP clients must implement the two-step flow for
    destructive tools — a bare `force: true` call now returns
    `confirmation_required` instead of executing.
- **`update apply` re-verifies the staged binary's minisign signature**
  against the `.minisig` persisted by `update download` (the
  `signature_verified` flag in `last_download.json` is informational only).
  When running as root, the metadata and downloads dir must be root-owned
  and not group/world-writable. Older staged downloads without a `.minisig`
  are rejected — re-run `update download`.
- **`update download`** now persists `<binary>.minisig` next to the staged
  binary so `apply` can re-verify it.
- **`uninstall`** only accepts absolute `uninstall.sh` paths
  (`/usr/local/share/enola/` or derived from the binary location) — a
  `scripts/ops/uninstall.sh` relative to the cwd is no longer a candidate.
- **`setup --pqc-tls`** writes its installer to a private 0700 tempdir
  instead of a fixed `/tmp` path.
- **Backups** (`maintenance backup`, per-service backups, pre-restore
  copies) now create the backup directory `0700` and artifacts `0600`.

## [0.7.0-alpha] — 2026-10-13

### Added
- **MCP server (`enola-cli mcp`)** — Model Context Protocol server over stdio
  (JSON-RPC 2.0, NDJSON, protocolVersion `2026-07-28`) exposing Enola to AI
  agents. Methods: `initialize`, `notifications/initialized`, `ping`,
  `tools/list`, `tools/call`, `resources/list`, `resources/read`. stdout carries
  only JSON-RPC; logs and diagnostics go to stderr.
- **21 MCP tools** — 13 read-only (`config_show`, `diag_nginx`, `diag_summary`,
  `diag_tor`, `doctor`, `plan_tor_create`, `plan_wp_create`, `ports_list`,
  `tor_list`, `update_check`, `verify_onion`, `wp_list`, `wp_status`), 6
  mutating (`files_create`, `tor_create`, `tor_remove`, `wp_create`, `wp_hide`,
  `wp_publish` — `tor_remove` and `wp_hide` require explicit `"force": true`)
  and 2 meta-tools (`enola_list_commands`, `enola_run_command`).
- **MCP write policy** — read-only by default; mutating tools require
  `--allow-write` and can be limited with `--allow-tools` (empty allowlist +
  `--allow-write` = all mutating tools enabled). Mutating calls are written to
  an audit log with secret redaction.
- **MCP resources** — the 36 embedded user documents are readable as
  `enola://docs/<path>` resources.
- **MCP configuration** — flags `--allow-write`, `--allow-tools`,
  `--max-output-bytes`, `--audit-log`, `--request-timeout-secs`; env vars
  `ENOLA_MCP_ALLOW_WRITE`, `ENOLA_MCP_ALLOWED_TOOLS`,
  `ENOLA_MCP_MAX_OUTPUT_BYTES`, `ENOLA_MCP_AUDIT_LOG`,
  `ENOLA_MCP_REQUEST_TIMEOUT_SECS`; `[mcp]` section of
  `~/.enola/config.toml`. Priority: CLI flag > env var > config file > safe
  defaults.
- **`diag onion`** — `diag onion <address>.onion [--format json]` probes a
  `.onion` service through Tor's SOCKS5h proxy and returns structured
  reachability (status, latency, title, failure reason:
  `timeout`/`proxy_unavailable`/`site_unreachable`).

### Removed
- **`serde_yaml` dependency** — dead dependency; it pulled in `unsafe-libyaml`
  (RUSTSEC-2024-0449).

### Security
- **`quinn-proto` 0.11.18** — fixes RUSTSEC-2026-0338, -0339, -0340, -0341 and
  -0342 (transitive via `reqwest` HTTP/3).
- **`chacha20` 0.10.2 and `der` 0.8.2** — replaces yanked versions.
- **`base64` unified on 0.22** — removes the duplicated 0.21/0.22 entries.

## [0.6.0-alpha] — 2026-10-07

### Added
- **`commit-msg` hook that rejects AI-attribution trailers** — commit messages
  carrying `Co-Authored-By:`, `Generated with`, or known bot emails are rejected
  mechanically. The author and signer of every commit is the human, only.

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

### Security
- **Atomic config writes (T4)** — `set_key_in_path` writes `config.toml` via
  `atomic_secret_file` (O_EXCL + 0600 + fsync + rename), never
  `fs::write` + `set_permissions`.
- **Constant-time dashboard token comparison (T5)** — the bearer token is
  checked byte-by-byte (XOR), not with `==`.
- **`.onion` requests always use `socks5h://` (T6)** — the latent HTTP client
  builder now enforces the SOCKS5-with-DNS rule too.
- **Minisign key rotation fix (T7)** — the active key is the most recently
  persisted one, never the first.
- **Archive extraction hardened against zip-slip (T8)** — member paths are
  pre-validated (no absolute paths, no `..` segments).
- **`.env` injection guards (T9)** — key/value reject control characters and
  `=` in keys.
- **Feed version validated before download-path interpolation (T10)** —
  defense in depth.
- **`install_pqc_tls_stack.sh` drops `eval "$@"` (T11)** — argv is executed
  plainly and snippets run in an isolated subshell.
- **Exponential backoff against dashboard brute force (T12)** — `100ms·2^n`,
  capped at 2s; a successful auth resets the counter.
- **`git publish` guards (T13)** — blocks while the Forgejo wizard is active
  and warns if open registration before exposing on `.onion`.

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
