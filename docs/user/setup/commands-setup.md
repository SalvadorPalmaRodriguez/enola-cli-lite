> **Documento usuario:** `docs/user/setup/commands-setup.md`
> **Versión:** 1.0 | **Actualizado:** 2026-08-07
> **Estado:** ✅ **VIGENTE — Guía de usuario**

# 🩺 Setup & Doctor — Comandos `enola-cli setup` y `enola-cli doctor`

Instalación de dependencias del sistema y diagnóstico de salud.

---

## `setup`

Instala las dependencias del sistema necesarias para que Enola CLI funcione: Docker, Nginx, Tor, WireGuard, UFW y AppArmor.

```bash
sudo enola-cli setup              # Instalar dependencias core
sudo enola-cli setup --all        # Instalar TODO (core + vpn + security)
sudo enola-cli setup --vpn        # Instalar solo VPN (WireGuard)
sudo enola-cli setup --security   # Instalar solo seguridad (UFW, AppArmor)
sudo enola-cli setup --pqc-tls    # Instalar stack PQC TLS (OpenSSL 3.5 + Nginx)
```

### Flags

| Flag | Descripción | Default |
|------|-------------|---------|
| `--all` | Instala core + VPN + seguridad | `false` |
| `--vpn` | Instala WireGuard | `false` |
| `--security` | Instala UFW y AppArmor | `false` |
| `--pqc-tls` | Instala OpenSSL 3.5.x + Nginx con soporte PQC | `false` |

Sin flags, instala solo las dependencias core (Docker, Nginx, Tor).

### Qué instala cada modo

- **Core**: Docker, Nginx, Tor
- **VPN**: `wireguard-tools`
- **Security**: UFW, AppArmor
- **PQC TLS**: Compila OpenSSL 3.5.x desde código fuente y recompila Nginx enlazado contra él

---

## `doctor`

Verifica qué dependencias están instaladas y cuáles faltan.

```bash
enola-cli doctor
enola-cli doctor --security
```

### Flags

| Flag | Descripción | Default |
|------|-------------|---------|
| `--security` | Ejecuta auditoría de seguridad: hardening de contenedores, configs de Nginx, AppArmor, UFW y secrets en env vars | `false` |

### Salida

Muestra una tabla con:
- Cada dependencia (Docker, Nginx, Tor, WireGuard, UFW, AppArmor)
- Estado: ✅ instalado / ❌ faltante
- Versión detectada (si aplica)

Con `--security`, adicionalmente verifica:
- Contenedores Docker con privilegios excesivos
- Configuraciones de Nginx inseguras
- Perfiles de AppArmor activos
- Reglas de UFW
- Variables de entorno con posibles secrets

---

## Ver también

- [Guía de inicio rápido](../guia/quickstart.md)
- [Instalación desde ISO](../guia/install-from-iso.md)
- [Firewall UFW](../firewall/commands-firewall.md)
- [AppArmor](../apparmor/commands-apparmor.md)
- [VPN WireGuard](../vpn/commands-vpn.md)
