# Enola CLI

**Rust CLI for self-hosting Tor hidden services (.onion), Git servers (Forgejo), CMS (WordPress, Drupal, Ghost, Strapi, Wagtail, Magnolia), file sharing, WireGuard VPN, UFW firewall and AppArmor sandboxing on Debian/Linux — with post-quantum signed releases (ML-DSA).**

[Repository](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite) · [Releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases) · [Security Policy](security-model.md) · [Verify Downloads](verify-downloads.md)

---

## Quick start

```bash
# Install
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-v0.1.1-alpha-x86_64-linux-client.tar.gz
tar xf enola-cli-v0.1.1-alpha-x86_64-linux-client.tar.gz
sudo cp enola-cli /usr/local/bin/

# Deploy your first .onion service
sudo enola-cli files create --name my-web
```

See the [Quick Start Guide](quickstart.md) for the full walkthrough.

---

## Documentation

| Document | Description |
|----------|-------------|
| [Quick Start](quickstart.md) | Get your first .onion service running in 5 minutes |
| [Commands](commands.md) | Full command index — all modules and subcommands |
| [Concepts](concepts.md) | Architecture: Tor, Nginx port chain, Docker, AppArmor, PQC |
| [FAQ](faq.md) | Frequently asked questions |
| [Security Model](security-model.md) | Threat model, credential protection, binary integrity |
| [Verify Downloads](verify-downloads.md) | How to verify SHA256 + minisign + ML-DSA-65 signatures |

---

## Documentación en español

| Documento | Descripción |
|-----------|-------------|
| [Inicio rápido](../user/guia/quickstart.md) | Primeros pasos |
| [Comandos](../user/general/commands.md) | Índice de comandos |
| [Conceptos](../user/general/concepts.md) | Conceptos clave |
| [FAQ](../user/general/faq.md) | Preguntas frecuentes |
| [Seguridad](../user/general/SECURITY.md) | Modelo de seguridad |
| [Verificar descargas](../user/verify/verify-downloads.md) | Verificación de descargas |

---

## License

Proprietary (source-visible). See [LICENSE](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/blob/main/LICENSE).
Security vulnerabilities must be reported **only** by encrypted email to `salvadorpalmarodriguez@gmail.com`. See [SECURITY.md](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/blob/main/SECURITY.md).
