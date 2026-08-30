# Enola CLI

**CLI en Rust para autohospedar servicios Tor hidden services (.onion), servidores Git (Forgejo), CMS (WordPress, Drupal, Ghost, Strapi, Wagtail, Magnolia), compartir archivos, VPN WireGuard, firewall UFW y sandboxing AppArmor en Debian/Linux — con releases firmadas post-cuánticas (ML-DSA).**

[Repositorio](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite) · [Releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases) · [Política de seguridad](general/SECURITY.md) · [Verificar descargas](verify/verify-downloads.md)

---

## Inicio rápido

```bash
# Instalar
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-v0.2.0-alpha-x86_64-linux-client.tar.gz
tar xf enola-cli-v0.2.0-alpha-x86_64-linux-client.tar.gz
sudo cp enola-cli /usr/local/bin/

# Desplegar tu primer servicio .onion
sudo enola-cli files create --name my-web
```

Consulta la [Guía de inicio rápido](guia/quickstart.md) para el tutorial completo.

---

## Documentación

| Documento | Descripción |
|-----------|-------------|
| [Inicio rápido](guia/quickstart.md) | Primeros pasos en 5 minutos |
| [Comandos](general/commands.md) | Índice completo de comandos — todos los módulos |
| [Conceptos](general/concepts.md) | Arquitectura: Tor, Nginx, Docker, AppArmor, PQC |
| [FAQ](general/faq.md) | Preguntas frecuentes |
| [Modelo de seguridad](general/SECURITY.md) | Modelo de amenazas, protección de credenciales |
| [Verificar descargas](verify/verify-downloads.md) | Cómo verificar SHA256 + minisign + ML-DSA-65 |

---

## English documentation

| Document | Description |
|----------|-------------|
| [Quick Start](../en/quickstart.md) | Get started in 5 minutes |
| [Commands](../en/commands.md) | Full command index |
| [Concepts](../en/concepts.md) | Architecture overview |
| [FAQ](../en/faq.md) | Frequently asked questions |
| [Security Model](../en/security-model.md) | Threat model |
| [Verify Downloads](../en/verify-downloads.md) | Signature verification |

---

## Licencia

Propietario (código visible). Ver [LICENSE](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/blob/main/LICENSE).
Las vulnerabilidades de seguridad deben reportarse **solo** por email cifrado a `salvadorpalmarodriguez@gmail.com`. Ver [SECURITY.md](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/blob/main/SECURITY.md).
