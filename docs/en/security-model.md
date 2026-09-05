> **User document:** `docs/en/security-model.md`
> **Version:** 3.1 (2026-08-04)
> **Audience:** end users, security researchers, journalists.
> **Spanish original:** [`docs/user/general/SECURITY.md`](../user/general/SECURITY.md)

# 🛡️ Security Policy — Enola CLI

---

## What is Enola CLI?

Enola CLI is a command-line tool for self-hosting services
(WordPress, Forgejo/Git, CMS) behind Tor on your own hardware,
without relying on cloud providers. It is distributed as a single binary
(Linux x86_64) with digital signatures.

---

## What we protect by design

### 1. Your username and local paths

The binary is compiled with a hardened profile that:

- Strips **all debug symbols** (`strip = "symbols"`).
- Rewrites developer absolute paths (`--remap-path-prefix`):
  no `/home/<dev>/.cargo/registry/...` or `/home/<dev>/.rustup/...`
  appears in the distributed binary.
- Compiles with fat LTO + `opt-level = "z"` + `panic = "abort"` to
  make reverse engineering more expensive.
- For publicly distributed releases, compilation happens inside
  `Dockerfile.build` with `WORKDIR /build` so **no build host paths**
  are embedded (including those from the `openssl-sys` crate with
  the `vendored` feature).

**Reproducible verification** (any user with a Linux shell):

```bash
BIN=enola-cli   # path to your downloaded binary
strings "$BIN" | grep -c '/home/'        # expected: 0 (or only internal AppArmor profile paths)
strings "$BIN" | grep -c '\.cargo/'      # expected: 0
nm "$BIN" 2>&1 | head -1                 # expected: "no symbols"
file "$BIN" | grep -q stripped && echo OK
```

### 2. Your credentials and configuration

- **Sensitive configuration** in `~/.enola/` with `0600` permissions.
- **Configuration file** at `~/.enola/config.toml` with `0600` permissions.
- In code: any value with suffix `_token`, `_secret`, `_password`,
  `_key` is displayed as `[REDACTED]` in `enola-cli config-show` and in logs.
- Credentials are **never** sent through logs or tracing — mandatory pattern in
  `infrastructure::http` and all adapters.

### 2.1 Forgejo admin credentials

- Forgejo admin credentials are stored as **bcrypt hashes** (cost 12),
  never as plaintext, in `/srv/enola-git/<name>/.enola-admin-creds`.
- When running `git user list/create/delete` commands without `--admin-pass`,
  the CLI **prompts for the password interactively** and verifies it against the hash.
- Compatibility: if a legacy file exists (`ADMIN_PASS=plaintext`),
  it is used directly without prompt (transparent migration).
- Every Forgejo admin is created with `--must-change-password=true`,
  forcing password rotation on first login.

### 2.2 Secrets in CMS containers

- Strapi and Wagtail don't support the `_FILE` pattern for reading secrets from
  files. Enola CLI uses an **entrypoint wrapper** that:
  1. Mounts secrets as Docker secrets in `/run/secrets/` (read-only).
  2. Reads and exports them as environment variables inside the container.
  3. Executes `exec "$@"` to hand control to the original command.
- This prevents secrets from appearing as plaintext in `docker inspect`,
  which exposes all container environment variables.
- Secret files are generated with `0600` permissions in
  `/srv/enola-strapi/<name>/secrets/` and `/srv/enola-wagtail/<name>/secrets/`.

### 3. Your network and file paths

- **Docker bind to `127.0.0.1` ALWAYS** — containers never expose
  ports to `0.0.0.0`. Only Tor (via hidden services) or nginx (via localhost)
  can talk to them.
- **UFW + DOCKER-USER**: the `enola-cli firewall setup` command applies a
  `deny incoming` policy and configures the `DOCKER-USER` chain to prevent
  a compromised container from bypassing the firewall.
- **AppArmor**: mandatory profiles for service containers
  (Forgejo, WordPress) when the kernel supports it.
- **Tor v3 hidden services**: `.onion` URLs are ephemeral addresses and
  are not published in DNS. Privacy comes from Tor, not from us.
- **Path validation**: file share paths
  are validated with lexical canonicalization (collapses `.` and `..`) and,
  if the path already exists on disk, with `canonicalize` to resolve symlinks.
  This prevents path traversal and TOCTOU attacks in `/srv/enola-files/`.

### 4. Binary integrity and trust anchors

- **`minisign` digital signature** (Ed25519 key): each binary and each Docker
  file has its `.minisig`. Verify with the public key in `minisign.pub`.
- **Post-quantum ML-DSA-65 signature** (FIPS 204): in addition to minisign, we offer
  dual signatures ready for the post-quantum era.
- **`enola-cli verify <file>`**: verifies the ML-DSA-65 post-quantum signature
  (embedded public key) and, if a sibling `.sha256` exists, the integrity,
  using only the distributed binary. No `enola-sign-pqc` or network required.
- **Minisign validation**: if
  `ENOLA_MINISIGN_BIN` is set to an absolute path, the CLI validates that the binary
  exists, is executable, and is actually minisign (identity check via `<bin> -V`).
  If it fails, it warns and falls back to `minisign` from PATH.
- **Installer trust anchor**: if
  `ENOLA_INSTALL_PUBKEY` is set to a value different from the default,
  the installer emits a visible warning. `ENOLA_INSTALL_STRICT_PUBKEY=1`
  aborts installation (CI mode).
- **Self-integrity check**: the binary verifies its own hash on startup
  (`check_self_integrity()` in `build.rs` + runtime).
- **Runtime anti-debug**: on startup, the binary:
  - Calls `prctl(PR_SET_DUMPABLE, 0)` → blocks **core dumps** and **ptrace attach**.
    If the process crashes, no `core` file with process memory is written —
    passphrases and keys in RAM don't leak to disk.
  - Reads `/proc/self/status` and aborts with exit code 2 if it detects `TracerPid != 0`
    (gdb, strace, ltrace, rr, ... already attached at startup).

  Verify it yourself:
  ```bash
  strace ./enola-cli --version
  # → "exited with 2" — the binary detects strace and aborts.
  ./enola-cli --version
  # → "enola 0.2.0-alpha" — normal operation.
  ```
  > **Honest note**: this layer **does not prevent** reverse engineering. An
  > attacker can recompile Rust or patch the binary on disk. It's a
  > defense-in-depth layer for the common case (opportunistic attacker
  > trying to dump memory from a running process).
- More details: `docs/user/verify-downloads.md`.

---

## Threat model — what does NOT protect you

Be honest with yourself about what this tool can and cannot do.

| Threat | Does Enola CLI protect you? |
|---------|-------------------------------|
| Attacker who controls your machine with root privileges | **No.** If they have root, they can read everything. |
| Attacker who controls your ISP | Partially. If you use Tor for everything, yes. If you use clear-net, no. |
| Attacker who breaks Tor (NSA-level) | No. That depends on Tor, not on us. |
| Reverse engineering of the binary | **Not completely.** Hardening makes it more expensive but doesn't prevent it. An attacker can patch the binary on their own machine. This is accepted by design: the model is "good faith + auditing", not DRM. |
| Credential theft from your disk | If your disk is encrypted and `~/.enola/` is 0600, yes. If your system is compromised at root level, no. |
| MITM on network connections | Yes, through TLS. The CLI forces `rustls-tls` on all connections. |
| Quantum adversary (Q-day) | Partially. Releases have dual ML-DSA signatures (FIPS 204). SSH hardened with post-quantum KEX (`ssh-harden-pqc`). PQC TLS available in Nginx via `setup --pqc-tls` (hybrid KEX X25519MLKEM768). Tor circuit is not yet PQC (requires arti). Full plan: `docs/user/general/quantum-security.md`. |

---

## Reporting vulnerabilities — Coordinated Disclosure

> **Important:** The use of Enola CLI is subject to a proprietary license
> that requires **coordinated disclosure**. By using the software, you accept
> these terms. See [LICENSE](../../LICENSE) §5.

If you find a security vulnerability:

1. **DO NOT open a public issue** on Forgejo/GitHub.
2. **DO NOT publish or share** the vulnerability on any channel (forums, social
   media, blogs, chats) until it has been remediated.
3. Report the vulnerability **within 72 hours** of discovery through one
   of these private channels:
   - **Tor**: the project's `.onion` URL is published on the official
     website (`docs/user/verify-downloads.md` lists the canonical domain).
   - **PGP-encrypted email** to the maintainer:

**Maintainer PGP key** (published 2026-04-28):
```
Fingerprint: 6101 0A8C D06A 8E27 563D C9CC 7C2D E4F2 DC40 C81B
Email:       salvadorpalmarodriguez@gmail.com
Type:        RSA 4096 + subkey RSA 4096
Expires:     2030-04-28 (4 years, renewable)
Keyserver:   hkps://keys.openpgp.org
```
Download the public key from the keyserver:
```bash
gpg --keyserver hkps://keys.openpgp.org --recv-keys 61010A8CD06A8E27563DC9CC7C2DE4F2DC40C81B
gpg --fingerprint 61010A8CD06A8E27563DC9CC7C2DE4F2DC40C81B  # Verify it matches
```

4. The maintainer will acknowledge receipt within a reasonable time (target: 7 days)
   and work with you to resolve the vulnerability.
5. **Only after** the vulnerability has been remediated and with written consent
   from the maintainer may it be publicly disclosed.
6. We give credit in release notes unless you request anonymity.

**Bug bounty**: there is currently no formal reward program
(self-hosted, small budget). We are open to coordinated disclosures
and acknowledge contributions publicly.

---

## Audits and dependencies

- **`cargo audit`** and **`cargo deny`** are run before each release
  via `bash scripts/supply-chain-check.sh`.
  If there are CVEs in dependencies, the release is **blocked** until resolved.
- **`cargo deny`** also verifies licenses (allow-list compatible with
  proprietary binary), bans (duplicate versions), and sources (crates.io only).
  Configuration in `deny.toml`.
- **Historical CVEs**: standard policy is `cargo update -p <crate>` and
  immediate verification.
- **OpenSSL vendored**: the binary compiles OpenSSL from `openssl-src-rs`.
  Internal maintainer CVE response runbook
  (target: bump + rebuild + release in <48h for critical CVE).
- **Professional external audit**: deferred to the future, budget pending.

---

## CrowdSec — Recommended intrusion detection

CrowdSec is a security tool **external** to Enola CLI. It is not
integrated into the binary or the UI — it is installed and managed
independently. It is **highly recommended** for any deployment
exposed to the internet or Tor.

### Installation

```bash
sudo apt install crowdsec
sudo systemctl enable --now crowdsec
```

### Useful `cscli` commands

#### Local decisions and alerts

| Command | Description |
|---------|-------------|
| `cscli decisions list` | List active decisions (banned IPs, duration, reason) |
| `cscli alerts list` | List all generated alerts |
| `cscli alerts show <id>` | Details of a specific alert |
| `cscli metrics` | Real-time metrics for parsers, scenarios, and bouncers |

#### Community alerts (CrowdSec Central Intel)

| Command | Description |
|---------|-------------|
| `cscli hub list` | Installed collections, parsers, and scenarios |
| `cscli hub show <item>` | Details of a hub item |

#### Bouncers

| Command | Description |
|---------|-------------|
| `cscli bouncers list` | Registered bouncers and their status |

#### Daemon metrics

| Command | Description |
|---------|-------------|
| `cscli metrics show` | Detailed metrics per component |
| `journalctl -u crowdsec -f` | Service logs in systemd |

#### Local API

| Command | Description |
|---------|-------------|
| `cscli api status` | Local API status (default `127.0.0.1:8080`) |
| `curl http://127.0.0.1:8080/v1/decisions` | Query decisions via API (requires API key) |

#### Web dashboard (optional)

| Command | Description |
|---------|-------------|
| `cscli dashboard setup` | Set up Metabase (requires Docker) |
| `cscli dashboard start` | Start dashboard at `https://127.0.0.1:443` |

### Why it's not integrated into Enola CLI

CrowdSec is a system tool (like `nginx` or `tor`), not an
Enola subcommand. Keeping it external allows:

- **Independent updates**: CrowdSec updates with `apt`,
  without depending on Enola CLI releases.
- **Separation of responsibilities**: Enola CLI manages Tor services;
  CrowdSec manages threat detection.
- **Smaller attack surface**: we don't expose `cscli` via the Enola
  web API (which listens on localhost).

### Nginx integration

If you use CrowdSec with Nginx, install the Nginx bouncer:

```bash
sudo apt install crowdsec-nginx-bouncer
sudo systemctl reload nginx
```

This makes Nginx automatically block IPs that CrowdSec
has flagged as malicious, returning HTTP 403.

---

## Quick references for curious users

| Topic | Public document |
|-------|-----------------|
| Verify the binary you downloaded | [`verify-downloads.md`](verify-downloads.md) |
| Post-quantum security status | `docs/user/quantum-security.md` |
| Harden your system before installing | [`quickstart.md`](quickstart.md) |
| Atomic uninstall | `docs/user/uninstall.md` |
