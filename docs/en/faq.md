> **User document:** `docs/en/faq.md`
> **Version:** 2.2 | **Updated:** 2026-10-24
> **Status:** ✅ **CURRENT — Frequently Asked Questions**
> **References:** commands.md, concepts.md
> **Spanish original:** [`docs/user/general/faq.md`](../user/general/faq.md)

# ❓ FAQ — Enola CLI

## General

**Do I need a public IP or a domain?**
No. Enola uses the Tor network to expose your services. The `.onion` address
works without a public IP, without DNS, without open ports on your router.

**What happens if I shut down my server?**
Services become unavailable. The `.onion` address is permanent
(associated with keys in `/var/lib/tor/`) — when you restart the server and services,
the same address becomes available again.

**Can I have multiple services on the same server?**
Yes, with no limit. Each service has its own `.onion` address.

**Should my machine suspend? (24/7 server vs. desktop)**
While the machine is suspended your `.onion` services are **unreachable**,
just as if it were powered off. Pick one of these two modes:

- **24/7 server** (services always available): block suspend.
  ```bash
  sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
  ```
  If you also want closing the lid or pressing the power button to do nothing,
  create a drop-in in `/etc/systemd/logind.conf.d/` with
  `HandleLidSwitch=ignore`, `HandlePowerKey=ignore`, `HandleSuspendKey=ignore`
  and `IdleAction=ignore`.

- **Desktop machine** (suspends again as before): revert the above.
  ```bash
  sudo systemctl unmask sleep.target suspend.target hibernate.target hybrid-sleep.target
  # Move (or delete) the logind drop-in you created and reboot
  # (or `sudo systemctl restart systemd-logind`, which ends the graphical session)
  ```
  Then set the idle timeout in your desktop. XFCE example
  (3 minutes on AC and on battery, suspend mode):
  ```bash
  for p in inactivity-on-ac inactivity-on-battery; do
    xfconf-query -c xfce4-power-manager -p /xfce4-power-manager/$p -n -t uint -s 3
  done
  for p in inactivity-sleep-mode-on-ac inactivity-sleep-mode-on-battery; do
    xfconf-query -c xfce4-power-manager -p /xfce4-power-manager/$p -n -t uint -s 1
  done
  ```
  On GNOME: **Settings → Power**.

> ⚠️ If the machine **never** suspends, not even with `systemctl suspend`,
> check whether the targets are masked: `systemctl status suspend.target`
> (it shows `Loaded: masked` in that case).

---

## Tor

**My service takes a while to appear (first time)**
The Tor network can take 30-90 seconds to propagate a new `.onion` address.
Wait a minute and try again.

**How do I regenerate my .onion address?**
```
sudo enola-cli tor rotate my-service
```
⚠️ The previous address will stop working permanently.

**`curl: (7) Failed to connect`**
Make sure to use Tor's SOCKS5 proxy:
```
curl --proxy socks5h://127.0.0.1:9050 http://XXXXX.onion/
```
Or use the Tor Browser.

---

## WordPress

**WordPress shows error 500 / installation page**
This is normal on first run. Open the Tor Browser and access the
`.onion` address or `http://localhost:PORT/`. Complete the
WordPress installation wizard.

**Where are the WordPress files?**
In `/srv/enola-wordpress/NAME_wp/` (Docker bind mount).

**How do I update WordPress?**
```
sudo enola-cli wp update my-wordpress
```

---

## CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail)

**Which CMS should I choose?**
It depends on your use case. See the comparison table in `enola-cli docs concepts cms`.
Quick summary: simple blog → Ghost; plugins → WordPress; headless → Strapi;
enterprise → Magnolia; structured content → Wagtail; multilingual → Drupal.

**Why does Strapi need `build-image` before `create`?**
Strapi doesn't have an official pre-built Docker image with Enola's configuration.
The `build-image` command generates a custom image with injected secrets.
Without this step, `create` can't find the image.

**Where is my CMS data stored?**
Each CMS stores data in `/srv/enola-{type}/{name}/` (Docker bind mount).
This includes files, database, and secrets.

**Can I publish Ghost on Tor?**
Yes. The `ghost publish`, `ghost hide`, and `ghost edit` subcommands are implemented:
```
sudo enola-cli ghost publish myblog
sudo enola-cli ghost hide myblog
sudo enola-cli ghost edit myblog --http-port 8095
```

---

## VPN

**When to use VPN vs Tor?**
Tor for publishing anonymous content (nobody knows who you are). VPN for
authenticated remote access between known machines. You can use both simultaneously.

**How do I add a peer to my VPN?**
```
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
```
This generates the peer configuration with its private/public keys.

**Does my VPN expose ports to the internet?**
No. WireGuard only listens on the configured port (default 51820) and requires
cryptographic key authentication. Without an authorized peer, there's no connection.

---

## AppArmor

**Which AppArmor mode should I use?**
`complain` during the first few hours to detect violations without blocking
services. Switch to `enforce` when there are no more violations in the logs.

**Does AppArmor affect performance?**
The overhead is minimal (<1% in most workloads). The security benefit
far outweighs it.

---

## Updates

**How do I know if there's an update?**
```
sudo enola-cli update check
```
Checks the advisory feed and shows if there's a new version or security
advisories.

**What does exit code 11 mean in `update check`?**
There's a critical advisory affecting your current version. Update as soon as
possible with `update download --yes`.

**Is minisign signature mandatory?**
Yes. The advisory feed must be signed with minisign. Without a valid signature,
the CLI rejects the feed (exit code 21). You can use `--allow-unsigned` only in
development environments.

---

## Web Dashboard

**Can I access the dashboard from another machine?**
No. The web server binds to `127.0.0.1` exclusively. For remote access,
use an SSH tunnel or VPN.

**Does the token change every time I start the dashboard?**
Yes. A new random token is generated on each start. It's displayed in the
terminal where you ran `enola-cli web`.

---

## PQC (Post-quantum)

**What is ML-DSA-65?**
A post-quantum digital signature algorithm standardized by NIST (FIPS 204).
It resists attacks from quantum computers that would break RSA/ECDSA.

**Is SSH over Tor quantum-resistant?**
Standard SSH is not. Use `maintenance ssh-harden-pqc` to configure
hybrid post-quantum algorithms (sntrup761x25519) that are.

---

## Local AI (Ollama)

**`ai rag ingest` fails with "Ollama no devolvió embeddings" (Ollama returned no embeddings) instead of indexing**
This is intentional: if Ollama returns no embeddings (instance stopped or the
embedding model missing), enola-cli **indexes nothing** rather than silently
filling the index with lower-quality vectors. Start the instance
(`sudo enola-cli ai start <NAME>`) and retry. On an interactive terminal you
are offered retry / hash / cancel; to index by hash on purpose use
`--embedding-fallback hash`. A collection never mixes methods: if you created
it with hash, it stays hash.

**`ask --yes` still asks for confirmation (or fails without a terminal)**
`--yes` only approves **mutating** operations. **Destructive** ones (delete,
`--force`) always require a `[s/N]` confirmation on an interactive terminal;
without a terminal they fail even with `--yes`. Reason: content read by tools
(RAG documents, logs) could lead the model to request a destructive action.
See [`commands-ask.md`](commands-ask.md).

**The `docs_lookup` MCP tool returns `results: []`**
The documentation collection (`enola_docs`) is not indexed yet. Run
`sudo enola-cli ai rag ingest-docs <NAME>` (and again after updating
enola-cli), or use `docs_search`, which needs no embeddings. Your own index
(`ai rag ingest`/`sync`) is never exposed to `docs_lookup`.

---

## Configuration

**Where is the configuration file?**
At `~/.enola/config.toml`. Copy `config.example.toml` as a template.
Required permissions: `chmod 0600 ~/.enola/config.toml`.

**Can I use a .onion URL for the update feed?**
Yes. Configure `[update].feed_url` with your .onion URL. The CLI routes it
automatically through Tor via `[http].tor_socks_proxy`.

---

## Ports and networking

**How do I see which ports Enola uses?**
```
sudo enola-cli ports list
```

**A port shows as occupied**
Stopped Docker containers retain their bindings. Check with:
```
docker ps -a --format "{{.Names}}: {{.Ports}}"
```

**How do I configure the firewall?**
```
sudo enola-cli firewall setup    # Initial setup (recommended)
sudo enola-cli firewall status   # View status
```

---

## Common errors

| Error | Likely cause | Solution |
|-------|-------------|----------|
| `Permission denied` | Not run with sudo | `sudo enola-cli ...` |
| `Docker not running` | Docker stopped | `sudo systemctl start docker` |
| `Tor service not running` | Tor stopped | `sudo systemctl start tor` |
| `Nginx config error` | Broken config | `sudo nginx -t` for details |
| `Port already in use` | Port occupied | `sudo enola-cli ports list` |

---

## More help

```
sudo enola-cli docs quickstart          # Step-by-step quick start
sudo enola-cli docs commands            # Full command reference
sudo enola-cli docs concepts tor        # Tor concepts
sudo enola-cli docs examples deploy     # Deployment examples
sudo enola-cli --help                   # CLI help
sudo enola-cli COMMAND --help           # Specific command help
```

## Cross-references

| Document | Purpose |
|----------|---------|
| [`commands.md`](commands.md) | Command index |
| [`concepts.md`](concepts.md) | Key concepts (Tor, security) |
| [`quickstart.md`](quickstart.md) | Quick start guide |
