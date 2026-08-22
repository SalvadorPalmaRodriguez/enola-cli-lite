> **User document:** `docs/en/quickstart.md`
> **Version:** 2.0 | **Updated:** 2026-07-31
> **Status:** ✅ **CURRENT — Quick Start Guide**
> **References:** commands.md, concepts.md, examples.md, faq.md
> **Spanish original:** [`docs/user/guia/quickstart.md`](../user/guia/quickstart.md)

# 🚀 Quick Start Guide — Enola CLI

Enola CLI lets you deploy web services, Git servers, CMS platforms,
and complete stacks, all accessible anonymously through the Tor network.
No cloud servers — everything runs on your machine.

> **Important:** this guide assumes you have already installed the `enola-cli` binary.

---

## Step 1: Verify your configuration

Show the current configuration and validate it:

```bash
enola-cli config-show
enola-cli config-validate
```

---

## Step 2: Your first file share on Tor

Create a static file server accessible via a .onion address:

```bash
sudo enola-cli files create --name my-web
```

You will see something like:
```
✅ Service created: my-web
🧅 .onion address: abc123...xyz.onion
```

Copy your web content into `/srv/enola-files/my-web/`.

---

## Step 3: Your first Git server

```bash
sudo enola-cli git create --name my-repo --http-port 10000
```

Access it from the Tor Browser at the .onion address shown.

---

## Step 4: Explore more

```bash
sudo enola-cli diag summary          # View overall system status
sudo enola-cli ports list            # View ports in use
```

## Help commands

```
sudo enola-cli docs commands          # Reference for all commands
sudo enola-cli docs concepts tor      # Understand how Tor works in Enola
sudo enola-cli docs examples deploy   # Deployment examples
sudo enola-cli docs faq               # Frequently asked questions
sudo enola-cli docs search <term>     # Search the documentation
```

---

## Key concepts

- **Tor**: Each service has a unique `.onion` address. Traffic is
  encrypted through the Tor network. You don't need a domain or public IP.
- **Nginx**: Acts as a reverse proxy between Tor and your application.
- **Port chain**: `.onion` → Nginx (127.0.0.1) → your app (127.0.0.1)
  Internal ports are never accessible from outside.
- **Updates**: The CLI checks for updates automatically
  and verifies its own hash on startup.

## Cross-references

| Document | Purpose |
|----------|---------|
| [`commands.md`](commands.md) | Command index |
| [`concepts.md`](concepts.md) | Key concepts of Enola CLI |
| [`faq.md`](faq.md) | Frequently asked questions |
