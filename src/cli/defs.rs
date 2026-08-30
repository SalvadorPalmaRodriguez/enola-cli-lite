// CLI definitions — extracted from mod.rs for build.rs introspection.
// This file contains ONLY clap types + primitives (no crate-internal deps).
// build.rs includes it via #[path = "src/cli/defs.rs"] mod defs;

pub use clap::{Parser, Subcommand};

/// Enola CLI - Manage Tor hidden services, Git, CMS, and more
#[derive(Parser, Debug)]
#[command(name = "enola")]
#[command(author = "Enola Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "CLI for managing Tor hidden services, CMS, VPN, and more", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub format: String,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    // ── CONFIG-003: Overrides globales de URLs configurables ──
    // Prioridad resuelta: flag CLI > env var > ~/.enola/config.toml > default.
    // Cuando se pasan, se exportan como env vars en main.rs para que
    // los adapters (DistributionSettings, WebSettings) las lean.
    /// Base URL to download binaries (override ENOLA_BINARY_BASE_URL / config.toml [distribution])
    #[arg(long, global = true, value_name = "URL")]
    pub binary_base_url: Option<String>,

    /// Public URL of the project web (override ENOLA_WEB_URL / config.toml [web])
    #[arg(long, global = true, value_name = "URL")]
    pub web_url: Option<String>,

    /// Public URL of the documentation (override ENOLA_DOCS_URL / config.toml [web])
    #[arg(long, global = true, value_name = "URL")]
    pub docs_url: Option<String>,

    /// Tor SOCKS5 proxy URL for .onion requests (override ENOLA_TOR_SOCKS_PROXY / config.toml [http].tor_socks_proxy)
    /// QUAL-003: e.g. `socks5h://127.0.0.1:9150` for Tor Browser, or `socks5h://10.0.0.5:9050` for remote Tor.
    #[arg(long, global = true, value_name = "URL")]
    pub tor_socks: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    // ═══════════════════════════════════════════════════════════════════
    // TOR SERVICES (🧅 Tor & Hidden Services)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/tor/commands-tor.md
    /// Manage Tor hidden services
    #[command(subcommand)]
    Tor(TorCommands),

    // ═══════════════════════════════════════════════════════════════════
    // GIT SERVICES (🐙 Git Services)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/git/commands-git.md
    /// Manage Git servers (Forgejo)
    #[command(subcommand)]
    Git(GitCommands),

    // ═══════════════════════════════════════════════════════════════════
    // WORDPRESS (🌐 WordPress & Web)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/wp/commands-wp.md
    /// Manage WordPress sites
    #[command(subcommand)]
    Wp(WordPressCommands),

    // ═══════════════════════════════════════════════════════════════════
    // DRUPAL (🌐 Drupal CMS — DRUPAL-003)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/drupal/commands-drupal.md
    /// Manage Drupal sites (CMS)
    ///
    /// Stack: drupal:10-apache + mariadb:10.11.
    ///
    /// Examples:
    ///   sudo enola-cli drupal create --name myblog --http-port 8090
    ///   sudo enola-cli drupal status myblog
    ///   sudo enola-cli drupal delete myblog --force
    #[command(subcommand)]
    Drupal(DrupalCommands),

    // ═══════════════════════════════════════════════════════════════════
    // GHOST CMS (✍️ Ghost — Node.js + SQLite)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/ghost/commands-ghost.md
    /// Manage Ghost blogs (CMS) — CMS-GHOST-002
    ///
    /// Stack: ghost:5-alpine + SQLite embedded (no separate DB container).
    /// Internal port: 2368 (Ghost default).
    ///
    /// Examples:
    ///   sudo enola-cli ghost create --name myblog --http-port 8095
    ///   sudo enola-cli ghost status myblog
    ///   sudo enola-cli ghost delete myblog --force
    #[command(subcommand)]
    Ghost(GhostCommands),

    // ═══════════════════════════════════════════════════════════════════
    // MAGNOLIA CMS (CMS-MAGNOLIA-CLI — Tomcat + H2/Postgres)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/magnolia/commands-magnolia.md
    /// Manage Magnolia CMS instances (CMS-MAGNOLIA-CLI)
    ///
    /// Stack: magnolia-cms:6 (Tomcat-based, Java).
    /// Note: needs ≥4 GB RAM available on the host.
    ///
    /// Examples:
    ///   sudo enola-cli magnolia create --name mysite --http-port 8100
    ///   sudo enola-cli magnolia status mysite
    ///   sudo enola-cli magnolia delete mysite --force
    #[command(subcommand)]
    Magnolia(MagnoliaCommands),

    // ═══════════════════════════════════════════════════════════════════
    // STRAPI CMS (CMS-STRAPI-CLI — Node + Postgres 16)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/strapi/commands-strapi.md
    /// Manage Strapi headless CMS instances (CMS-STRAPI-CLI)
    ///
    /// Stack: enola/strapi:5.49.0 + postgres:16-alpine.
    /// Generates 6 secrets (0600) per instance: app keys, API token, JWT,
    /// admin JWT, transfer token, db password (SEC-EXT-DOCKER-040).
    ///
    /// Examples:
    ///   sudo enola-cli strapi create --name myapi --http-port 1337
    ///   sudo enola-cli strapi status myapi
    ///   sudo enola-cli strapi delete myapi --force
    #[command(subcommand)]
    Strapi(StrapiCommands),

    // ═══════════════════════════════════════════════════════════════════
    // WAGTAIL CMS (CMS-WAGTAIL-CLI — Python/Django + Postgres)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/wagtail/commands-wagtail.md
    /// Manage Wagtail CMS instances (CMS-WAGTAIL-CLI)
    ///
    /// Stack: wagtail (Python/Django) + postgres:16-alpine.
    ///
    /// Examples:
    ///   sudo enola-cli wagtail create --name mysite --http-port 8200
    ///   sudo enola-cli wagtail status mysite
    ///   sudo enola-cli wagtail delete mysite --force
    #[command(subcommand)]
    Wagtail(WagtailCommands),

    // ═══════════════════════════════════════════════════════════════════
    // FILE SERVICES (📂 File Shares)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/files/commands-files.md
    /// Manage file sharing services
    #[command(subcommand)]
    Files(FileCommands),

    // ═══════════════════════════════════════════════════════════════════
    // MAINTENANCE (🔧 Maintenance)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/maintenance/commands-maintenance.md
    /// System maintenance operations
    #[command(subcommand)]
    Maintenance(MaintenanceCommands),

    // ═══════════════════════════════════════════════════════════════════
    // DIAGNOSTICS (🩺 Diagnostics)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/diag/commands-diag.md
    /// System diagnostics and health checks
    #[command(subcommand)]
    Diag(DiagnosticsCommands),

    // ═══════════════════════════════════════════════════════════════════
    // TESTS (🧪 System Tests)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/test/commands-test.md
    /// Run system tests
    #[command(subcommand)]
    Test(TestCommands),

    // ═══════════════════════════════════════════════════════════════════
    // LOGS (📝 Logs)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/logs/commands-logs.md
    /// View and manage logs
    #[command(subcommand)]
    Logs(LogCommands),

    // ═══════════════════════════════════════════════════════════════════
    // PORTS (🔌 Port Management)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/ports/commands-ports.md
    /// Show all ports used by Enola services
    ///
    /// Displays a table of every port in use: service name, type, port number,
    /// protocol, interface binding, and role in the Tor→Nginx→App chain.
    /// Includes stopped containers (they retain Docker port bindings).
    ///
    /// Example:
    ///   sudo enola-cli ports list
    #[command(subcommand)]
    Ports(PortsCommands),

    // ═══════════════════════════════════════════════════════════════════
    // FIREWALL (🛡 UFW Firewall)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/firewall/commands-firewall.md
    /// Manage the UFW firewall (setup, status, allow, deny)
    ///
    /// Enola services bind to 127.0.0.1 and are protected by Tor.
    /// UFW is an extra layer for the host machine.
    ///
    /// Quick start:
    ///   sudo enola-cli firewall setup
    #[command(subcommand)]
    Firewall(FirewallCommands),

    // ═══════════════════════════════════════════════════════════════════
    // APPARMOR (🛡️ Sandboxing)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/apparmor/commands-apparmor.md
    /// Manage AppArmor sandboxing profiles (setup, status, mode)
    ///
    /// AppArmor restricts what each container/service can access.
    /// Complements UFW (network) with process-level isolation.
    ///
    /// Quick start:
    ///   sudo enola-cli apparmor setup
    ///   sudo enola-cli apparmor status
    #[command(subcommand)]
    Apparmor(AppArmorCommands),

    // ═══════════════════════════════════════════════════════════════════
    // VPN (🔒 WireGuard VPN — Tarea 162)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/vpn/commands-vpn.md
    /// Manage WireGuard VPN tunnels for secure remote access.
    ///
    /// Creates encrypted tunnels so trusted devices can reach
    /// your Enola services without exposing them to the internet.
    /// Complements Tor (anonymous access) with authenticated VPN access.
    ///
    /// Quick start:
    ///   sudo enola-cli vpn create myvpn
    ///   sudo enola-cli vpn peer add myvpn laptop --endpoint vpn.example.com
    ///   sudo enola-cli vpn status myvpn
    #[command(subcommand)]
    Vpn(VpnCommands),

    // ═══════════════════════════════════════════════════════════════════
    // SETUP & DOCTOR (🩺 System Dependencies — DEP-001..003)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/setup/commands-setup.md
    /// Install system dependencies (Docker, Nginx, Tor, WireGuard, UFW, AppArmor)
    ///
    /// Examples:
    ///   sudo enola-cli setup              # Install core dependencies
    ///   sudo enola-cli setup --all        # Install ALL dependencies
    ///   sudo enola-cli setup --vpn        # Install VPN (WireGuard) only
    ///   sudo enola-cli setup --security   # Install security tools (UFW, AppArmor)
    ///   sudo enola-cli setup --pqc-tls    # Install official OpenSSL 3.5 + Nginx PQC stack
    Setup {
        /// Install ALL dependencies (core + vpn + security)
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Install VPN dependencies (wireguard-tools)
        #[arg(long, default_value_t = false)]
        vpn: bool,
        /// Install security dependencies (UFW, AppArmor)
        #[arg(long, default_value_t = false)]
        security: bool,
        /// Install the official PQC TLS stack (OpenSSL 3.5.x source + Nginx linked against it)
        #[arg(long = "pqc-tls", default_value_t = false)]
        pqc_tls: bool,
    },

    /// DOC-SYNC: docs/user/general/commands.md
    /// Check system dependencies — shows what's installed and what's missing
    ///
    /// Examples:
    ///   sudo enola-cli doctor
    ///   sudo enola-cli doctor --security
    Doctor {
        /// Run security audit: verify container hardening, Nginx configs,
        /// AppArmor, UFW, and check for leaked secrets in env vars.
        #[arg(long, default_value_t = false)]
        security: bool,
    },

    // ═══════════════════════════════════════════════════════════════════
    // QUICKREF (📖 Quick Reference)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/general/commands.md
    /// Show quick reference: Docker commands vs Enola CLI equivalents
    Quickref,

    /// DOC-SYNC: docs/user/general/commands.md
    /// Show the full proprietary software license text
    ///
    /// Displays the complete Enola CLI license agreement embedded in the binary.
    /// Works offline — no network required.
    ///
    /// Examples:
    ///   enola-cli license
    ///   enola-cli license | less
    License,

    // ═══════════════════════════════════════════════════════════════════
    // VERIFY (🔐 Release authenticity — PQC-030)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/verify/verify-downloads.md
    /// Verify that a downloaded Enola release is legitimate (PQC-030).
    ///
    /// Comprueba que el archivo descargado está firmado con la clave
    /// post-cuántica ML-DSA-65 (FIPS 204) del proyecto, usando la clave pública
    /// embebida en este binario. No requiere red, login ni herramientas externas
    /// (no necesitas `enola-sign-pqc`, que es una herramienta de desarrollo).
    ///
    /// Si existe un archivo `<FILE>.sha256` junto al release, también verifica
    /// la integridad SHA-256.
    ///
    /// Ejemplos:
    ///   enola-cli verify enola-cli-v0.1.2-alpha-x86_64-linux.tar.gz
    ///   enola-cli verify mybinary --pqsig mybinary.pqsig --json
    Verify {
        /// Ruta al archivo descargado a verificar (tarball o binario).
        file: String,
        /// Ruta a la firma .pqsig (por defecto: <FILE>.pqsig).
        #[arg(long)]
        pqsig: Option<String>,
        /// Ruta a una clave pública ML-DSA alternativa (por defecto: clave embebida).
        #[arg(long)]
        pubkey: Option<String>,
        /// Emitir salida en formato JSON.
        #[arg(long)]
        json: bool,
    },

    /// DOC-SYNC: docs/user/uninstall/uninstall.md
    /// Desinstalar Enola CLI del sistema (UNINSTALL-003)
    ///
    /// Borra de forma limpia binario, servicios, contenedores, configs de
    /// Tor/Nginx/AppArmor/UFW/systemd y datos (/srv/enola-*, /opt/enola/).
    /// Por defecto ejecuta en modo --dry-run: solo lista lo que se borraría.
    ///
    /// Ejemplos:
    ///   sudo enola-cli uninstall                        # dry-run (no borra)
    ///   sudo enola-cli uninstall --yes                  # borrar todo
    ///   sudo enola-cli uninstall --yes --keep-data      # preserva /srv y configuración
    ///   sudo enola-cli uninstall --yes --only tor,nginx # solo estas secciones
    ///   sudo enola-cli uninstall --yes --remove-deps    # también borra deps que Enola instaló
    Uninstall {
        /// Confirmar y ejecutar el borrado (sin esto es dry-run)
        #[arg(long)]
        yes: bool,
        /// Conservar datos de servicios (/srv/enola-* y config.toml)
        #[arg(long)]
        keep_data: bool,
        /// Solo secciones indicadas (coma-separadas): binary,config,services,tor,nginx,systemd,apparmor,docker,ufw,data,deps
        #[arg(long)]
        only: Option<String>,
        /// Continuar ante errores no críticos (servicios no instalados)
        #[arg(long)]
        force: bool,
        /// Desinstalar dependencias de terceros que Enola instaló (según manifiesto)
        #[arg(long)]
        remove_deps: bool,
    },

    /// DOC-SYNC: docs/user/general/config-reference.md
    /// Inspeccionar la configuracin centralizada (CFG-NEW-001)
    ///
    /// Muestra el valor efectivo resuelto para cada clave junto con su fuente
    /// (flag > env > file > default) y la variable de entorno equivalente.
    /// Los valores sensibles (API keys, tokens, secrets) se muestran como
    /// `[REDACTED]`.
    ///
    /// EJEMPLOS:
    ///   enola-cli config-show             Tabla legible (default)
    ///   enola-cli config-show --json      Salida mquina (CI, jq)
    ///
    ConfigShow {
        /// Emitir la configuracin como JSON en vez de tabla ASCII
        #[arg(long)]
        json: bool,
    },

    /// DOC-SYNC: docs/user/general/config-reference.md
    /// Validar la configuracin centralizada (CFG-NEW-002)
    ///
    /// Ejecuta comprobaciones: TOML parseable, permisos 0600, sintaxis de URLs,
    /// Tor disponible si hay `.onion` en la config, y opcionalmente alcanzabilidad
    /// HTTP de `auth_url` y `web_public_url`.
    ///
    /// Devuelve exit code 1 si hay errores (CI-friendly). Warnings no bloquean.
    ///
    /// EJEMPLOS:
    ///   enola-cli config-validate                   Checks offline (rpidos)
    ///   enola-cli config-validate --reachable       Aade ping HTTP a las URLs
    ///   enola-cli config-validate --json            Salida mquina (CI, jq)
    ConfigValidate {
        /// Comprobar alcanzabilidad HTTP de URLs principales (ms lento).
        #[arg(long)]
        reachable: bool,
        /// Emitir los findings como JSON estructurado.
        #[arg(long)]
        json: bool,
    },

    // ═══════════════════════════════════════════════════════════════════
    // DOCS (📚 User Documentation)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/docs/commands-docs.md
    /// Consultar la documentación de uso directamente en el terminal
    ///
    /// La documentación está embebida en el binario — funciona offline.
    ///
    /// GUÍAS BÁSICAS:
    ///   docs quickstart              Guía de inicio rápido
    ///   docs commands [GRUPO]        Referencia de comandos
    ///   docs concepts [TEMA]         Conceptos clave
    ///   docs faq [TERMINO]           Preguntas frecuentes
    ///   docs examples [CASO]         Ejemplos de uso
    ///   docs search TERMINO          Buscar en toda la documentación
    ///
    /// GUÍAS AVANZADAS:
    ///   docs quantum-security        Seguridad post-cuántica
    ///   docs verify-downloads        Verificación de descargas
    #[command(subcommand)]
    Docs(DocsCommands),

    // ═══════════════════════════════════════════════════════════════════
    // UPDATE (🔄 update checker + advisory feed — UPD-CLI-001/002)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/update/commands-update.md
    /// Check for available updates and security advisories.
    ///
    /// Examples:
    ///   sudo enola-cli update check
    ///   sudo enola-cli update check --json
    #[command(subcommand)]
    Update(UpdateCommands),

    // ═══════════════════════════════════════════════════════════════════
    // WEB GUI (🌐 Embedded web dashboard)
    // ═══════════════════════════════════════════════════════════════════
    /// DOC-SYNC: docs/user/web/README.md
    /// Start a local web dashboard (GUI) for managing Enola services.
    ///
    /// The server binds to 127.0.0.1 only and requires a token shown in the terminal.
    ///
    /// Examples:
    ///   sudo enola-cli web --port 8090
    Web {
        /// Port to bind the web server (default: 8090)
        #[arg(short, long, default_value = "8090")]
        port: u16,
    },
}

/// UPD-CLI-001: subcomandos del comando `update`.
#[derive(Subcommand, Debug)]
pub enum UpdateCommands {
    /// Check for available updates and security advisories.
    ///
    /// Fetches the advisory feed (ENOLA_UPDATE_FEED_URL or [update].feed_url),
    /// compares with the current version, and shows if an update is available
    /// or if any security advisories affect your installation.
    ///
    /// Exit codes estables para scripts/CI:
    ///   0  = OK (incluye update disponible sin advisory crítico)
    ///   11 = advisory crítico afecta a la versión actual
    ///   12 = versión actual por debajo de min_supported
    ///   20 = feed inválido/no parseable/no alcanzable
    ///   21 = firma minisign inválida o ausente
    Check {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Ignore cache and force a fresh check.
        #[arg(long)]
        force: bool,
    },
    /// Show the current advisory feed schema (for operators creating their own feed).
    Schema {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Verify a signed advisory feed manually from URL or local path.
    ///
    /// Useful for operator debugging or offline checks:
    ///   sudo enola-cli update verify-feed web/releases/advisories.json
    ///   sudo enola-cli update verify-feed https://host/advisories.json --json
    VerifyFeed {
        /// Feed source (http/https URL or local file path).
        source: String,
        /// Optional signature source (URL/path). Default: <source>.minisig
        #[arg(long)]
        signature: Option<String>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Download the latest binary from the update feed.
    ///
    /// Fetches the advisory feed, extracts `download_url`, downloads the binary
    /// with its SHA256 and minisign signature, and verifies both.
    /// The binary is saved to a temporary directory for inspection.
    /// Use `--yes` to also apply (replace the current binary).
    /// Use `--dry-run` to show what would happen without downloading.
    Download {
        /// Also apply the update (replace current binary) after downloading.
        #[arg(long)]
        yes: bool,
        /// Show what would happen without downloading or applying.
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Force fresh feed check (ignore cache).
        #[arg(long)]
        force: bool,
        /// Allow applying without minisign signature verification (dangerous).
        #[arg(long)]
        allow_unsigned: bool,
    },
    /// Apply a previously downloaded update.
    ///
    /// Replaces the current binary atomically: backs up the old binary to
    /// `/usr/local/share/enola/enola-cli.bak`, moves the new binary into place,
    /// and updates `cli.sha256`. Requires root.
    Apply {
        /// Path to the downloaded binary to apply.
        /// If omitted, uses the most recent download from `update download`.
        #[arg(long)]
        binary: Option<String>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Allow applying without minisign signature verification (dangerous).
        #[arg(long)]
        allow_unsigned: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// TOR SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum TorCommands {
    /// List all Tor hidden services
    List,

    /// Create a new Tor hidden service
    ///
    /// Architecture depends on service type:
    /// - raw/tcp:  Tor → App (direct TCP, for SSH, databases)
    /// - web/proxy: Tor → Nginx → App (HTTP, recommended for web apps)
    /// - static:   Tor → Nginx (serving static files from /var/www/{name})
    /// - files:    Tor → Nginx (file server with directory listing)
    Create {
        /// Service name (alphanumeric + hyphens)
        #[arg(short, long)]
        name: String,

        /// Service type: raw, web, static, files
        ///
        /// - raw/tcp: Direct TCP connection (Tor → App)
        /// - web/proxy/http: HTTP via Nginx (Tor → Nginx → App) [RECOMMENDED for web]
        /// - static: Static website served by Nginx
        /// - files: File server with directory listing
        #[arg(short, long, default_value = "web")]
        service_type: String,

        /// Virtual port (public .onion port, usually 80)
        #[arg(short = 'p', long, default_value = "80")]
        virtual_port: u16,

        /// Target port (your local app port, e.g. 3000, 8080)
        #[arg(short, long)]
        target_port: Option<u16>,

        /// Enable HTTPS with self-signed certificate
        /// Creates both HTTP (port 80) and HTTPS (port 443) endpoints
        #[arg(long)]
        ssl: bool,
    },

    /// Start a Tor hidden service
    Start {
        /// Service name
        name: String,
    },

    /// Stop a Tor hidden service
    Stop {
        /// Service name
        name: String,
    },

    /// Remove a Tor hidden service
    Remove {
        /// Service name
        name: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Edit service ports
    ///
    /// Allows changing the port configuration of a Tor hidden service.
    ///
    /// Port relationships:
    /// - Virtual port: Public .onion port (e.g., 80 for http://xxx.onion/)
    /// - Nginx port: Internal port where Nginx listens (Tor forwards here)
    /// - Target/Backend port: Your application's port (Nginx proxies here)
    ///
    /// Flow: .onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT
    ///
    /// Examples:
    ///   Manual: enola-cli tor edit myservice -p 8081 -t 9000 -n 15000
    ///   Auto:   enola-cli tor edit myservice --auto-ports
    ///   Mixed:  enola-cli tor edit myservice -p 443 --auto-ports
    Edit {
        /// Service name
        name: String,

        /// New virtual port (public .onion port)
        /// If changed, Nginx port will be updated to match unless --nginx-port is specified
        #[arg(short = 'p', long)]
        virtual_port: Option<u16>,

        /// New Nginx listen port (internal, Tor forwards traffic here)
        /// Usually auto-assigned; only change if you have a specific need
        #[arg(short = 'n', long)]
        nginx_port: Option<u16>,

        /// New target/backend port (your application's port)
        #[arg(short, long)]
        target_port: Option<u16>,

        /// Automatically find available ports for Nginx and optionally backend
        /// Validates that ports are free before applying changes
        #[arg(long)]
        auto_ports: bool,
    },

    /// Rotate .onion address (generate new identity)
    Rotate {
        /// Service name
        name: String,
    },

    /// Manage client authorization
    #[command(subcommand)]
    Auth(TorAuthCommands),
}

#[derive(Subcommand, Debug)]
pub enum TorAuthCommands {
    /// List authorized clients for a service
    List {
        /// Service name
        service: String,
    },

    /// Enable client authorization for a service
    Enable {
        /// Service name
        service: String,
    },

    /// Disable client authorization for a service
    Disable {
        /// Service name
        service: String,
    },

    /// Add an authorized client (SERVER-SIDE operation)
    ///
    /// The operator adds a client's public key to the authorized_clients directory.
    /// The client should have generated their own keypair with 'tor auth generate'
    /// and sent only the public key.
    ///
    /// Flow:
    ///   1. Client generates keys: enola-cli tor auth generate --client alice
    ///   2. Client sends public key to operator (secure channel)
    ///   3. Operator runs: enola-cli tor auth add <service> --client alice --pubkey <key>
    Add {
        /// Service name
        service: String,

        /// Client name
        #[arg(short, long)]
        client: String,

        /// Client public key (x25519, base32, 52 chars)
        #[arg(short, long)]
        pubkey: String,
    },

    /// Revoke a client's authorization
    Revoke {
        /// Service name
        service: String,

        /// Client name to revoke
        #[arg(short, long)]
        client: String,
    },

    /// Generate a new client keypair (CLIENT-SIDE operation)
    ///
    /// Like SSH keys for GitHub/GitLab: you generate the keypair on YOUR machine.
    /// The private key NEVER leaves your computer. You send only the public key
    /// to the service operator, who adds you with 'tor auth add'.
    ///
    /// Flow:
    ///   1. Client: enola-cli tor auth generate --client alice
    ///   2. Client sends public key to operator (Signal, PGP, etc.)
    ///   3. Operator: enola-cli tor auth add <service> --client alice --pubkey <key>
    ///   4. Client imports private key in Tor Browser → Onion Services → Client auth
    ///
    /// ⚠️  X25519 keys are not quantum-resistant. Rotate periodically until
    ///    Tor supports post-quantum auth (PQC-043). See POST_QUANTUM_PLAN.md.
    Generate {
        /// Client name
        #[arg(short, long)]
        client: String,
    },

    /// Rotate client keypair — generates new X25519 keys and updates the server
    /// (PQC-013: mitigates harvest-now-decrypt-later by reducing key lifetime)
    ///
    /// The operator generates new keys for the client, updates the server with
    /// the new public key, and sends the new private key to the client via a
    /// secure channel. The old key is automatically revoked.
    Rotate {
        /// Service name
        service: String,

        /// Client name to rotate
        #[arg(short, long)]
        client: String,
    },
}

// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum GitCommands {
    /// List Git servers
    List,

    /// Crea un nuevo servidor Git (Forgejo).
    ///
    /// Dos modos de primer acceso:
    ///
    /// MODO CLI (recomendado) — admin creado automáticamente:
    ///   sudo enola-cli git create --name myrepo --admin-user alice --admin-password MiPass123
    ///
    /// MODO WEB — el usuario configura desde el navegador:
    ///   sudo enola-cli git create --name myrepo
    ///   → Abre http://localhost:<puerto>/ y rellena el formulario de instalación.
    Create {
        /// Nombre del servidor
        #[arg(short, long)]
        name: String,

        /// Habilitar HTTPS con certificado autofirmado
        #[arg(long)]
        ssl: bool,

        /// Puerto HTTP del contenedor Forgejo (cadena: Nginx → Docker:PORT → Forgejo).
        ///
        /// Este puerto es INTERNO — solo accesible desde localhost.
        /// El visitante nunca lo ve: accede via la dirección .onion.
        ///
        /// Rango válido: 10000-15000. Por defecto: auto (primer libre en el rango).
        ///
        /// Ejemplo: --http-port 10500
        #[arg(
            long,
            value_name = "PORT",
            help = "Puerto HTTP interno de Forgejo (auto: rango 10000-15000)"
        )]
        http_port: Option<u16>,

        /// Puerto SSH del contenedor Forgejo (cadena: Tor → Docker:PORT → Forgejo SSH).
        ///
        /// Permite clonar repos via SSH sobre Tor: git clone ssh://xxx.onion/repo
        /// Este puerto es INTERNO — solo accesible desde localhost.
        ///
        /// Rango válido: 30000-35000. Por defecto: auto (primer libre en el rango).
        ///
        /// Ejemplo: --ssh-port 30100
        #[arg(
            long,
            value_name = "PORT",
            help = "Puerto SSH interno de Forgejo (auto: rango 30000-35000)"
        )]
        ssh_port: Option<u16>,

        /// (Modo CLI) Nombre de usuario del administrador inicial.
        /// Si se omite, Forgejo mostrará el asistente web de instalación.
        #[arg(long, value_name = "USERNAME")]
        admin_user: Option<String>,

        /// (Modo CLI) Contraseña del administrador inicial.
        /// Requerida si se indica --admin-user.
        #[arg(long, value_name = "PASSWORD")]
        admin_password: Option<String>,
    },

    /// Start a Git server
    Start {
        /// Server name
        name: String,
    },

    /// Stop a Git server
    Stop {
        /// Server name
        name: String,
    },

    /// Show status of a Git server
    Status {
        /// Server name
        name: String,
    },

    /// Delete a Git server
    Delete {
        /// Server name
        name: String,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Enable or disable user self-registration on a Forgejo instance
    ///
    /// When enabled: anyone can sign up. When disabled: admin must create accounts.
    ///
    /// Examples:
    ///   sudo enola-cli git registration myrepos --enable
    ///   sudo enola-cli git registration myrepos --disable
    ///   sudo enola-cli git registration myrepos --status
    Registration {
        /// Server name
        name: String,

        /// Enable user self-registration
        #[arg(long)]
        enable: bool,

        /// Disable user self-registration
        #[arg(long)]
        disable: bool,

        /// Show current registration status without modifying it
        #[arg(long)]
        status: bool,
    },

    /// Edit service ports
    Edit {
        /// Service name
        name: String,

        /// New HTTP port (Nginx listen port if SSL enabled)
        #[arg(long)]
        http_port: Option<u16>,

        /// New HTTPS port (Nginx listen port, only if SSL enabled)
        #[arg(long)]
        https_port: Option<u16>,

        /// New SSH port on Host (mapped to container SSH)
        /// Note: Changing this might require container recreation in some contexts
        #[arg(long)]
        ssh_port: Option<u16>,

        /// Automatically find available ports
        #[arg(long)]
        auto_ports: bool,
    },

    /// Publish Git server on Tor (create hidden service)
    Publish {
        /// Server name
        name: String,

        /// Enable HTTPS with self-signed certificate
        #[arg(long)]
        ssl: bool,
    },

    /// Hide Git server from Tor (remove hidden service)
    Hide {
        /// Server name
        name: String,
    },

    /// Manage users
    #[command(subcommand)]
    User(GitUserCommands),

    /// Run the Git Pipeline Watcher (foreground)
    Watcher,
}

#[derive(Subcommand, Debug)]
pub enum GitUserCommands {
    /// Lista los usuarios del servidor Git.
    ///
    /// Si el servidor se creó con --admin-user las credenciales se leen automáticamente.
    /// Si se creó en modo web, pasa --admin-user y --admin-pass.
    List {
        /// Nombre del servidor
        server: String,

        /// Usuario admin de Forgejo (sólo necesario si el servidor se creó en modo web)
        #[arg(long, value_name = "USERNAME")]
        admin_user: Option<String>,

        /// Contraseña admin (sólo necesaria si el servidor se creó en modo web)
        #[arg(long, value_name = "PASSWORD")]
        admin_pass: Option<String>,
    },

    /// Crea un usuario en el servidor Git via API REST (modo automático).
    ///
    /// Para registro manual vía web (sin credenciales de admin):
    ///   1. sudo enola-cli git registration <server> --enable
    ///   2. El usuario abre http://localhost:<puerto>/user/sign_up en el navegador
    ///   3. Rellena nombre de usuario, email y contraseña en el formulario web
    ///   4. sudo enola-cli git registration <server> --disable  (recomendado después)
    Create {
        /// Nombre del servidor
        server: String,

        /// Nombre de usuario a crear
        #[arg(short, long)]
        username: String,

        /// Email del usuario
        #[arg(short, long)]
        email: String,

        /// Contraseña del usuario
        #[arg(short, long)]
        password: String,

        /// Dar permisos de administrador al usuario creado
        #[arg(long, default_value = "false")]
        admin: bool,

        /// Usuario admin de Forgejo (sólo necesario si el servidor se creó en modo web)
        #[arg(long, value_name = "USERNAME")]
        admin_user: Option<String>,

        /// Contraseña admin (sólo necesaria si el servidor se creó en modo web)
        #[arg(long, value_name = "PASSWORD")]
        admin_pass: Option<String>,
    },

    /// Elimina un usuario del servidor Git.
    Delete {
        /// Nombre del servidor
        server: String,

        /// Nombre de usuario a eliminar
        #[arg(short, long)]
        username: String,

        /// Usuario admin de Forgejo (sólo necesario si el servidor se creó en modo web)
        #[arg(long, value_name = "USERNAME")]
        admin_user: Option<String>,

        /// Contraseña admin (sólo necesaria si el servidor se creó en modo web)
        #[arg(long, value_name = "PASSWORD")]
        admin_pass: Option<String>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// WORDPRESS SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum WordPressCommands {
    /// List WordPress sites
    List,

    /// Create a new WordPress site
    Create {
        /// Site name
        #[arg(short, long)]
        name: String,

        /// Puerto HTTP del contenedor WordPress (cadena: Nginx → Docker:PORT → WordPress).
        ///
        /// Este puerto es INTERNO — solo accesible desde localhost.
        /// El visitante nunca lo ve: accede via la dirección .onion.
        ///
        /// Rango válido: 8080-9000. Por defecto: auto (primer libre en el rango).
        ///
        /// Ejemplo: --http-port 8090
        #[arg(
            long,
            value_name = "PORT",
            help = "Puerto HTTP interno de WordPress (auto: rango 8080-9000)"
        )]
        http_port: Option<u16>,
    },

    /// Start a WordPress site
    Start {
        /// Site name
        name: String,
    },

    /// Stop a WordPress site
    Stop {
        /// Site name
        name: String,
    },

    /// Restart a WordPress site
    Restart {
        /// Site name
        name: String,
    },

    /// Delete a WordPress site
    Delete {
        /// Site name
        name: String,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Update WordPress (with backup)
    Update {
        /// Site name
        name: String,
    },

    /// Edit WordPress configuration
    Config {
        /// Site name
        name: String,
    },

    /// Show WordPress status
    Status {
        /// Site name
        name: String,
    },

    /// Publish site on Tor (create hidden service)
    Publish {
        /// Site name
        name: String,
    },

    /// Hide site from Tor (remove hidden service)
    Hide {
        /// Site name
        name: String,
    },

    /// Edit WordPress site ports and SSL configuration
    Edit {
        /// Site name
        name: String,

        /// HTTP port (Nginx listen port)
        #[arg(long)]
        http_port: Option<u16>,

        /// HTTPS port (Nginx SSL listen port)
        #[arg(long)]
        https_port: Option<u16>,

        /// Enable or disable SSL
        #[arg(long)]
        ssl: Option<bool>,

        /// Auto-detect available ports
        #[arg(long)]
        auto_ports: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// DRUPAL SUBCOMMANDS — DRUPAL-003 (clonado de WordPressCommands)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum DrupalCommands {
    /// List Drupal sites
    List,

    /// Create a new Drupal site
    ///
    /// Stack: drupal:10-apache + mariadb:10.11.
    /// Data root: /srv/enola-drupal/{name}/{web,db,secrets}/
    Create {
        /// Site name (alphanumeric + `_-`)
        #[arg(short, long)]
        name: String,

        /// Internal HTTP port (Nginx → Docker:PORT → Drupal Apache).
        ///
        /// INTERNAL — only reachable from 127.0.0.1. Visitors hit the .onion.
        /// Required for now: auto-allocation lives in the CLI layer (DRUPAL-003+).
        #[arg(long, value_name = "PORT")]
        http_port: u16,
    },

    /// Start a Drupal site (DB first, then web)
    Start {
        /// Site name
        name: String,
    },

    /// Stop a Drupal site (web first, then DB)
    Stop {
        /// Site name
        name: String,
    },

    /// Delete a Drupal site (containers, network; data preserved unless --purge)
    Delete {
        /// Site name
        name: String,

        /// Skip the running-state check
        #[arg(short, long)]
        force: bool,
    },

    /// Show Drupal site status (running/stopped/initializing/not-found + port)
    Status {
        /// Site name
        name: String,
    },

    /// Publish site on Tor (create hidden service)
    Publish {
        /// Site name
        name: String,
    },

    /// Hide site from Tor (remove hidden service)
    Hide {
        /// Site name
        name: String,
    },

    /// Edit Drupal site HTTP port (recreates the web container atomically)
    ///
    /// Docker no permite reasignar port bindings en caliente — `drupal edit`
    /// recrea el contenedor `drupal-{name}` preservando imagen, env, volumen
    /// `/var/www/html`, network y secret mount. El contenedor de BD no se toca
    /// (3306 es interno a la network). Si el sitio está publicado en Tor,
    /// el HiddenServicePort se reactualiza automáticamente.
    Edit {
        /// Site name
        name: String,

        /// New HTTP port (required — must be free at SO + Docker level)
        #[arg(long)]
        http_port: Option<u16>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// GHOST SUBCOMMANDS — CMS-GHOST-002
//
// Mismo lifecycle que Drupal pero con UN solo contenedor (SQLite embebido).
// Stack: ghost:5-alpine, puerto interno 2368.
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum GhostCommands {
    /// List Ghost blogs
    List,

    /// Create a new Ghost blog
    ///
    /// Stack: ghost:5-alpine + SQLite embedded.
    /// Data root: /srv/enola-ghost/{name}/content/
    /// Internal port (container): 2368.
    Create {
        /// Blog name (alphanumeric + `_-`)
        #[arg(short, long)]
        name: String,

        /// Internal HTTP port mapped to Ghost's 2368.
        ///
        /// INTERNAL — only reachable from 127.0.0.1. Visitors hit the .onion.
        #[arg(long, value_name = "PORT")]
        http_port: u16,
    },

    /// Start a Ghost blog
    Start {
        /// Blog name
        name: String,
    },

    /// Stop a Ghost blog
    Stop {
        /// Blog name
        name: String,
    },

    /// Delete a Ghost blog (container + network; data preserved on /srv unless purged)
    Delete {
        /// Blog name
        name: String,

        /// Skip the running-state check
        #[arg(short, long)]
        force: bool,
    },

    /// Show Ghost blog status (running/stopped/initializing/not-found + port)
    Status {
        /// Blog name
        name: String,
    },

    /// Publish blog on Tor (create hidden service)
    Publish {
        /// Blog name
        name: String,
    },

    /// Hide blog from Tor (remove hidden service)
    Hide {
        /// Blog name
        name: String,
    },

    /// Edit Ghost blog HTTP port (atomic container recreation)
    Edit {
        /// Blog name
        name: String,

        /// New HTTP port (host side; container side stays at 2368)
        #[arg(long)]
        http_port: Option<u16>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// MAGNOLIA SUBCOMMANDS (CMS-MAGNOLIA-CLI)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum MagnoliaCommands {
    /// List Magnolia instances
    List,

    /// Create a new Magnolia instance
    ///
    /// Stack: magnolia-cms:6 (Tomcat). Needs ≥4 GB RAM.
    /// Container prefix: `magnolia-`.
    Create {
        /// Instance name (alphanumeric + `_-`)
        #[arg(short, long)]
        name: String,

        /// Internal HTTP port (host side, mapped to Tomcat 8080).
        ///
        /// INTERNAL — only reachable from 127.0.0.1. Visitors hit the .onion.
        #[arg(long, value_name = "PORT")]
        http_port: u16,
    },

    /// Start a Magnolia instance
    Start { name: String },

    /// Stop a Magnolia instance
    Stop { name: String },

    /// Delete a Magnolia instance (data preserved on /srv unless purged)
    Delete {
        name: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show Magnolia instance status
    Status { name: String },

    /// Publish Magnolia instance on Tor (create hidden service)
    Publish { name: String },

    /// Hide Magnolia instance from Tor (remove hidden service)
    Hide { name: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// STRAPI SUBCOMMANDS (CMS-STRAPI-CLI)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum StrapiCommands {
    /// List Strapi instances
    List,

    /// Create a new Strapi instance (Node + Postgres 16)
    ///
    /// Generates 5 secrets (0600 perms) per instance.
    /// Container prefix: `strapi-`.
    Create {
        /// Instance name (alphanumeric + `_-`)
        #[arg(short, long)]
        name: String,

        /// Internal HTTP port mapped to Strapi's 1337.
        ///
        /// INTERNAL — only reachable from 127.0.0.1.
        #[arg(long, value_name = "PORT")]
        http_port: u16,
    },

    /// Start a Strapi instance (web + db containers)
    Start { name: String },

    /// Stop a Strapi instance (web + db)
    Stop { name: String },

    /// Delete a Strapi instance
    Delete {
        name: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show Strapi instance status
    Status { name: String },

    /// Publish Strapi instance on Tor (create hidden service)
    Publish { name: String },

    /// Hide Strapi instance from Tor (remove hidden service)
    Hide { name: String },

    /// Build the Strapi production Docker image (multi-stage, Node 20, Strapi 5.49).
    ///
    /// Must be run once before `create`. The Dockerfile is embedded in the binary
    /// and extracted automatically. Build takes ~5-10 minutes.
    BuildImage {
        /// Force rebuild even if image already exists locally.
        #[arg(short, long)]
        force: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// WAGTAIL SUBCOMMANDS (CMS-WAGTAIL-CLI)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum WagtailCommands {
    /// List Wagtail instances
    List,

    /// Create a new Wagtail instance (Python/Django + Postgres)
    ///
    /// Container prefix: `wagtail-`.
    Create {
        /// Instance name (alphanumeric + `_-`)
        #[arg(short, long)]
        name: String,

        /// Internal HTTP port mapped to Wagtail's 8000.
        ///
        /// INTERNAL — only reachable from 127.0.0.1.
        #[arg(long, value_name = "PORT")]
        http_port: u16,
    },

    /// Start a Wagtail instance (web + db)
    Start { name: String },

    /// Stop a Wagtail instance (web + db)
    Stop { name: String },

    /// Delete a Wagtail instance
    Delete {
        name: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show Wagtail instance status
    Status { name: String },

    /// Publish Wagtail instance on Tor (create hidden service)
    Publish { name: String },

    /// Hide Wagtail instance from Tor (remove hidden service)
    Hide { name: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// FILE SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum FileCommands {
    /// List all file shares (name, onion address, Nginx port, status)
    ///
    /// Shows every active file share managed by Enola, including its .onion
    /// address and the local Nginx port it listens on.
    ///
    /// Examples:
    ///   sudo enola-cli files list
    ///   sudo enola-cli files list --format json
    List,

    /// Create a new anonymous file share accessible via Tor .onion
    ///
    /// Creates a Nginx autoindex server pointing to /srv/enola-files/<name>
    /// and exposes it as a Tor hidden service.
    ///
    /// Architecture (HTTP):  .onion:80  → Nginx:[auto-port] → /srv/enola-files/<name>
    /// Architecture (HTTPS): .onion:80  → Nginx:[auto-port] → /srv/enola-files/<name>
    ///                        .onion:443 → Nginx:[auto-port SSL] → /srv/enola-files/<name>
    ///
    /// After creation, place files in /srv/enola-files/<name> to serve them.
    ///
    /// Examples:
    ///   sudo enola-cli files create --name myshare
    ///   sudo enola-cli files create --name myshare --ssl
    ///   sudo enola-cli files create --name myshare --auth
    ///   sudo enola-cli files create --name myshare --ssl --auth
    ///   sudo enola-cli files create --name myshare
    Create {
        /// Name of the file share (used as directory name under /srv/enola-files/)
        #[arg(short, long)]
        name: String,

        /// Enable Tor client authorization (only authorised clients can access the .onion)
        #[arg(short, long)]
        auth: bool,

        /// Enable HTTPS: generates a self-signed certificate and adds a second Nginx
        /// listener (TLSv1.3 only — PQC-safe). The .onion expone tanto :80 (HTTP) como :443 (HTTPS).
        #[arg(long)]
        ssl: bool,
    },

    /// Edit file share configuration
    ///
    /// Without --port: shows the current port configuration (read-only, safe to run).
    /// With --port <p>: updates the internal Nginx listening port and reloads Nginx.
    /// The .onion address stays the same — only the internal Nginx port changes.
    ///
    /// Examples:
    ///   sudo enola-cli files edit myshare                 # show current config
    ///   sudo enola-cli files edit myshare --port 18080    # change Nginx port
    Edit {
        /// Name of the file share to edit
        name: String,

        /// New Nginx listening port. Omit to display current configuration.
        #[arg(short = 'p', long, value_name = "PORT")]
        port: Option<u16>,
    },

    /// Delete a file share (removes Nginx config and Tor hidden service)
    ///
    /// Removes the Nginx site config and the Tor hidden service configuration.
    /// The shared directory under /srv/enola-files/<name> is NOT deleted automatically
    /// to prevent accidental data loss — remove it manually if needed.
    ///
    /// Requires --force to prevent accidental deletion.
    ///
    /// Examples:
    ///   sudo enola-cli files delete myshare --force
    Delete {
        /// Name of the file share to delete
        name: String,

        /// Required confirmation flag — prevents accidental deletion
        #[arg(short, long)]
        force: bool,
    },

    /// Fix ownership and permissions on a file share directory
    ///
    /// Sets ownership to root:www-data and permissions to 750 on the
    /// /srv/enola-files/<name> directory so Nginx can read the files.
    /// Run this after manually copying files into the share directory.
    ///
    /// Examples:
    ///   sudo enola-cli files fix-perms myshare
    FixPerms {
        /// Name of the file share
        name: String,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// MAINTENANCE SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum MaintenanceCommands {
    /// Show system status
    Status,

    /// Run smoke test
    SmokeTest,

    /// Enable automatic health checks
    EnableChecks,

    /// Disable automatic health checks
    DisableChecks,

    /// Show timer status for automatic checks
    TimerStatus,

    /// Configure SSH check
    SshConfig,

    /// Harden SSH host configuration with post-quantum-safe algorithms (PQC-012)
    /// Adds sntrup761x25519-sha512 KEX (OpenSSH 9.0+) as first preferred algorithm.
    /// This is a TRANSITIONAL measure until full PQC is standardized.
    /// Run again after OpenSSH upgrade to apply improved algorithms.
    SshHardenPqc {
        /// Apply changes without asking for confirmation
        #[arg(short, long)]
        force: bool,

        /// Show what would change without applying (dry run)
        #[arg(long)]
        dry_run: bool,
    },

    /// Create system backup
    Backup,

    /// Cleanup temporary files and residual data
    Cleanup {
        /// Target to clean: all, logs, docker
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Dry run: show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,

        /// Force cleanup without confirmation
        #[arg(short, long)]
        force: bool,

        /// Days to keep logs (default: 7)
        #[arg(long, default_value = "7")]
        keep_days: u32,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTICS SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum DiagnosticsCommands {
    /// Show all services summary
    Summary,

    /// Check NGINX status
    Nginx,

    /// Check Tor status
    Tor,

    /// Check SSH status
    Ssh,

    /// Check WordPress status
    WordPress,

    /// Check WordPress/NGINX sync
    WpSync,

    /// Test NGINX configuration
    NginxTest,

    /// Show system resources (RAM, Disk, GPU)
    Resources,
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum TestCommands {
    /// Run all system tests
    Run {
        /// Test filter (optional)
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// List available tests
    List,

    /// Run benchmarks
    Benchmark,

    /// Show last test results
    Results,

    /// Clean test artifacts
    Clean,
}

// ═══════════════════════════════════════════════════════════════════════════
// LOG SUBCOMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum LogCommands {
    /// List available log sources
    List,

    /// View logs from a specific source
    View {
        /// Log source (system, tor, nginx, docker, ai-<name>, etc.)
        source: String,

        /// Number of lines
        #[arg(short, long, default_value = "50")]
        lines: usize,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },

    /// View installation logs
    Install,

    /// View smoke test logs
    SmokeTest,
}

// ═══════════════════════════════════════════════════════════════════════════
// PORTS SUBCOMMANDS (🔌 Port Management)
// NOTE: BundleCommands / AlertCommands / SettingsCommands eliminados TEST-ORPHAN-001 2026-06-06.

#[derive(Subcommand, Debug)]
pub enum PortsCommands {
    /// List all ports used by Enola services
    ///
    /// Shows every port in the Tor→Nginx→App chain for all services.
    /// Includes running AND stopped containers (stopped containers
    /// still retain their Docker port bindings).
    ///
    /// Columns: Service | Type | Role | Port | Interface | Status
    ///
    /// Roles:
    ///   onion-http   = virtual port in .onion URL (visitor sees this)
    ///   nginx-listen = Nginx listen port (Tor→Nginx, internal)
    ///   backend      = Docker container port (Nginx→App, internal)
    ///   ssh          = SSH port for Git services
    ///
    /// Example:
    ///   sudo enola-cli ports list
    ///   sudo enola-cli ports list --json
    List {
        /// Output in JSON format
        #[arg(long, help = "Output as JSON instead of table")]
        json: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// FIREWALL SUBCOMMANDS (🛡 UFW)

#[derive(Subcommand, Debug)]
pub enum FirewallCommands {
    /// Configure UFW with a secure default policy
    ///
    /// Applies: deny incoming, allow outgoing, allow SSH, configure DOCKER-USER chain.
    /// The DOCKER-USER chain prevents Docker containers from bypassing UFW.
    ///
    /// Examples:
    ///   sudo enola-cli firewall setup
    ///   sudo enola-cli firewall setup --ssh-port 2222
    ///   sudo enola-cli firewall setup --force
    Setup {
        /// SSH port to keep open (default: 22).
        /// Change this if your SSH runs on a non-standard port.
        #[arg(
            long,
            default_value = "22",
            help = "SSH port to keep open (anti-lockout)"
        )]
        ssh_port: u16,

        /// Skip confirmation prompt (for scripts and automation)
        #[arg(long, short, help = "Skip interactive confirmation")]
        force: bool,
    },

    /// Show current firewall status and rules
    ///
    /// Shows: active/inactive, default policies, rules, DOCKER-USER chain status.
    ///
    /// Example:
    ///   sudo enola-cli firewall status
    Status,

    /// Allow traffic on a port
    ///
    /// Examples:
    ///   sudo enola-cli firewall allow --port 443
    ///   sudo enola-cli firewall allow --port 8080 --proto tcp
    ///   sudo enola-cli firewall allow --port 5432 --from 192.168.1.0/24
    Allow {
        /// Port number to allow (1-65535)
        #[arg(long, short, help = "Port number to open")]
        port: u16,

        /// Protocol: tcp, udp, both (default: tcp)
        #[arg(long, default_value = "tcp", help = "Protocol: tcp, udp, or both")]
        proto: String,

        /// Source IP or CIDR (default: anywhere)
        ///
        /// Examples: 192.168.1.1, 10.0.0.0/8, 192.168.0.0/24
        #[arg(long, help = "Restrict to source IP or CIDR (default: anywhere)")]
        from: Option<String>,
    },

    /// Deny traffic on a port
    ///
    /// Note: Enola services bind to 127.0.0.1 — UFW rules don't affect them.
    /// This command is for external ports on the host machine.
    ///
    /// Examples:
    ///   sudo enola-cli firewall deny --port 23
    ///   sudo enola-cli firewall deny --port 3306 --proto tcp
    Deny {
        /// Port number to deny (1-65535)
        #[arg(long, short, help = "Port number to close")]
        port: u16,

        /// Protocol: tcp, udp, both (default: tcp)
        #[arg(long, default_value = "tcp", help = "Protocol: tcp, udp, or both")]
        proto: String,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// APPARMOR SUBCOMMANDS (🛡️ Sandboxing)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug)]
pub enum AppArmorCommands {
    /// Load base AppArmor profiles (nginx, tor, docker-base)
    ///
    /// First-time setup. Loads system-level profiles.
    /// Per-service profiles are created automatically with git/wp create.
    ///
    /// Examples:
    ///   sudo enola-cli apparmor setup
    ///   sudo enola-cli apparmor setup --mode enforce
    ///   sudo enola-cli apparmor setup --force
    Setup {
        /// Profile mode: complain (log only) or enforce (block + log).
        /// Recommended: start with complain, switch to enforce after validation.
        #[arg(long, default_value = "complain", help = "Mode: complain or enforce")]
        mode: String,

        /// Skip confirmation prompt
        #[arg(long, short, help = "Skip interactive confirmation")]
        force: bool,
    },

    /// Show AppArmor status: installed, enabled, profiles, violations
    ///
    /// Example:
    ///   sudo enola-cli apparmor status
    Status,

    /// Change mode of AppArmor profiles (enforce/complain/disable)
    ///
    /// Without --profile, changes ALL Enola profiles.
    ///
    /// Examples:
    ///   sudo enola-cli apparmor mode --enforce
    ///   sudo enola-cli apparmor mode --complain --profile enola-git-myserver
    ///   sudo enola-cli apparmor mode --disable --profile enola-git-myserver
    Mode {
        /// Set all/specific profiles to enforce mode
        #[arg(long, conflicts_with_all = ["complain", "disable"], help = "Block violations")]
        enforce: bool,

        /// Set all/specific profiles to complain mode
        #[arg(long, conflicts_with_all = ["enforce", "disable"], help = "Log only, don't block")]
        complain: bool,

        /// Disable (unload) a specific profile
        #[arg(long, conflicts_with_all = ["enforce", "complain"], help = "Unload profile")]
        disable: bool,

        /// Target a specific profile (default: all Enola profiles)
        #[arg(long, help = "Profile name (e.g., enola-git-myserver)")]
        profile: Option<String>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// VPN SUBCOMMANDS (🔒 WireGuard — Tarea 162)
// ═══════════════════════════════════════════════════════════════════════════

/// WireGuard VPN commands (Tarea 162)
#[derive(Subcommand, Debug)]
pub enum VpnCommands {
    /// Create a new WireGuard VPN interface and start it.
    ///
    /// Generates a key pair, writes /etc/wireguard/{name}.conf,
    /// and brings the interface up with wg-quick.
    ///
    /// Examples:
    ///   sudo enola-cli vpn create wg0
    ///   sudo enola-cli vpn create myvpn --port 51821 --subnet 10.9.0.0/24
    ///   sudo enola-cli vpn create myvpn --autostart
    ///   sudo enola-cli vpn create myvpn --port 51821 --sync-firewall
    Create {
        /// Interface name (max 15 chars, e.g. wg0, enola-vpn)
        name: String,

        /// UDP listen port (default: 51820)
        #[arg(long, short)]
        port: Option<u16>,

        /// VPN subnet in CIDR notation (default: 10.8.0.0/24)
        #[arg(long, short = 'n')]
        subnet: Option<String>,

        /// Enable systemd autostart on boot (wg-quick@{name})
        #[arg(long, short = 'a')]
        autostart: bool,

        /// Automatically add/remove UFW rule for the VPN UDP port.
        /// When set, `ufw allow <port>/udp` is added on create.
        /// When not set (default), you must manage the firewall rule manually.
        #[arg(long)]
        sync_firewall: bool,
    },

    /// Start a stopped WireGuard interface (wg-quick up).
    ///
    /// Example:
    ///   sudo enola-cli vpn start wg0
    Start {
        /// Interface name
        name: String,
    },

    /// Stop a running WireGuard interface (wg-quick down).
    ///
    /// Example:
    ///   sudo enola-cli vpn stop wg0
    Stop {
        /// Interface name
        name: String,
    },

    /// Show status of a WireGuard interface (connected peers, traffic).
    ///
    /// Examples:
    ///   sudo enola-cli vpn status wg0
    ///   sudo enola-cli vpn status myvpn
    Status {
        /// Interface name
        name: String,
    },

    /// List all WireGuard interfaces on this system.
    ///
    /// Example:
    ///   sudo enola-cli vpn list
    List,

    /// Delete a WireGuard interface (stop + remove config).
    ///
    /// This is irreversible — all peer configs will be lost.
    /// Use --force to skip confirmation.
    ///
    /// Example:
    ///   sudo enola-cli vpn delete wg0 --force
    ///   sudo enola-cli vpn delete wg0 --force --sync-firewall
    Delete {
        /// Interface name
        name: String,

        /// Skip confirmation
        #[arg(long, short)]
        force: bool,

        /// Automatically remove the UFW rule for the VPN UDP port.
        /// When set, the `ufw allow <port>/udp` rule added on create is removed.
        #[arg(long)]
        sync_firewall: bool,
    },

    /// Manage VPN peers (clients).
    #[command(subcommand)]
    Peer(VpnPeerCommands),
}

/// VPN peer management subcommands
#[derive(Subcommand, Debug)]
pub enum VpnPeerCommands {
    /// Add a new peer to a VPN interface.
    ///
    /// Generates a new key pair for the peer and prints the client .conf
    /// ready to be copied to the remote device or scanned as QR code.
    ///
    /// Examples:
    ///   sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
    ///   sudo enola-cli vpn peer add wg0 phone --endpoint 1.2.3.4 --dns 1.1.1.1
    ///   sudo enola-cli vpn peer add wg0 server --endpoint myhostname.com --psk
    Add {
        /// Interface name
        interface: String,

        /// Peer name (e.g., laptop, phone, coworker)
        peer_name: String,

        /// Server hostname or IP for clients to connect to
        #[arg(long, short)]
        endpoint: String,

        /// Optional: DNS servers for this peer (e.g., 1.1.1.1)
        #[arg(long)]
        dns: Option<String>,

        /// Add an extra preshared key for post-quantum security
        #[arg(long)]
        psk: bool,

        /// Optional: assign specific IP (auto-assigned if omitted)
        #[arg(long)]
        ip: Option<String>,
    },

    /// Add a peer using their existing public key (client manages own keys).
    ///
    /// Example:
    ///   sudo enola-cli vpn peer add-pubkey wg0 myserver PUBKEY_BASE64 10.8.0.5
    AddPubkey {
        /// Interface name
        interface: String,

        /// Peer name
        peer_name: String,

        /// WireGuard public key (base64, 44 chars)
        public_key: String,

        /// IP to assign in the VPN subnet (e.g., 10.8.0.5)
        ip: String,
    },

    /// Remove a peer from a VPN interface by public key.
    ///
    /// Example:
    ///   sudo enola-cli vpn peer remove wg0 PUBKEY_BASE64
    Remove {
        /// Interface name
        interface: String,

        /// WireGuard public key of the peer to remove
        public_key: String,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// DEV SUBCOMMANDS — Solo con --features testing
// ═══════════════════════════════════════════════════════════════════════════
/// Subcomandos de desarrollo y testing.
/// Solo compilados cuando se usa `--features testing`.
/// No disponibles en binarios de producción.
#[cfg(not(feature = "testing"))]
#[derive(clap::Subcommand, Debug)]
#[doc(hidden)]
pub enum DevCommands {}
#[cfg(feature = "testing")]
#[derive(clap::Subcommand, Debug)]
pub enum DevCommands {
    /// Genera un token de test HMAC-SHA256 (TTL: 5 minutos)
    ///
    /// El token permite a los scripts de test E2E saltar la verificación
    /// de licencia sin usar la antigua ENOLA_SKIP_AUTH (eliminada por brecha de seguridad).
    ///
    /// Uso en scripts de test:
    ///   export ENOLA_TEST_TOKEN=$(sudo enola-cli dev test-token)
    ///   sudo -E enola-cli wp create --name test-site
    ///
    /// El token expira en 5 minutos. Genera uno nuevo al inicio de cada suite de tests.
    TestToken,
    /// Inicializa la clave de test en ~/.enola/test.key
    ///
    /// La clave se crea automáticamente al generar el primer token.
    /// Usa este comando para regenerarla manualmente (invalida todos los tokens anteriores).
    SetupTestKey,
    /// Verifica si el token de test actual (ENOLA_TEST_TOKEN) es válido
    ///
    /// Útil para depurar problemas de autenticación en tests.
    VerifyToken {
        /// Token a verificar (si se omite, lee ENOLA_TEST_TOKEN del entorno)
        #[arg(long)]
        token: Option<String>,
    },
}
// ═══════════════════════════════════════════════════════════════════════════
// DOCS SUBCOMMANDS (📚 User Documentation)
// ═══════════════════════════════════════════════════════════════════════════
/// Subcomandos para consultar la documentación de uso embebida.
#[derive(Subcommand, Debug)]
pub enum DocsCommands {
    /// Muestra la guía de inicio rápido (configuración, primer servicio)
    ///
    /// Ejemplo:
    ///   sudo enola-cli docs quickstart
    Quickstart,
    /// Muestra la referencia de todos los comandos, con ejemplos
    ///
    /// Filtra por grupo si se especifica: tor, git, wp, files, ports,
    /// firewall, diag, maintenance
    ///
    /// Ejemplos:
    ///   sudo enola-cli docs commands
    ///   sudo enola-cli docs commands tor
    Commands {
        /// Grupo de comandos (tor, git, wp, files, ports, etc.)
        /// Si se omite, muestra todos los comandos.
        group: Option<String>,
    },
    /// Explica conceptos clave de Enola
    ///
    /// Temas disponibles: tor, ports, vpn, apparmor, docker, cms, pqc, advisories
    ///
    /// Ejemplos:
    ///   sudo enola-cli docs concepts
    ///   sudo enola-cli docs concepts tor
    Concepts {
        /// Tema concreto (tor, ports)
        /// Si se omite, muestra todos los conceptos.
        topic: Option<String>,
    },
    /// Muestra preguntas frecuentes y solución de problemas
    ///
    /// Puedes filtrar por término para encontrar respuestas concretas.
    ///
    /// Ejemplos:
    ///   sudo enola-cli docs faq
    ///   sudo enola-cli docs faq sudo
    ///   sudo enola-cli docs faq onion
    Faq {
        /// Término de búsqueda para filtrar la FAQ
        filter: Option<String>,
    },
    /// Muestra ejemplos de uso por caso de uso
    ///
    /// Casos disponibles: deploy, git-server, wordpress, backup, firewall, files, cms, vpn, apparmor, update, web
    ///
    /// Ejemplos:
    ///   sudo enola-cli docs examples
    ///   sudo enola-cli docs examples wordpress
    Examples {
        /// Caso de uso específico
        case: Option<String>,
    },
    /// Busca un término en toda la documentación embebida
    ///
    /// Devuelve todos los párrafos que contienen el término buscado,
    /// organizados por sección de documentación.
    ///
    /// Ejemplos:
    ///   sudo enola-cli docs search wordpress
    ///   sudo enola-cli docs search "hidden service"
    ///   sudo enola-cli docs search tor
    Search {
        /// Término a buscar en la documentación
        term: String,
    },
    /// Muestra la guía de seguridad post-cuántica y plan PQC
    ///
    /// Ejemplo:
    ///   sudo enola-cli docs quantum-security
    #[command(name = "quantum-security")]
    QuantumSecurity,
    /// Muestra la guía para verificar autenticidad de descargas con minisign
    ///
    /// Ejemplo:
    ///   sudo enola-cli docs verify-downloads
    #[command(name = "verify-downloads")]
    VerifyDownloads,
    /// Muestra el modelo de seguridad orientado al usuario
    ///
    /// Ejemplo:
    ///   sudo enola-cli docs security
    #[command(name = "security")]
    Security,
    /// Muestra la guía de instalación desde ISO
    ///
    /// Ejemplo:
    ///   sudo enola-cli docs install-from-iso
    #[command(name = "install-from-iso")]
    InstallFromIso,
}
