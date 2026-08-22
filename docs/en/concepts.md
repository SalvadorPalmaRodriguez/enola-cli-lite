> **User document:** `docs/en/concepts.md`
> **Version:** 2.0 | **Updated:** 2026-08-08
> **Status:** ✅ **CURRENT — Concepts Guide**
> **References:** commands.md, faq.md
> **Spanish original:** [`docs/user/general/concepts.md`](../user/general/concepts.md)

# 💡 Key Concepts — Enola CLI

## Tor and .onion addresses

Tor (The Onion Router) is a network that encrypts and anonymizes traffic across
multiple nodes ("hops"). Tor **hidden services** have a `.onion` address —
only accessible from within the Tor network.

**What this means for you:**
- Your service has no public IP or registered domain.
- Visitors don't know which server serves the content.
- You don't need to open ports on your router or configure DNS.
- Traffic is encrypted end-to-end.

**How to access a .onion address:**
- Install the [Tor Browser](https://www.torproject.org/)
- Type the `.onion` address in the address bar

---

## The port chain: .onion → Nginx → App

When Enola deploys a web service, the architecture is:

```
Visitor (Tor Browser)
    │
    │  encrypted Tor network
    ▼
Tor (on your server)        ← HiddenServicePort 80 → 127.0.0.1:NGINX_PORT
    │
    │  127.0.0.1 (localhost only)
    ▼
Nginx (reverse proxy)       ← listen 127.0.0.1:NGINX_PORT
    │                         proxy_pass 127.0.0.1:APP_PORT
    │  127.0.0.1 (localhost only)
    ▼
Your application / Docker    ← -p 127.0.0.1:APP_PORT:INTERNAL_PORT
```

**Key points:**
- `NGINX_PORT` and `APP_PORT` are **internal** ports — never accessible from outside.
- Docker always binds to `127.0.0.1`, never to `0.0.0.0`.
- The visitor only sees the `.onion` address.

---

## Ports and network security

Enola manages three types of ports:

| Type | Example | Who can see it |
|------|---------|----------------|
| Virtual .onion port | 80, 443 | Tor visitors only |
| Nginx port (listen) | 10000-20000 | Localhost only |
| App port (backend) | 8080-9000 (WordPress), 10000-15000 (Git HTTP), 30000-35000 (Git SSH) | Localhost only |

**UFW (Firewall):**
Docker can bypass UFW rules directly. Enola includes the
`firewall setup` command that configures the `DOCKER-USER` chain to
block external access to Docker ports.

```
sudo enola-cli firewall setup
```

---

## VPN and WireGuard

WireGuard is a modern, lightweight VPN protocol. Unlike Tor, which provides
anonymity, a VPN provides **authentication and encryption** between known peers.

**Key difference Tor vs VPN:**

| Feature | Tor | VPN (WireGuard) |
|---------|-----|------------------|
| Anonymity | Yes (nobody knows who you are) | No (peers know each other) |
| Authentication | No | Yes (cryptographic keys) |
| Latency | High (3 hops) | Low (1 hop) |
| Use case | Publishing anonymous content | Authenticated remote access |

**Peer model:** each WireGuard interface has a private key and N peers with
their public keys. Traffic between peers is encrypted with ChaCha20-Poly1305.

```
sudo enola-cli vpn create wg0 --port 51820
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
```

---

## AppArmor and sandboxing

AppArmor is a Linux kernel module that confines individual programs to a
limited set of resources (profiles). Enola CLI uses it to isolate services.

**Operating modes:**

| Mode | Description |
|------|-------------|
| `enforce` | Blocks actions not allowed by the profile (recommended in production) |
| `complain` | Allows actions but logs them (useful for debugging profiles) |
| `disable` | No confinement |

**Complementing with UFW:** AppArmor confines processes; UFW controls ports. Both
are needed for defense in depth.

```
sudo enola-cli apparmor setup
sudo enola-cli apparmor mode --enforce
```

---

## Docker and container architecture

Enola uses Docker to isolate each service. Each CMS, Git, or file share runs in
its own container with limited resources.

**Security principles:**

- **Bind to `127.0.0.1`**: Docker ports are never exposed to `0.0.0.0`. Only
  Nginx (reverse proxy) accesses them via localhost.
- **Bind mounts to `/srv/`**: persistent data lives in `/srv/enola-{type}/{name}/`.
  This allows data to survive container recreation.
- **Isolated networks**: each service has its own Docker network (`enola_net_{type}_{name}`).
- **Docker secrets**: passwords and tokens are mounted as Docker secrets in
  `/run/secrets/` (read-only), not as plaintext environment variables.

---

## CMS: catalog and stacks

Enola supports 6 CMS platforms with different stacks:

| CMS | Language | DB | Containers | Min RAM | Internal port |
|-----|----------|----|------------|---------|---------------|
| WordPress | PHP | MariaDB | 2 (web + db) | 512 MB | 80 |
| Drupal | PHP | MariaDB | 2 (web + db) | 768 MB | 80 |
| Ghost | Node.js | SQLite | 1 (web) | ~256 MB | 2368 |
| Magnolia | Java | — | 1 (Tomcat) | ≥4 GB | 8080 |
| Strapi | Node.js | Postgres | 2 (web + db) | 512 MB | 1337 |
| Wagtail | Python | Postgres | 2 (web + db) | 512 MB | 8000 |

**Which to choose?**
- Simple blog → Ghost (lighter) or WordPress (more plugins)
- Large corporate site → Magnolia (Java enterprise)
- Headless/API-first → Strapi
- Structured content → Wagtail (Django)
- Multilingual + permissions → Drupal

---

## Post-quantum signatures (PQC) — does not imply quantum anonymity

Enola CLI incorporates post-quantum cryptography to prepare against future
quantum computers that could break current algorithms.

**ML-DSA-65 (FIPS 204):** a post-quantum digital signature algorithm based on
lattices. Used to sign release binaries.

**minisign:** a traditional signature system used to verify the advisory
feed. Each release has a `.minisig` file verified with the
public key embedded in the binary.

**SSH hardening with sntrup761x25519:** the `maintenance ssh-harden-pqc`
command configures SSH to use hybrid post-quantum algorithms.

```
sudo enola-cli maintenance ssh-harden-pqc
```

**PQC roadmap:** see `enola-cli docs quantum-security` for the full plan.

---

## Advisory feed and updates

Enola CLI checks a minisign-signed JSON feed to detect updates
and security advisories.

**How `update check` works:**
1. Downloads the JSON feed from the configured URL (`[update].feed_url`)
2. Verifies the minisign signature (`{feed_url}.minisig`)
3. Compares the current version with the latest in the feed
4. Shows advisories affecting the installed version

**Exit codes for CI/scripts:**

| Code | Meaning |
|------|---------|
| 0 | OK (includes update available without critical advisory) |
| 11 | Critical advisory affects current version |
| 12 | Current version below minimum supported |
| 20 | Feed invalid/unparseable/unreachable |
| 21 | Minisign signature invalid or missing |

**Minisign key rotation:** if the public key changes, a new `minisign.pub` is
distributed and the embedded key is updated in the next release.

## Cross-references

| Document | Purpose |
|----------|---------|
| [`commands.md`](commands.md) | Command index |
| [`quickstart.md`](quickstart.md) | Quick start guide |
| [`faq.md`](faq.md) | Frequently asked questions |
