# Changelog

All notable changes to Enola CLI are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Breaking
- **Web dashboard: no custom or unsigned updates, no test runs, confirmations for setup/update** — `POST /api/update/apply` and `/api/update/download` no longer accept `binary` or `allow_unsigned` (requests that send them get `400`); the dashboard only applies the update downloaded and signature-verified by `update download`. Installing a custom binary or skipping the signature check is CLI-only (`sudo enola-cli update apply --binary <path> [--allow-unsigned]`). `update apply`, `update download` (except `dry_run`) and `setup` now require `"confirm": "update"` / `"confirm": "setup"` in the body (`409` otherwise); the dashboard asks before sending them. `POST /api/test/run`, `/api/test/benchmark` and `/api/test/clean` are removed (they are developer tools: use `sudo enola-cli test run`); `GET /api/test/list` and `/api/test/results` remain.
- **Web dashboard: PQC TLS progress stream moved** — `GET /api/setup/pqc-tls` (which started the 30-minute installer on a GET and never worked from the browser, because `EventSource` cannot send the token) is replaced by `POST /api/setup {"pqc_tls": true, "confirm": "setup"}`, which starts the installer and returns a single-use `pqc_tls_ticket` (valid 60 s), and `GET /api/setup/pqc-tls/events?ticket=…`, which only streams its output. Only one installer runs at a time; posting again returns a new ticket for the running job.

- **As root, `ENOLA_*` environment variables are ignored (except three)** — see *Security*. If you ran `sudo -E` with variables such as `ENOLA_AI_ENDPOINT`, `ENOLA_UPDATE_FEED_URL`/`ENOLA_UPDATE_MINISIGN_PUBKEY`, `ENOLA_TOR_SOCKS_PROXY`, `ENOLA_MINISIGN_BIN`, `ENOLA_MAX_BACKUPS` or `ENOLA_DOCKER_DEFAULT_*`, use the matching flag or `/etc/enola/config.toml` instead (`ENOLA_DOCKER_DEFAULT_*` has no replacement: root uses the default container limits). `ENOLA_WEB_TOKEN` and `ENOLA_ALLOW_UNSIGNED_UPDATE` were removed: the dashboard token is always random (read it from `~/.enola/web-token`) and only `--allow-unsigned` skips update signature checks. `[ai].endpoint`/`[ai].tor_proxy` and `[distribution]` in `~/.enola/config.toml` are ignored as root.

- **New WordPress, Git and CMS instances use supported, pinned images** — see *Security*. New instances get Forgejo 15 LTS (was 9.0), MariaDB 11.4 LTS for WordPress (was 10.6), WordPress 6.9.4/PHP 8.3 (was `latest`), Drupal 11.4 (was 10), Ghost 6 (was 5) and Strapi built on Node 22 (was 20, image tag `enola/strapi:5.49.0-node22`, rebuilt automatically). **Existing instances are not migrated** (their data may not be compatible): `sudo enola-cli doctor --security` lists them under `image-eol`; back up, export from the application and recreate. Git servers now use their own Docker network `enola_net_git_<name>`.
- **MCP: `--allow-write` now requires an allowlist** — an empty allowlist used to enable every mutating tool. `enola-cli mcp --allow-write` now refuses to start unless you pass `--allow-tools <tools>` (or `[mcp].allowed_tools`); to really enable every mutating tool use the explicit `--allow-all-mutating` (or `[mcp].allow_all_mutating = true`), which prints a warning. `enola-cli ask` is unchanged (its safeguard is the interactive `[s/N]`).
- **File shares: strict CSP, HTML pages lose inline scripts and styles** — every file share (with and without `--ssl`) now sends `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; frame-ancestors 'none'; …` (the plain share used to allow inline styles). A shared `.html` file no longer runs inline `<script>`/`style=` code; external `.js`/`.css` files from the same share still work. Existing shares keep their old Nginx configuration: delete and recreate them (`sudo enola-cli files delete <name>` + `files create`).
- **Services published with v0.9.0 are no longer recognized** — `hide`, `delete`, `publish`, `tor ssl` and `tor auth <domain> <instance>` used to detect a service with its v0.9.0 name (`<n>`, `proxy_<n>`, `proxy_<cms>-<n>`, `fileserver-<n>`) and answer with the command to remove it; `files` also still listed, edited and deleted `fileserver-<n>` shares and `<n>.crt` certificates, `ai list` showed `proxy_ai-<n>`/`ai-<n>` addresses and Tor directories named `hidden_service_<n>` were listed. All of that is gone: these commands only know the current names and say the service does not exist. Remove an old service by its real name (`sudo enola-cli tor list`, then `sudo enola-cli tor remove <name>`) and publish it again (the `.onion` address changes).
- **`<domain> hide` also removes the Nginx site and the TLS certificate** — `wp`, `drupal`, `ghost`, `magnolia`, `strapi` and `wagtail hide` only removed the Tor service and left the site and `/etc/nginx/ssl/<service>.{crt,key}` behind (`git hide` already removed the site). Every way of unpublishing (`tor remove`, `hide`, `delete`, MCP `tor_remove`/`wp_hide`) now removes the Tor service, its site and its certificate. `publish --ssl` creates a new certificate.
- **`[distribution].minisign_pubkey_url` removed** — it was never used (it suggested a configurable trust anchor that did not exist). Update signatures are verified with the embedded key (rotated only through the signed feed) or `[update].minisign_pubkey` in `/etc/enola/config.toml`.

- **MCP and web console no longer run commands that print a generated secret** — `tor auth generate` (prints the client's private key), `vpn peer add` (prints a WireGuard configuration with `PrivateKey`) and `git user create` (prints Forgejo's random password) are refused by `enola_run_command`, `ask` and `/api/console/run`: the secret would end up in the agent's context. Run them from the terminal or with the Tor, VPN and Git buttons of the dashboard (unchanged).
- **AI providers: `http://` endpoints only towards this machine** — `ai provider set --endpoint` accepted any `http://` URL, so the API key could travel unencrypted. The endpoint must now be `https://`, or `http://` to `127.0.0.1`, `::1` or `localhost` (e.g. a local gateway); URLs with `user@` are refused. The rule is also applied when the provider is used, so an edited `~/.enola/providers.env` cannot bypass it.
- **Forgejo admin hash moved to `/etc/enola/git/<name>.creds`** — it lived in `/srv/enola-git/<name>/.enola-admin-creds`, inside the volume the container mounts as `/data`. Servers created in CLI mode before this release no longer find it: pass `--admin-user`/`--admin-pass` to `git user …`, or move it once with `sudo install -d -m 700 /etc/enola/git && sudo mv /srv/enola-git/<name>/.enola-admin-creds /etc/enola/git/<name>.creds`. `git delete` removes it, and `uninstall.sh` removes the directory together with the data.
- **VPN interface names must start with a letter or digit** — a name starting with `-` (e.g. `-rf`) was accepted and passed to `wg`/`wg-quick`/`ip`, where it reads as an option.
- **`files fix-perms` only works on its own share** — it read the folder from `sites-available/fileserver_<name>` *or* `sites-available/<name>` (the site of a `tor create static`/`raw` service with that name) and ran `chown -R root:www-data` + `chmod -R 750` on it as root. It now only uses the share's own `fileserver_<name>` site and refuses system paths (`/etc`, `/usr`, `/var/lib`…), other Enola services' data (`/srv/enola-git`…), a whole home directory and other users' homes.

### Security
- **Forgejo and Ghost run as their own system users** — both ran as uid 1000 (Forgejo through `USER_UID=1000`, Ghost as the image's `node` user), which on Ubuntu/Debian is the first human account, usually with `sudo`: the data in `/srv/enola-git`/`/srv/enola-ghost` belonged to that user and a container escape landed in it. New servers and blogs run as `enola-git` / `enola-ghost` (created with `useradd --system`, no shell, no home), which own their data; `git edit` and `ghost edit` move existing ones to the new user. If the user cannot be created, `create` fails instead of falling back to uid 1000.
- **RAG and fine-tuning only read what you could read** — `ai rag ingest`/`sync` and `ai finetune prepare`/`validate` read files as root, so under `sudo` they could index or copy other users' files, `/srv` service data or `/var/backups`. They now check, before each file, that the user who ran `sudo` could read it (owner/group/other bits of the file and every parent folder; symlinks are resolved first) and skip or refuse the rest; `/var/backups` is added to the paths that can never be a source.
- **Web API rejects invalid names in the URL** — the 53 dashboard endpoints that take a name in the path (`/wp/{name}/…`, `/cms/{type}/{name}/…`, `/vpn/{interface}/…`…) passed it on to commands run as root; they now validate it first and answer `400` (defence in depth: each command also validated it). `/api/docs/…` refuses parts that start with `-`.
- **`ssh add-key --comment` cannot add extra lines** — the comment was written to `authorized_keys` without checks: a line break added a second key with any options. Comments with control characters or longer than 256 bytes are refused.
- **TLS private keys are checked to be `0600`** — after generating a self-signed certificate, the key was `chmod`-ed by running `chmod` and ignoring the result. It is now set and verified on the file; generation fails if the key is not `0600`.
- **Web console history no longer stores secrets** — the dashboard kept the last 50 console commands in the browser's `localStorage` (unencrypted, no expiry), including values of `--password`, `--admin-pass`, `--token`, `--api-key`…; they are now stored as `***`, and histories saved by earlier versions are cleaned when the dashboard loads.
- **`providers.env` is written and read without following symlinks, owned by you** — under `sudo`, the file holding your AI provider keys was created root-owned in your `~/.enola` and read by path (a `providers.env` symlink was followed as root). It now goes through the same validated `~/.enola` handling as `config.toml`: no symlinks, owned by the user who ran `sudo`. `uninstall.sh` finds that user's home from the passwd database instead of assuming `/home/<user>`.
- **`ask` no longer suggests `curl … | sh`** — when Ollama is unreachable, the help now recommends `sudo enola-cli ai create` and links the official install guide instead of piping a downloaded script into a shell.
- **HTTPS file shares had no CSP and served sensitive files** — the `--ssl` variant of a file share (both its HTTP and HTTPS blocks) sent no `Content-Security-Policy` and did not block `.env`, `.git`, `.sql`, `.pem`, `.key`, `.crt`, `.der`, `.p12`, `.pfx` files (SEC-018 only covered shares without `--ssl`). All file share blocks now share the same headers and blocked locations.
- **Web dashboard: Host check, security headers and rotating token** — requests whose `Host` is not `127.0.0.1`, `localhost` or `[::1]` now get `421` (blocks DNS-rebinding pages from reading the dashboard; the port is not checked, so SSH tunnels keep working). Every response carries a strict `Content-Security-Policy` (`default-src 'self'`, `script-src 'self'`, `style-src 'self'`, `object-src 'none'`, `frame-ancestors 'none'`; no `'unsafe-inline'`: the dashboard no longer uses inline `onclick` handlers or inline styles, and service names are passed to actions as data, so a hostile name cannot run code), `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and `Cache-Control: no-store` on `/api`. The token now rotates every 12 hours (`[web].token_ttl_secs`) and is rewritten to `~/.enola/web-token` without restarting; the browser asks for it again when it changes, and **System → Rotate Token** (`POST /api/token/rotate`) rotates it on demand.
- **`ask` with a remote LLM asks before sending host data** — with an endpoint that is not on this machine (and no `--instance`), every read-only tool that can reveal host details (`.onion` addresses, logs, configuration, ports, VPN, service status…) now asks `[s/N]` first, because its output goes to that server; only the embedded docs, the command catalog and `plan` run without asking. `--yes` approves them; without a TTY they are refused and the model is told why.
- **VPN peer names and IPs are validated** — the peer name is written into WireGuard configuration files, where `wg-quick` runs `PostUp` lines as root, and the peer IP becomes its `AllowedIPs`: a name with a line break could inject directives and an IP like `0.0.0.0/0` gave a peer the whole tunnel. Names must now be 1–32 letters, digits, `-` or `_`, and the IP a plain IPv4 inside the server subnet (not the network, broadcast or server address), in the CLI, the web dashboard and MCP alike.
- **Update feed: signed but old feeds are refused** — a mirror or attacker able to serve the feed could replay an older, legitimately signed feed to hide new advisories or versions. Enola now remembers the newest `published_at` accepted and refuses a feed older than that; it also warns when the feed is more than 90 days old.
- **Manifest and uninstaller cannot be tricked into deleting other paths** — the install manifest (`key|value` lines) did not reject line breaks, and `uninstall.sh` removed paths read from it as root. Entries with line breaks, NUL or `|` in the key are now rejected, and the uninstaller only removes paths under the locations Enola manages (any other path is skipped with a warning).
- **Fine-tuning runs unprivileged and never runs model repository code** — the training container ran as root inside the container; it now always runs as the unprivileged UID 65532 (set by Enola when starting it, so an image built earlier is covered too; the Dockerfile also declares it), can only write its own output directory, and the bundled `convert_hf_to_gguf.py` loads tokenizers with `trust_remote_code=False` (was `True`). UID 65532 is used instead of 1000 because on most hosts 1000 is the invoking user, who could otherwise tamper with the model before `ai finetune register` imports it as root.
- **Web: enabling HTTPS on a published service needs confirmation** — `POST /api/tor/ssl/{name}` now requires `{"confirm": "<name>"}` (as MCP already did); the dashboard asks before sending it.
- **Publishable services: pinned, supported images with minimal capabilities** — several images were out of support or rolling: Forgejo 9.0 (no fixes since 2025-01-16), MariaDB 10.6 (since 2026-07-06), `wordpress:latest`, Drupal 10 (ends 2026-12-09), Ghost 5 and Strapi on Node 20 (since 2026-04-30). **Ghost 5 with SQLite — every Ghost instance created so far — exposes password hashes through the public API (CVE-2026-53949, fixed only in Ghost ≥ 6.21.2): hide a published Ghost (`ghost hide`) until you recreate it.** All images now come from one table with a fixed version, digest pin (`image@sha256:…`) and known end-of-support date; containers start with `cap_drop: ALL` plus only the capabilities each image needs (checked by starting each image: HTTP, Git over SSH and clean stop), also when a port edit recreates the container (`git|drupal|ghost edit` previously recreated it without `no-new-privileges`, resource limits or — for Git — the capabilities sshd needs). The AppArmor profiles for Git and WordPress now allow exactly those capabilities (they denied binding ports 80/22 when loaded). `doctor --security` adds `image-eol` (containers on out-of-support images) and recommends Docker `userns-remap`. The upstream Wagtail (2021) and Magnolia (2020) images are unmaintained and no maintained alternative exists: they are kept, pinned by digest, reported by `doctor`, and `wagtail|magnolia create` and `publish` (CLI, web, MCP) now print a warning that the software inside receives no security fixes.
- **Web console follows the same command policy as the MCP server** — with the dashboard token (readable by any process of the user in `~/.enola/web-token`), `/api/console/run` ran any CLI command as root: commands the MCP server refuses (`update apply --binary … --allow-unsigned`, `web`, `mcp`, `setup`, `uninstall`, `ssh`, `test run`), destructive commands without the confirmation the dedicated endpoints require, with the server's full environment (including `ENOLA_WEB_TOKEN`) and with no limit on run time. Both now go through one shared decision point: refused commands, flags (`--binary`, `--allow-unsigned`) and global URL overrides (`--tor-socks`, `--binary-base-url`, `--web-url`, `--docs-url`) get `403`; destructive commands get `409` until resent with `"confirm": "<resource>"` (and `--force` when the command has it); input the gate cannot parse unambiguously (combined short flags, `--`) gets `400`. Commands run with a minimal environment and without automatic dependency installation, `timeout_secs` must be 1–1800 (a timed-out command is now killed instead of left running as root) and at most 4 run at once (`429`). The dashboard greys out refused commands and asks for confirmation on `409`.
- **Static `.onion` sites listen on loopback only** — `tor create static` generated an Nginx site with `listen <port>;`, reachable on every network interface instead of only by Tor (`127.0.0.1`). New sites now use `listen 127.0.0.1:<port>;`. Sites created before this release are not changed automatically: `sudo enola-cli doctor --security` lists them (`nginx-listen`) and prints the exact command to fix them in place without changing the `.onion` address.
- **VPN-over-Tor bridge listens on loopback only** — the `socat` unit created by `vpn tor enable` listened on every network interface (`TCP-LISTEN:<port>`); it now binds to `127.0.0.1`, the only address Tor uses. Existing `enola-vpn-bridge-<if>` units are not changed automatically: `sudo enola-cli doctor --security` lists them (`vpn-bridge-listen`) and prints the fix, which needs `systemctl daemon-reload` and a restart of the unit (re-running `vpn tor enable` alone does not restart an active unit).
- **Lock directory can no longer be pre-planted by other users** — port and shared-artifact locks lived in `/var/lock/enola` (under the world-writable `/run/lock`), the directory was accepted without checking its owner or mode, a `/tmp/enola-locks` fallback was used when it could not be created, and lockfiles were opened following symlinks. Locks now live in `/run/enola/locks` (root-owned `0755`); the directory and any `ENOLA_PORT_LOCK_DIR`/`ENOLA_LOCK_DIR` override must be a real directory owned by the current user and not writable by group/others, lockfiles are opened without following symlinks, and the `/tmp` fallback is gone (without root and without an override, commands that reserve ports fail with a clear error). Restart a running `enola-cli web` after updating so it uses the new location.
- **PQC TLS installer: pinned sources and current versions** — `setup --pqc-tls` (CLI and web) ran as root and could take the OpenSSL/nginx tarballs from a fixed `/tmp` cache, compared OpenSSL against a checksum downloaded from the same place, verified nginx with the packages key (not the release key) and then fell back to fetching any key from a public keyserver. The installer now pins the SHA-256 of both tarballs and the nginx release-signer fingerprints in the script, has no `/tmp` cache and no keyserver fallback, and checks that its working directory is root-owned `0700`. It also moves to the current releases **OpenSSL 3.5.9** and **nginx 1.30.6** (3.5.7 and 1.28.3 are superseded), and an existing stack is rebuilt unless it is exactly those versions.
- **`ssh add-key` no longer follows symlinks in the target user's home** — when run as root for another user (`--user`), the command wrote `authorized_keys` by path inside that user's `~/.ssh`, so a `~/.ssh` (or `authorized_keys`) replaced by a symbolic link sent the write elsewhere. The directory is now opened once without following symlinks and must belong to the target user (or root); if it has more permissions than `0700` (e.g. `775`, which also makes `sshd` reject the keys) they are removed and reported — permissions are only ever removed, never added — and the read, lock and atomic write all happen inside it. `authorized_keys` now keeps the target user as owner (it used to become root-owned after each `add-key`).
- **Root no longer follows symlinks in `~/.enola`** — under `sudo`, `config set` and `web` wrote `config.toml` / `web-token` in the invoking user's `~/.enola` and then ran `chown` *by path*, which follows a symbolic link swapped in between; `config set` also read `config.toml` through a symlink. Both files are now read and written inside the validated `~/.enola` directory without following symlinks, and ownership is given to the invoking user on the file descriptor before the atomic rename. If `~/.enola` belongs to the user but has more permissions than `0700` (common with umask `002`), they are removed and reported (it holds `providers.env` and `web-token`); a `~/.enola` that is a symlink or belongs to someone else is refused.
- **`update download`/`apply`: staging out of the user's home and install from verified bytes** — as root the staged binary, its signatures, `last_download.json` and the update cache were kept in `$HOME/.enola` (the invoking user's home under `sudo -E`), the ownership check followed symlinks, and the binary was verified by path and then moved into place by path; with `apply --binary <path>` the installed `enola-cli` even kept the owner of the supplied file. As root, update state now lives in `/var/lib/enola/updates` (root-owned `0700`, created and checked without following symlinks); `apply` reads the binary once, verifies SHA-256 and minisign on those bytes and installs exactly those bytes as a new root-owned file (the supplied or staged file is never moved). A download made with an earlier version is not picked up: run `sudo enola-cli update download` again. Note: the first update to this release is still applied by the previously installed binary.
- **Inherited environment is no longer trusted when running as root** — under `sudo -E` the root process kept the user's environment, and each sensitive variable was handled one by one (4 of about 60 `ENOLA_*`): `ENOLA_WEB_TOKEN` fixed the root dashboard token to a value the attacker knew, `ENOLA_ALLOW_UNSIGNED_UPDATE` disabled update signature checks, `ENOLA_MCP_AUDIT_LOG` made root change permissions of and append to any path, `ENOLA_AI_ENDPOINT` chose which LLM drives the tools `ask` runs as root, `ENOLA_APPARMOR_ENABLED_PATH` disabled AppArmor profiles, `PATH` decided which binaries root ran. As root the CLI now keeps only `ENOLA_ACCEPT_LICENSE`, `ENOLA_MCP_CHILD` and `ENOLA_AUTO_SETUP`, drops every other `ENOLA_*` with one warning listing the names, and resets `PATH` to `/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`. Command-line flags are always honoured. Security settings in the user's `~/.enola/config.toml` (`[ai].endpoint`, `[ai].tor_proxy`, `[distribution]`, besides the already root-only `[update]`, `[mcp]` and non-loopback `[http].tor_socks_proxy`) are ignored as root with a warning and read only from root-owned `/etc/enola/config.toml`; convenience keys (model, timeouts, backups) still come from the user config. As root the MCP audit log must be directly in `/var/log/enola/` or `/var/lib/enola/` (`mcp` refuses to start otherwise) and its directory is no longer `chmod`-ed by path. `enola-cli ask` prints the effective LLM endpoint and where it came from, and warns if it is not on this machine. Test-only overrides (`ENOLA_SKIP_ROOT_CHECK`, `ENOLA_APPARMOR_ENABLED_PATH`, `ENOLA_MANIFEST_PATH`) no longer exist in release builds.
- **Service data directories are private** — `/srv/enola-<service>` (git, wordpress, drupal, ghost, magnolia, strapi, wagtail, ai) was created `0755`, so any local user could read the secrets the containers write inside with `0644` (WordPress `wp-config.php` salts, Forgejo `app.ini` `SECRET_KEY`, `ghost.db`, Open WebUI data). These base directories are now `0700 root:root` when a service is created; existing ones are fixed automatically the next time a command for that service runs as root (and when `enola-cli web` starts), and `doctor --security` reports any that are still open (`srv-data-perms`). Containers are unaffected (bind mounts do not traverse host parents); `/srv/enola-files` keeps its permissions because Nginx serves from it. On a shared machine, consider rotating the WordPress salts and the Forgejo `SECRET_KEY` of existing instances.

### Fixed
- **`vpn peer add` always failed** — `vpn create` wrote the server address as `Address = 10.8.0.1/32` instead of using the subnet prefix, so the peer check ("Peer IP '10.8.0.2' is outside the VPN subnet 10.8.0.1/32") refused every peer, and the kernel had no route to them either. The server now gets `10.8.0.1/24` (the prefix of `--subnet`). Interfaces created with earlier versions keep `/32`: delete and recreate them (`vpn delete <name> --force` + `vpn create`).
- **`files delete` could delete another service's certificate** — besides its own `fileserver_<name>.{crt,key}`, it deleted `/etc/nginx/ssl/<name>.{crt,key}`, which can belong to another service. It now deletes only its own.
- **Deleting a service left its TLS private key behind** — `tor remove`, `delete` and `hide` removed the Tor service and the Nginx site but not `/etc/nginx/ssl/<service>.{crt,key}`. The certificate is now removed with the service. Certificates left behind by earlier versions are not cleaned up.
- **`git delete` left the `.onion` address online** — it removed a Tor service named `<name>` (which could belong to another service created with `tor create`) instead of `git_<name>`, and from the web dashboard it did not remove any Tor service. It now removes `git_<name>`, its site and its certificate, from the CLI and the dashboard.
- **MCP `tor_remove` left the Nginx site** — it removed only the Tor service; it now also removes the site and the certificate.
- **Web dashboard: Delete, Edit and Fix Perms on file shares** — `GET /api/files` returned the Tor service name (`fileserver_<name>`) while `/api/files/{name}/…` expects the share name, so the three buttons failed with "invalid service name". The list now returns the share name (`<name>`).
- **Web console accepts `--format`/`--verbose` after the subcommand** — the command gate introduced for the console rejected `tor list --format json` with `400 unknown flag` although the CLI accepts these global flags in any position (they only worked before the command, `--format json tor list`). They are now accepted after the subcommand too, without changing the veto or the destructive classification of the command; a subcommand flag with the same name (e.g. `ai finetune prepare --format`) still takes precedence, as in the CLI.
- **Dashboard token file always matches the active token** — a manual `POST /api/token/rotate` overlapping with the automatic rotation (or two requests arriving after the token expired) could write one token to `~/.enola/web-token` and keep a different one in memory, locking the operator out until the next rotation or a restart. Persisting and activating the new token now happen as one step.

## [0.10.0-alpha] — 2026-10-24

### Breaking
- **Canonical Tor service names `<domain>_<name>` (services published with v0.9.0 must be republished)** — publishing now names every hidden service and its certificate after the domain (the Nginx site is `proxy_<domain>_<n>`): `git_<n>`, `wp_<n>`, `drupal_<n>`, `ghost_<n>`, `magnolia_<n>`, `strapi_<n>`, `wagtail_<n>`, `fileserver_<n>` and `ai_<n>` (`tor create web` keeps `proxy_<n>`). Services published with v0.9.0 keep their old names — `git publish` → `<n>`, `wp publish` → `proxy_<n>`, `drupal|ghost|magnolia|strapi|wagtail publish` → `proxy_<cms>-<n>` (`files create` is unchanged) — and are **not managed automatically**: `hide`, `tor auth <domain> <instance>` and `tor ssl` no longer find them. They are never ignored silently: if only the old service exists, these commands fail with the exact command to run, and publishing a CMS whose old service still exists is refused instead of creating a second `.onion` (for `wp`/`git`, whose old names are also used by `tor create`, it publishes and warns). To migrate: `sudo enola-cli tor remove <old-name>` and publish again. **The `.onion` address changes.**

### Added
- **`--ssl` and `--auth` on every publish command** — `git publish`, `wp|drupal|ghost|magnolia|strapi|wagtail publish`, `files create` and `ai publish` accept the same two flags: HTTPS with a self-signed certificate (`.onion:443`) and Tor client authorization. All of them go through one shared publication flow (certificate → Nginx → Tor → client auth), which replaces seven copies of that sequence; `ai publish` reports `https_port` and `auth_enabled` in its JSON result.
- **`tor ssl <service>`** — adds HTTPS to a service already published over HTTP: canonical certificate (`/etc/nginx/ssl/<service>.{crt,key}`), Nginx site rewritten as HTTP+HTTPS and port 443 added to the same `.onion`. It fails with a clear error if the service already has SSL or does not exist (it never creates a new hidden service). Also available in the web GUI (`POST /api/tor/ssl/{name}`) and as the MCP tool `tor_ssl` (destructive, two-step confirmation).
- **`tor auth <domain> <instance>`** — every `tor auth` subcommand also accepts the domain and the instance (`tor auth enable ai dev` is `ai_dev`); a single argument keeps working as before.
- **Third-party copyright notices (legal compliance)** — `THIRD_PARTY_LICENSES.txt` now reproduces the real copyright notice and license text shipped by each Rust crate included in the binary (297 crates, x86_64 + aarch64 union), instead of generic SPDX templates; development-only crates are no longer listed. Adds the MPL-2.0 source notice for `option-ext`, the verbatim text of the vendored `convert_hf_to_gguf.py` (llama.cpp, MIT) and, for crates whose package carries no license file, the text copied from their upstream repository at the published commit. New `enola-cli license --third-party` prints it; the notices are embedded in the binary (also covering the standalone binary download) and included in the release archive.
- **Local AI instances `enola-cli ai` (Fase 10 — Ollama + Open WebUI over `ContainerPort`)** — new `enola-cli ai create|list|status|start|stop|delete|publish|hide|logs` command family rewritten over `ContainerPort` (`BollardDockerAdapter`, zero `docker compose` / `Command::new("docker")`). Enforces exclusive `127.0.0.1` port bindings, `flock` port reservation (`acquire_port_lock`), full rollback on partial creation failures, calibrated R20 container limits (`memory_limit >= 8 GiB`, `pids_limit >= 4096`), NVIDIA GPU passthrough via `EnolaHardwareProbe` with graceful CPU fallback (R21), `.onion` publication via Nginx + Tor with optional SOCKS5h verification (`--verify`), and `logs view ai-<name>` integration.
- **AI fine-tuning `enola-cli ai finetune` (Fase 14 / 14.5)** — `ai finetune prepare|validate|start|status|register|wizard`: JSONL datasets (`alpaca|chatml|sharegpt|text`) → canonical ChatML with deterministic split; pre-flight schema/VRAM checks via `HardwareProbePort`; `start` resolves the base model from the instance GGUF library, pre-validates the GGUF header and mounts GGUF + HF tokenizer (host-side on-demand download, `--tokenizer`) into an isolated training container (`enola-ai-<n>-finetune-<job>`, no ports, `network_mode: none`, reproducible image from `assets/finetune/Dockerfile` with a digest-pinned base and exact package versions, ro-rootfs, GPU, `--low-memory`) running the embedded `train_lora.py`. `register <n> <job_id>` imports the exported `.gguf` into the library (SHA-256 + header) and registers it in Ollama as `<name>-ft`. MCP classes: `status` ReadOnly, `prepare|validate|register` Mutating, `start` Destructive (`--force` + `confirm_token`), `wizard` in `FORBIDDEN_PATHS`.
- **Local benchmark and BYOK providers (Fase 13)** — `ai benchmark <n>` measures tokens/s, latency and model size from Ollama; `ai provider list|status|set|remove|test` + `ask --provider` for optional cloud providers (`openai|anthropic|grok|groq|openrouter`). Disabled by default, explicit consent required (TTY `[s/N]` or `--accept-risks`, fails closed), keys in `~/.enola/providers.env` (`0600`, never logged/argv), optional SOCKS5h routing via `--tor`.
- **RAG index `enola-cli ai rag` (Fase 12)** — `ai rag status|ingest|bind|sync|config`: per-instance ChromaDB/Qdrant container lazily provisioned over `ContainerPort` on `127.0.0.1`, boundary-aware chunking presets and Ollama `/api/embed` embeddings; `bind` registers `git|filesystem|wordpress` sources (Destructive, canonicalized, system dirs rejected); `ai rag ingest-docs <n>` indexes the documentation embedded in the binary into the `enola_docs` collection, queried by the new `docs_lookup` MCP tool.
- **GGUF model library `enola-cli ai model` (Fase 11)** — `ai model list|download|import|add|delete|validate|refresh`: per-instance library with HuggingFace/URL download (streaming, `Range` resume, SHA-256), registration via `/api/create`, explicit `--tor` routing; `delete` is Destructive (`--force`).
- **Local AI assistant `enola-cli ask` (Fase 9 — Ollama)** — new `enola-cli ask <prompt> [--model <m>] [--endpoint <url>] [--instance <name>] [--no-exec] [--yes]` command backed by local Ollama (`/api/chat` with tools, `/api/tags`, `/api/pull` over `infrastructure::http` with automatic `.onion` SOCKS5h routing). Shares the MCP `ToolRegistry`, strips MCP-only meta-properties (`confirm_token`, `try_run`) from schemas sent to local models (R11), executes `ReadOnly` tools directly, simulations via `--no-exec` (`try_run`), and enforces an interactive TTY `[s/N]` gate with secret-redacted arguments (`sanitize_args` / `redact_argv`) for `Mutating` and `Destructive` operations (issuing internal `confirm_token`s after human approval so destructive `enola_run_command` calls complete in a single LLM turn, R18). Added `[ai]` configuration section (`endpoint`, `model`, `timeout_secs` + `ENOLA_AI_*`) and added `"ask"` to `FORBIDDEN_FAMILIES` (R19).

### Security
- **BYOK provider keys written without a permissions window** — `~/.enola/providers.env` is now written through the shared atomic secret writer (`O_EXCL` temporary created `0600` in the same syscall, random name, atomic `rename`) instead of `write` + late `chmod`; the API key is never readable by other local users, a planted symlink on a predictable temporary name is no longer followed, and a newly created `~/.enola` directory is `0700`.
- **Dependency advisories** — `rustls-webpki` 0.103.16 and `webpki-roots` 1.0.10 (RUSTSEC-2026-0382, RUSTSEC-2026-0381), `ctutils` 0.4.5 (RUSTSEC-2026-0385) and `tokio` 1.54.1 (RUSTSEC-2026-0386).

### Fixed
- **`ask --yes` no longer approves destructive operations** — `--yes` now authorizes mutating tools only; destructive tools (e.g. exposing a service, deleting data) always require the interactive `[s/N]` confirmation and fail closed without a TTY even with `--yes`, because the model chooses the operation and tool output (RAG documents, logs, third-party pages) could steer it.
- **Registering GGUF models in Ollama works** — `ai model download|import` (unless `--no-register`), `ai model add <file>.gguf` and `ai finetune register` sent the old `modelfile` field to `/api/create`, which the pinned Ollama (0.40.6) rejects (`neither 'from' or 'files' was specified`), so no library model could be registered. The file is now uploaded as an Ollama blob (streamed, SHA-256 digest, skipped if already present) and created with `files`.
- **`ai finetune start` training no longer crashes on GPU** — with the CUDA build of torch 2.14 some operations are JIT-compiled with Triton at runtime, which writes a cache to `$HOME` and needs a C compiler; in the read-only, compiler-less training container the job failed after a few seconds. The container now sets `TORCH_DISABLE_NATIVE_JIT=1`, so torch uses its prebuilt CUDA kernels.
- **`ai finetune start` training container really has no network** — it was created with Docker's default bridge (outbound access) despite being documented as network-less; it now runs with `network_mode: none`.
- **Pinned images for reproducible builds** — `assets/finetune/Dockerfile` pins `python:3.11-slim` by digest and every pip package, transitive dependencies included (`requirements.lock`, plus `requirements-cu126.lock` for the GPU build, applied with `--no-deps` and checked with `pip check`), to an exact version (`torch==2.14.1`; the GPU build example now uses `TORCH_INDEX=.../cu126`, as torch 2.14.1 is not published for cu124); RAG vector stores use `qdrant/qdrant:v1.20.0` and a digest-pinned `chromadb/chroma` (1.4.4) instead of `latest`. `gguf` is pinned to `0.16.2`, the first published release with the API the embedded GGUF converter (llama.cpp b4879) uses — with `0.16.0` the converter failed on import and no `.gguf` was exported — and `sentencepiece` to `0.2.0`, the newest version it accepts, so the image builds.
- **`ai model download` never corrupts or deletes an existing model** — downloads go to `<file>.part` and resume only from that partial file; SHA-256 (if given) and the full GGUF header (prefix and metadata, the same read `ai model validate` does) are verified before an atomic rename, and a downloaded file whose metadata cannot be read is never left in the library. Previously an existing library file with the same name (possibly a different model) was resumed with `Range`, producing a corrupt model that still passed the header check, and a `--sha256` mismatch deleted the previous valid model.
- **`ai rag` no longer falls back to hash embeddings silently** — if the instance's Ollama returns no embeddings, `ai rag ingest|sync` fails without indexing anything; an interactive terminal offers retry (up to 3 times), hash or cancel, and scripts/MCP fail closed. Hash embeddings (non-semantic) require the explicit new flag `--embedding-fallback hash`. Each collection records the embedding method it was built with (`rag_config.json` → `embedding_kinds`) and mixing methods or embedding models is refused; queries (`docs_lookup`) use the recorded method and fail instead of hashing a query against a semantic index.
- **Qdrant RAG backend works with Ollama embeddings** — Qdrant collections were always created with a fixed 384-dimension vector size (the hash fallback size), so indexing with Ollama embeddings (`nomic-embed-text` = 768) was always rejected and only worked through the silent hash fallback. The collection is now created on the first ingestion with the real embedding dimension, and a dimension mismatch on an existing collection fails with a clear "reindex from scratch" error.
- **`docs_lookup` works and never fails for missing docs** — the tool queried an `enola_docs` collection that no command filled, so with a vector store provisioned it always failed. `ai rag ingest-docs` now fills it from the embedded documentation; `docs_lookup` only uses instances where it is indexed and otherwise returns empty results with a note (`ai rag ingest-docs` / `docs_search`) instead of an error. It never searches the user's own RAG documents.
- **`ai finetune start` accepted the wrong GGUF quantizations** — the supported list compared the header's `general.file_type` against GGML tensor-type codes, so the common Q4_K_M and Q8_0 files were rejected while Q5_0 was accepted. The check now uses the real `general.file_type` values (F32, F16, Q4_0, Q8_0, Q4_K_S/M, Q5_K_S/M, Q6_K).
- **GGUF models with large vocabularies are accepted** — `ai model download|validate` and `ai finetune start` read only the first 4 MiB of a GGUF and parsed the whole metadata block, so models whose tokenizer vocabulary does not fit (e.g. Qwen2.5, 5.66 MiB of metadata) failed with "Header GGUF truncado". Parsing now stops once `general.architecture` and `general.file_type` are known, and up to 32 MiB are read when those keys come after the vocabulary.
- **`tor create --ssl` works for web services again** — the HTTP+HTTPS hidden service was registered as `proxy_<name>`, and that full name went through the service-name check (which rejects `_`), so `tor create <name> --ssl` always failed after configuring Nginx. The base name is now validated and the `proxy_` prefix added afterwards, as in the plain web path, so `tor start|stop|remove <name>` find the service.
- **`wp delete` no longer removes other services** — after deleting the site it tried several Tor names in turn and removed the first one found, so a `tor create web` service with the same name (`proxy_<name>`) could be deleted, `.onion` keys included. It now removes only `wp_<name>`; if only a service from v0.9.0 remains, it prints how to remove it instead of deleting it.
- **No post-quantum claim in `files create --ssl` help** — the help said the HTTPS listener was "PQC-safe", which TLS 1.3 with a regular certificate is not. A test now checks that no command's help claims PQC safety.
- **Certificates of services with the same name no longer collide** — `git create --ssl dev` and `files create --ssl dev` both wrote `/etc/nginx/ssl/dev.crt`; each certificate now uses the canonical service name (`git_dev`, `fileserver_dev`, …).
- **`ai delete` no longer removes containers of other instances** — containers are matched by their exact suffix (`ollama`, `webui`, `chromadb`, `qdrant`, `finetune-ft<id>`) instead of the `enola-ai-<name>-` prefix, so deleting `dev` leaves an instance named `dev-test` untouched.
- **Deterministic service-name resolution** — `tor start|stop|remove|rotate|edit` and the web dashboard no longer pick whichever Tor service happens to be listed first when a short name matches several services (e.g. `dev` with both `git_dev` and `wp_dev`): an exact name always wins, a single match is used, and several matches now fail with the candidates listed in type order so the full name can be given (web API: HTTP 409). `ai list`/`ai status` show the `.onion` of the name `ai publish` deploys even if Tor lists another form first, and `ask --instance <n>` resolves the managed container `enola-ai-<n>-ollama` before legacy names; without `--instance`, auto-discovery is ordered (managed → legacy → `piloto-ollama`, then alphabetically) instead of following `docker ps` order.

## [0.9.0-alpha] — 2026-10-18

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
- **MCP stdio `FD_CLOEXEC` and `config-show` root parity (F-NEW-3, F-NEW-4)** —
  `StdoutGuard`/`StdioTransport` duplicate the protocol descriptor with
  `F_DUPFD_CLOEXEC` so child processes never inherit the JSON-RPC pipe;
  `config-show` / `doctor` apply the same `euid 0` rules for `[update]` as
  `update_checker` (`load_section_root_owned` and `ENOLA_UPDATE_ENV_ANCHORS=1`).

### Changed
- **Hexagonal boundary cleanup (F-NEW-2)** — moved the web dashboard driving
  adapter (`web_api`, `web_server`, `web_errors`) from `src/application/` to
  `src/adapters/web/`, and injected `DoctorPorts` (`DependencyPort`,
  `PortCheckerPort`, `HardwareProbePort`) into `application::system_doctor`,
  leaving zero adapter/CLI imports in `src/application/` production code.

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
