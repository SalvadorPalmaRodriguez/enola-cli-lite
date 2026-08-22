# 🚀 Enola CLI — Self-Hosted Tor Services, CMS & Privacy Toolkit

**English** · **[Español](README.es.md)**

[![Version](https://img.shields.io/badge/version-0.1.1--alpha-blue.svg)](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases)
[![License](https://img.shields.io/badge/license-Proprietary%20(source--visible)-orange.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux-green.svg)](https://www.debian.org/)
[![Rust](https://img.shields.io/badge/rust-1.96-orange.svg)](https://www.rust-lang.org/)

> **Enola CLI** is a Rust command-line tool for self-hosting **Tor hidden services (.onion)**, **Git servers (Forgejo)**, **CMS platforms (WordPress, Drupal, Ghost, Magnolia, Strapi, Wagtail)**, **anonymous file sharing**, **WireGuard VPN**, **UFW firewall** and **AppArmor sandboxing** on Debian/Linux — with **post-quantum signed releases (ML-DSA-65, FIPS 204)**. Everything binds to `127.0.0.1` and is exposed only through Tor: privacy by design.
>
> 📖 **Documentation**: [https://salvadorpalmarodriguez.github.io/enola-cli-lite/](https://salvadorpalmarodriguez.github.io/enola-cli-lite/) · 📄 **[llms.txt](llms.txt)** for AI indexers

---

## ✨ Features

- **🧅 Tor hidden services** — create `.onion` services (web, static, file server, raw TCP) with automatic Nginx wiring and client authorization (x25519).
- **🐙 Git hosting** — Forgejo servers with SSH-over-Tor cloning, user management and a pipeline watcher.
- **🌐 Six CMS platforms** — WordPress, Drupal, Ghost, Magnolia, Strapi and Wagtail, each Dockerized, localhost-only and publishable to Tor with one command.
- **📁 Anonymous file sharing** — Nginx autoindex shares over `.onion`, with optional HTTPS (TLSv1.3) and Tor client auth.
- **🔒 WireGuard VPN** — encrypted tunnels for trusted devices, with optional preshared keys for post-quantum resistance.
- **🛡️ Hardening built in** — UFW firewall setup (incl. DOCKER-USER chain), AppArmor profiles, SSH post-quantum KEX hardening.
- **🔐 Post-quantum release signing** — every release is signed with minisign (Ed25519) **and** ML-DSA-65 (FIPS 204); verification is offline via `enola-cli verify`.
- **📚 Embedded offline docs** — `enola-cli docs` works with no network.
- **🌐 Local web dashboard** — token-protected GUI bound to `127.0.0.1`.

## 🏗️ Architecture

Hexagonal (ports & adapters) Rust codebase:

```
src/domain/         → Pure business logic (no external dependencies)
src/ports/          → Injectable traits (interfaces)
src/adapters/       → Concrete implementations (Docker, Nginx, Tor, …)
src/application/    → Orchestration (uses ports only)
src/cli/            → Command routing and validation
src/infrastructure/ → Cross-cutting utilities (privileges, locks, …)
```

Traffic model for every service: `.onion:VIRTUAL_PORT → Nginx:INTERNAL_PORT → App:TARGET_PORT` — all internal ports bind to `127.0.0.1` only.

---

## 📋 Table of Contents

- [Installation](#-installation)
- [Basic Usage](#-basic-usage)
- [Commands by Module](#-commands-by-module)
  - [🧅 Tor — Hidden Services](#-tor--hidden-services)
  - [🐙 Git — Forgejo Servers](#-git--forgejo-servers)
  - [🌐 WordPress — Websites](#-wordpress--websites)
  - [🌐 Drupal — CMS](#-drupal--cms)
  - [✍️ Ghost — Blogs](#️-ghost--blogs)
  - [☕ Magnolia — Java CMS](#-magnolia--java-cms)
  - [🚀 Strapi — Headless CMS](#-strapi--headless-cms)
  - [🐦 Wagtail — Django CMS](#-wagtail--django-cms)
  - [📁 Files — File Sharing](#-files--file-sharing)
  - [🔧 Maintenance](#-maintenance)
  - [🩺 Diagnostics](#-diagnostics)
  - [🧪 Test — System Tests](#-test--system-tests)
  - [📝 Logs](#-logs)
  - [🔌 Ports](#-ports)
  - [🛡️ Firewall — UFW](#️-firewall--ufw)
  - [🛡️ AppArmor — Sandboxing](#️-apparmor--sandboxing)
  - [🔒 VPN — WireGuard](#-vpn--wireguard)
  - [📦 Setup — Dependencies](#-setup--dependencies)
  - [🩺 Doctor — Dependency Check](#-doctor--dependency-check)
  - [🔄 Update — Advisories](#-update--advisories)
  - [🔐 Verify — PQC Verification](#-verify--pqc-verification)
  - [🗑️ Uninstall](#️-uninstall)
  - [📚 Docs — Offline Documentation](#-docs--offline-documentation)
  - [📄 License Command](#-license-command)
  - [📖 Quickref](#-quickref)
  - [🌐 Web — Local Dashboard](#-web--local-dashboard)
  - [⚙️ Config](#️-config)
- [Practical Examples](#-practical-examples)
- [Troubleshooting](#-troubleshooting)
- [License](#-license)

---

## 📦 Installation

### From GitHub Releases (production)

```bash
# Download and verify (recommended)
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash
```

The installer downloads the binary, verifies SHA256 + minisign signature, and installs everything.
Full guide: [verify-downloads.md](docs/user/verify/verify-downloads.md)

---

## 🎯 Basic Usage

```bash
# Root privileges required
sudo enola-cli <command> [subcommand] [options]

# General help
sudo enola-cli --help

# Module-specific help
sudo enola-cli tor --help

# Global options
sudo enola-cli --format json tor list    # JSON output
```

### Command Tree

```
enola-cli
├── tor         # Tor hidden services (.onion)
├── git         # Git servers (Forgejo)
├── wp          # WordPress sites
├── drupal      # Drupal sites (CMS)
├── ghost       # Ghost blogs (CMS)
├── magnolia    # Magnolia CMS (Tomcat)
├── strapi      # Headless CMS Strapi
├── wagtail     # Wagtail CMS (Django)
├── files       # Secure file sharing
├── maintenance # Maintenance operations
├── diag        # System diagnostics
├── test        # Run tests
├── logs        # View system logs
├── ports       # Port management
├── firewall    # UFW firewall
├── apparmor    # AppArmor sandboxing
├── vpn         # WireGuard VPN tunnels
├── setup       # Install dependencies
├── doctor      # Check dependencies
├── update      # Advisory feed and updates
├── verify      # Verify download authenticity (PQC)
├── uninstall   # CLI uninstallation
├── docs        # Documentation embedded in the binary
├── license     # License text
├── quickref    # Docker ↔ Enola quick reference
├── web         # Local web dashboard (GUI)
├── config-show    # Show effective configuration
└── config-validate # Validate configuration
```

---

## 📚 Commands by Module

---

## 🧅 Tor — Hidden Services

Manage Tor hidden services (.onion) with different architectures.

### List Services

```bash
sudo enola-cli tor list
```

Shows all active Tor services with their .onion addresses and ports.

### Create a Service

```bash
sudo enola-cli tor create [options]
```

| Option | Description | Default |
|--------|-------------|---------|
| `-n, --name <NAME>` | Service name (required) | - |
| `-s, --service-type <TYPE>` | Type: `web`, `static`, `files`, `raw` | `web` |
| `-p, --virtual-port <PORT>` | Public .onion port | `80` |
| `-t, --target-port <PORT>` | Your application's port | auto |
| `--ssl` | Enable HTTPS with a self-signed certificate | `false` |

**Service types:**

| Type | Architecture | Use case |
|------|--------------|----------|
| `web` / `proxy` | Tor → Nginx → App | Web apps (recommended) |
| `static` | Tor → Nginx | Static sites |
| `files` | Tor → Nginx | File server |
| `raw` / `tcp` | Tor → App | SSH, databases |

**Examples:**

```bash
# Basic web service (HTTP)
sudo enola-cli tor create -n myapp -s web --target-port 3000

# Web service with HTTPS
sudo enola-cli tor create -n myapp-secure -s web --target-port 8080 --ssl

# File server
sudo enola-cli tor create -n my-files -s files

# Static site
sudo enola-cli tor create -n my-blog -s static
```

### Start/Stop a Service

```bash
# Start
sudo enola-cli tor start <name>

# Stop
sudo enola-cli tor stop <name>
```

### Edit Ports

```bash
sudo enola-cli tor edit <name> [options]
```

| Option | Description |
|--------|-------------|
| `-p, --virtual-port <PORT>` | Public .onion port |
| `-n, --nginx-port <PORT>` | Internal Nginx port |
| `-t, --target-port <PORT>` | Your application's port |
| `--auto-ports` | Find free ports automatically |

**Port flow:** `.onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT`

```bash
# Change virtual port to 8080
sudo enola-cli tor edit myapp -p 8080

# Auto-assign free ports
sudo enola-cli tor edit myapp --auto-ports

# Change application port
sudo enola-cli tor edit myapp -t 4000
```

### Remove a Service

```bash
sudo enola-cli tor remove <name> [--force]
```

### Rotate Identity (.onion)

```bash
sudo enola-cli tor rotate <name>
```

Generates a new .onion address for the service (useful if the previous one was compromised).

### Client Authorization

Access control for Tor services using public-key cryptography.

```bash
# List authorized clients
sudo enola-cli tor auth list <service>

# Enable authorization
sudo enola-cli tor auth enable <service>

# Disable authorization
sudo enola-cli tor auth disable <service>

# Add an authorized client
sudo enola-cli tor auth add <service> -c <client> -p <public_key>

# Revoke a client's access
sudo enola-cli tor auth revoke <service> -c <client>

# Generate a client keypair
sudo enola-cli tor auth generate -c <client_name>

# Rotate a client's keys (mitigates harvest-now-decrypt-later)
sudo enola-cli tor auth rotate <service> -c <client>
```

---

## 🐙 Git — Forgejo Servers

Manage Git servers (Forgejo) with Tor integration.

### Main Commands

```bash
# List servers
sudo enola-cli git list

# Create a server (web mode: browser install wizard)
sudo enola-cli git create -n my-git [--ssl] [--http-port <PORT>] [--ssh-port <PORT>]

# Create a server (CLI mode: admin created automatically)
sudo enola-cli git create -n my-git --admin-user alice --admin-password MyPass123

# Lifecycle control
sudo enola-cli git start <name>
sudo enola-cli git stop <name>
sudo enola-cli git status <name>
sudo enola-cli git delete <name> [--force]

# Configure user self-registration (enable/disable)
sudo enola-cli git registration <name> --enable
```

### Edit Ports

```bash
sudo enola-cli git edit <name> [options]
```

| Option | Description |
|--------|-------------|
| `--http-port <PORT>` | HTTP port |
| `--https-port <PORT>` | HTTPS port |
| `--ssh-port <PORT>` | SSH port |
| `--auto-ports` | Auto-detect free ports |

### Expose on Tor

```bash
# Publish on Tor
sudo enola-cli git publish <name> [--ssl]

# Hide from Tor
sudo enola-cli git hide <name>
```

### User Management

```bash
# List users
sudo enola-cli git user list <server>

# Create a user
sudo enola-cli git user create <server> -u user -e email@test.com -p password

# Delete a user
sudo enola-cli git user delete <server> -u user
```

### Pipeline Watcher

```bash
# Run the pipeline watcher (foreground)
sudo enola-cli git watcher
```

---

## 🌐 WordPress — Websites

Manage WordPress sites with Docker and Tor exposure.

### Main Commands

```bash
# List sites
sudo enola-cli wp list

# Create a site
sudo enola-cli wp create -n my-blog [--http-port <PORT>]   # auto: range 8080-9000

# Lifecycle control
sudo enola-cli wp start <name>
sudo enola-cli wp stop <name>
sudo enola-cli wp restart <name>
sudo enola-cli wp delete <name> [--force]

# Check status
sudo enola-cli wp status <name>

# Update WordPress (with backup)
sudo enola-cli wp update <name>

# Configuration
sudo enola-cli wp config <name>
```

### Tor Exposure

```bash
# Publish on Tor
sudo enola-cli wp publish <name>

# Hide from Tor
sudo enola-cli wp hide <name>
```

### Edit Configuration

```bash
sudo enola-cli wp edit <name> [options]
```

| Option | Description |
|--------|-------------|
| `--http-port <PORT>` | HTTP port |
| `--https-port <PORT>` | HTTPS port |
| `--ssl <true/false>` | Enable/disable SSL |
| `--auto-ports` | Auto-detect free ports |

---

## 🌐 Drupal — CMS

Manage Drupal sites. Stack: `drupal:10-apache` + `mariadb:10.11`. Data lives in `/srv/enola-drupal/<name>/`.

```bash
# List sites
sudo enola-cli drupal list

# Create a site (internal HTTP port required)
sudo enola-cli drupal create --name my-site --http-port 8090

# Lifecycle
sudo enola-cli drupal start <name>
sudo enola-cli drupal stop <name>
sudo enola-cli drupal status <name>
sudo enola-cli drupal delete <name> [--force]

# Tor exposure
sudo enola-cli drupal publish <name>
sudo enola-cli drupal hide <name>

# Change HTTP port (recreates the web container atomically)
sudo enola-cli drupal edit <name> --http-port <PORT>
```

---

## ✍️ Ghost — Blogs

Manage Ghost blogs. Stack: `ghost:5-alpine` + embedded SQLite (single container). Data lives in `/srv/enola-ghost/<name>/content/`.

```bash
# List blogs
sudo enola-cli ghost list

# Create a blog (internal HTTP port required; container uses 2368)
sudo enola-cli ghost create --name my-blog --http-port 8095

# Lifecycle
sudo enola-cli ghost start <name>
sudo enola-cli ghost stop <name>
sudo enola-cli ghost status <name>
sudo enola-cli ghost delete <name> [--force]

# Tor exposure
sudo enola-cli ghost publish <name>
sudo enola-cli ghost hide <name>

# Change HTTP port
sudo enola-cli ghost edit <name> --http-port <PORT>
```

---

## ☕ Magnolia — Java CMS

Manage Magnolia CMS instances. Stack: `magnolia-cms:6` (Tomcat, Java). **Requires ≥4 GB of available RAM.**

```bash
# List instances
sudo enola-cli magnolia list

# Create an instance (internal HTTP port required; Tomcat uses 8080)
sudo enola-cli magnolia create --name my-site --http-port 8100

# Lifecycle
sudo enola-cli magnolia start <name>
sudo enola-cli magnolia stop <name>
sudo enola-cli magnolia status <name>
sudo enola-cli magnolia delete <name> [--force]

# Tor exposure
sudo enola-cli magnolia publish <name>
sudo enola-cli magnolia hide <name>
```

---

## 🚀 Strapi — Headless CMS

Manage Strapi instances. Stack: `enola/strapi:5.49.0` + `postgres:16-alpine`. Generates per-instance secrets with 0600 permissions.

```bash
# Build the production Docker image (once, before the first create; ~5-10 min)
sudo enola-cli strapi build-image [--force]

# List instances
sudo enola-cli strapi list

# Create an instance (internal HTTP port required; Strapi uses 1337)
sudo enola-cli strapi create --name my-api --http-port 1337

# Lifecycle
sudo enola-cli strapi start <name>
sudo enola-cli strapi stop <name>
sudo enola-cli strapi status <name>
sudo enola-cli strapi delete <name> [--force]

# Tor exposure
sudo enola-cli strapi publish <name>
sudo enola-cli strapi hide <name>
```

---

## 🐦 Wagtail — Django CMS

Manage Wagtail instances. Stack: Wagtail (Python/Django) + `postgres:16-alpine`.

```bash
# List instances
sudo enola-cli wagtail list

# Create an instance (internal HTTP port required; Wagtail uses 8000)
sudo enola-cli wagtail create --name my-site --http-port 8200

# Lifecycle
sudo enola-cli wagtail start <name>
sudo enola-cli wagtail stop <name>
sudo enola-cli wagtail status <name>
sudo enola-cli wagtail delete <name> [--force]

# Tor exposure
sudo enola-cli wagtail publish <name>
sudo enola-cli wagtail hide <name>
```

---

## 📁 Files — File Sharing

Create secure file servers accessible via Tor.

```bash
# List shares
sudo enola-cli files list

# Create a share
sudo enola-cli files create -n my-files [-a] [--ssl]

# Edit port
sudo enola-cli files edit <name> -p 8080

# Fix permissions
sudo enola-cli files fix-perms <name>

# Delete a share
sudo enola-cli files delete <name> [-f]
```

**Files directory:** `/srv/enola-files/<name>/`

```bash
# Add files to the share
sudo cp file.pdf /srv/enola-files/my-files/
sudo cp -r folder/ /srv/enola-files/my-files/
```

---

## 🔧 Maintenance

System maintenance operations.

```bash
# Show system status
sudo enola-cli maintenance status

# Run smoke test
sudo enola-cli maintenance smoke-test

# Automatic health checks
sudo enola-cli maintenance enable-checks
sudo enola-cli maintenance disable-checks
sudo enola-cli maintenance timer-status

# Configure SSH check
sudo enola-cli maintenance ssh-config

# Harden SSH with post-quantum-safe algorithms (OpenSSH 9.0+)
sudo enola-cli maintenance ssh-harden-pqc [--dry-run] [--force]

# Create a system backup
sudo enola-cli maintenance backup

# Clean temporary files and residual data
sudo enola-cli maintenance cleanup [--target all|logs|docker] [--dry-run] [--keep-days 7]
```

---

## 🩺 Diagnostics

Check the status of system components.

```bash
# Summary of all services
sudo enola-cli diag summary

# Check individual components
sudo enola-cli diag nginx
sudo enola-cli diag tor
sudo enola-cli diag ssh
sudo enola-cli diag wordpress

# WordPress/Nginx sync
sudo enola-cli diag wp-sync

# Test Nginx configuration
sudo enola-cli diag nginx-test

# Show system resources (RAM, Disk, GPU)
sudo enola-cli diag resources
```

---

## 🧪 Test — System Tests

Run automated system tests.

```bash
# Run all tests
sudo enola-cli test run

# Run with a filter
sudo enola-cli test run -f "tor"

# List available tests
sudo enola-cli test list

# Run benchmarks
sudo enola-cli test benchmark

# Show last results
sudo enola-cli test results

# Clean test artifacts
sudo enola-cli test clean
```

---

## 📝 Logs

View and manage system logs.

```bash
# List log sources
sudo enola-cli logs list

# View logs from a source (default: 50 lines)
sudo enola-cli logs view <source> [-l 50] [-f]

# Available sources: system, tor, nginx, docker, etc.

# View installation logs
sudo enola-cli logs install

# View smoke test logs
sudo enola-cli logs smoke-test
```

---

## 🔌 Ports

Shows every port used by Enola services (Tor, Nginx, Docker).

```bash
# List all ports
sudo enola-cli ports list
```

Includes stopped containers that retain Docker port bindings.

---

## 🛡️ Firewall — UFW

Manage the host's UFW firewall.

```bash
# Configure a secure default policy
sudo enola-cli firewall setup

# Show firewall status
sudo enola-cli firewall status

# Allow/deny ports
sudo enola-cli firewall allow --port <port>
sudo enola-cli firewall deny --port <port>
```

---

## 🛡️ AppArmor — Sandboxing

Manage AppArmor profiles for process isolation.

```bash
# Install base profiles (nginx, tor, docker)
sudo enola-cli apparmor setup

# Show profile status
sudo enola-cli apparmor status

# Change mode (enforce/complain/disable)
sudo enola-cli apparmor mode --enforce
```

---

## 🔒 VPN — WireGuard

Manage WireGuard VPN tunnels for authenticated remote access.

```bash
# Create a VPN interface
sudo enola-cli vpn create <name> [--port 51820] [--subnet 10.8.0.0/24] [--autostart] [--sync-firewall]

# List interfaces
sudo enola-cli vpn list

# Manage an interface
sudo enola-cli vpn start <name>
sudo enola-cli vpn stop <name>
sudo enola-cli vpn status <name>
sudo enola-cli vpn delete <name> [--force] [--sync-firewall]

# Manage peers
sudo enola-cli vpn peer add <interface> <peer> --endpoint <host> [--dns <ip>] [--psk] [--ip <ip>]
sudo enola-cli vpn peer add-pubkey <interface> <peer> <public_key> <ip>
sudo enola-cli vpn peer remove <interface> <public_key>
```

---

## 📦 Setup — Dependencies

Install system dependencies (Docker, Nginx, Tor, WireGuard, UFW, AppArmor).

```bash
# Install core dependencies
sudo enola-cli setup

# Install everything (core + VPN + security)
sudo enola-cli setup --all

# Install VPN only
sudo enola-cli setup --vpn

# Install security tools only (UFW, AppArmor)
sudo enola-cli setup --security

# Install the PQC TLS stack (OpenSSL 3.5 + Nginx)
sudo enola-cli setup --pqc-tls
```

---

## 🩺 Doctor — Dependency Check

Check which dependencies are installed and which are missing.

```bash
# Basic check
sudo enola-cli doctor

# Security audit (hardening, configs, secrets)
sudo enola-cli doctor --security
```

---

## 🔄 Update — Advisories

Security advisory feed and updates.

```bash
# Check for available updates
sudo enola-cli update check

# JSON output (CI)
sudo enola-cli update check --json

# Show the feed schema
sudo enola-cli update schema

# Verify a feed manually
sudo enola-cli update verify-feed <url-or-path>

# Download the latest version
sudo enola-cli update download

# Download and apply
sudo enola-cli update download --yes

# Apply an already-downloaded update
sudo enola-cli update apply [--binary <path>]
```

---

## 🔐 Verify — PQC Verification

Verify download authenticity with the ML-DSA-65 (FIPS 204) post-quantum signature.

```bash
# Verify a downloaded release
enola-cli verify enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz

# With an alternative signature and JSON output
enola-cli verify <file> --pqsig <signature.pqsig> --json
```

No network or external tools required — the public key is embedded in the binary.

---

## 🗑️ Uninstall

Cleanly uninstall Enola CLI from the system.

```bash
# Dry-run (deletes nothing, only lists)
sudo enola-cli uninstall

# Delete everything
sudo enola-cli uninstall --yes

# Preserve data
sudo enola-cli uninstall --yes --keep-data

# Only specific sections
sudo enola-cli uninstall --yes --only tor,nginx

# Also remove dependencies Enola installed
sudo enola-cli uninstall --yes --remove-deps
```

---

## 📚 Docs — Offline Documentation

Documentation embedded in the binary — works offline.

```bash
# Quickstart guide
enola-cli docs quickstart

# Command reference
enola-cli docs commands [GROUP]

# Key concepts
enola-cli docs concepts [TOPIC]

# FAQ
enola-cli docs faq [TERM]

# Usage examples
enola-cli docs examples [CASE]

# Search all documentation
enola-cli docs search TERM

# Advanced guides
enola-cli docs quantum-security
enola-cli docs verify-downloads
enola-cli docs security
enola-cli docs install-from-iso
```

---

## 📄 License Command

Shows the full proprietary license text.

```bash
enola-cli license
enola-cli license | less
```

---

## 📖 Quickref

Equivalence table between Docker commands and Enola CLI.

```bash
enola-cli quickref
```

---

## 🌐 Web — Local Dashboard

Start a local web dashboard to manage services from the browser.

```bash
sudo enola-cli web --port 8090
```

The server binds only to `127.0.0.1`. A random token is generated and shown in the terminal. Open `http://127.0.0.1:8090` and enter the token.

More details: [docs/user/web/README.md](docs/user/web/README.md)

---

## ⚙️ Config

Inspect and validate the centralized configuration (`config.toml`).

```bash
# Show effective configuration (with the source of each value)
enola-cli config-show

# JSON output (CI, jq)
enola-cli config-show --json

# Validate configuration (offline)
enola-cli config-validate

# Validate with HTTP reachability ping
enola-cli config-validate --reachable

# Structured JSON output
enola-cli config-validate --json
```

---

## 💡 Practical Examples

### Create an Anonymous Blog with WordPress

```bash
# 1. Create the WordPress site
sudo enola-cli wp create -n my-blog

# 2. Wait for it to start (may take 1-2 minutes)
sudo enola-cli wp status my-blog

# 3. Publish on Tor
sudo enola-cli wp publish my-blog

# 4. Get the .onion address
sudo enola-cli tor list
```

### Secure Git Server

```bash
# 1. Create a server with HTTPS
sudo enola-cli git create -n code --ssl

# 2. Create an admin user
sudo enola-cli git user create code -u admin -e admin@local.onion -p MyPassword123

# 3. Expose on Tor
sudo enola-cli git publish code --ssl

# 4. Get the .onion address
sudo enola-cli tor list
```

---

## 🔧 Troubleshooting

### Error: "Root privileges required"

```bash
# Solution: run with sudo
sudo enola-cli <command>
```

### Service not reachable via Tor

```bash
# 1. Check that Tor is active
sudo systemctl status tor

# 2. Check the service
sudo enola-cli tor list

# 3. View Tor logs
sudo enola-cli logs view tor -l 100
```

### Port in use

```bash
# Use auto-ports to find free ports
sudo enola-cli tor edit myservice --auto-ports

# Or specify manually
sudo enola-cli tor edit myservice -p 8081 -t 9000
```

### Full diagnostics

```bash
sudo enola-cli diag summary
sudo enola-cli diag resources
```

---

### Highlighted User Guides

- [docs/user/general/SECURITY.md](docs/user/general/SECURITY.md)
- [docs/user/general/concepts.md](docs/user/general/concepts.md)
- [docs/user/general/faq.md](docs/user/general/faq.md)
- [docs/user/guia/quickstart.md](docs/user/guia/quickstart.md)
- [docs/user/guia/install-from-iso.md](docs/user/guia/install-from-iso.md)
- [docs/user/verify/verify-downloads.md](docs/user/verify/verify-downloads.md)
- [docs/user/uninstall/uninstall.md](docs/user/uninstall/uninstall.md)

For AI assistants and LLM crawlers: see [llms.txt](llms.txt) and [llms-full.txt](llms-full.txt).

---

## 📄 License

**Enola CLI is source-visible proprietary software.** Copyright © 2026 Salvador Palma Rodriguez. All rights reserved.

- 📖 **Source-visible** — you may view, read and compile the source code for personal use.
- ✅ **Free for personal use** — the license is free for personal, non-commercial use.
- 🚫 **No redistribution** — redistributing, publishing, selling or making the software available to third parties is not permitted.
- 🚫 **No forks** — forking the software to create a competing product or service is not permitted.
- 🚫 **No business use** — commercial, corporate or revenue-generating use is not permitted.
- ⚠️ **No continuity guarantee** — the software may be discontinued at any time without notice.
- ⚠️ **No author liability** — the author is NOT responsible for the use of the software or any damages.
- 🛡️ **Coordinated disclosure** — vulnerabilities must be reported to the author within 72 hours. Public disclosure is **prohibited** until they are remediated.
- ⚖️ **Jurisdiction** — Spain / European Union.

Full license: [LICENSE](LICENSE) · Contact: salvadorpalmarodriguez@gmail.com

### Third-Party Licenses

This software uses third-party dependencies (Rust crates) licensed under MIT, Apache-2.0, BSD, ISC, MPL-2.0 and other permissive licenses. The full list of dependencies and their license texts is available at:
[THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt)

---

<div align="center">

**Enola CLI** — Privacy by design 🔐

[Documentation](docs/) · [Issues](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/issues) · [Releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases)

</div>
