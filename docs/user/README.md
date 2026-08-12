# Documentación de Usuario — Enola CLI

Esta carpeta contiene toda la documentación orientada al usuario final,
organizada por comando del CLI y categorías transversales.

## Estructura

### Directorios por comando

Cada subdirectorio corresponde a un comando del CLI.

| Directorio | Comando | Descripción |
|------------|---------|-------------|
| `tor/` | `enola-cli tor` | Servicios ocultos Tor |
| `git/` | `enola-cli git` | Servidores Git (Forgejo) |
| `wp/` | `enola-cli wp` | Sitios WordPress |
| `drupal/` | `enola-cli drupal` | Sitios Drupal (CMS) |
| `ghost/` | `enola-cli ghost` | Blogs Ghost (CMS) |
| `magnolia/` | `enola-cli magnolia` | CMS Magnolia (Tomcat) |
| `strapi/` | `enola-cli strapi` | Headless CMS Strapi |
| `wagtail/` | `enola-cli wagtail` | CMS Wagtail (Django) |
| `files/` | `enola-cli files` | Servidores de archivos |
| `maintenance/` | `enola-cli maintenance` | Mantenimiento del sistema |
| `diag/` | `enola-cli diag` | Diagnósticos y salud |
| `logs/` | `enola-cli logs` | Ver y gestionar logs |
| `ports/` | `enola-cli ports` | Gestión de puertos |
| `firewall/` | `enola-cli firewall` | Firewall UFW |
| `apparmor/` | `enola-cli apparmor` | Sandboxing con AppArmor |
| `vpn/` | `enola-cli vpn` | Túneles WireGuard VPN |
| `update/` | `enola-cli update` | Feed de advisories y actualizaciones |
| `verify/` | `enola-cli verify` | Verificar autenticidad de descargas (PQC) |
| `uninstall/` | `enola-cli uninstall` | Desinstalación del CLI |
| `setup/` | `enola-cli setup`, `enola-cli doctor` | Instalación de dependencias y diagnóstico |
| `test/` | `enola-cli test` | Tests del sistema y benchmarks |
| `docs/` | `enola-cli docs` | Documentación integrada offline |
| `web/` | `enola-cli web` | Interfaz web del CLI |

### Categorías transversales

| Directorio | Descripción |
|------------|-------------|
| `guia/` | Guías de uso y recomendaciones (inicio rápido, ejemplos, instalación) |
| `general/` | Documentos genéricos sin clasificación clara (referencia de comandos, conceptos, FAQ, seguridad, configuración) |

## Notas

- Los archivos de esta carpeta se embeben en el binario mediante
  `include_str!()` en `src/cli/docs.rs`. Cualquier movimiento o renombrado
  debe actualizarse también en ese archivo.
- `config-reference.md` es la única excepción: no se embebe por ser referencia
  consultable, no guía offline.
