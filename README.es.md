# 🚀 Enola CLI - Referencia Completa de Comandos

**[English](README.md)** · **Español**

[![Version](https://img.shields.io/badge/version-0.1.2--alpha-blue.svg)](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases)
[![License](https://img.shields.io/badge/license-Proprietary-orange.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux-green.svg)](https://www.debian.org/)
[![Rust](https://img.shields.io/badge/rust-1.96-orange.svg)](https://www.rust-lang.org/)

> **CLI para gestionar servicios Tor, servidores Git, WordPress, CMS y archivos compartidos.**
>
> 📖 **Documentación**: [https://salvadorpalmarodriguez.github.io/enola-cli-lite/](https://salvadorpalmarodriguez.github.io/enola-cli-lite/) · 📄 **[llms.txt](llms.txt)** para indexadores de IA — la legibilidad por IA no constituye concesión de licencia; ver [LICENSE](LICENSE)

---

## 📋 Tabla de Contenidos

- [Instalación](#-instalación)
- [Uso Básico](#-uso-básico)
- [Comandos por Módulo](#-comandos-por-módulo)
  - [🧅 Tor - Servicios Ocultos](#-tor---servicios-ocultos)
  - [🐙 Git - Servidores Forgejo](#-git---servidores-forgejo)
  - [🌐 WordPress - Sitios Web](#-wordpress---sitios-web)
  - [🌐 Drupal - CMS](#-drupal---cms)
  - [✍️ Ghost - Blogs](#️-ghost---blogs)
  - [☕ Magnolia - CMS Java](#-magnolia---cms-java)
  - [� Strapi - Headless CMS](#-strapi---headless-cms)
  - [🐦 Wagtail - CMS Django](#-wagtail---cms-django)
  - [�📁 Files - Compartir Archivos](#-files---compartir-archivos)
  - [🔧 Maintenance - Mantenimiento](#-maintenance---mantenimiento)
  - [🩺 Diagnostics - Diagnósticos](#-diagnostics---diagnósticos)
  - [🧪 Test - Pruebas del Sistema](#-test---pruebas-del-sistema)
  - [📝 Logs - Gestión de Logs](#-logs---gestión-de-logs)
  - [🔌 Ports - Gestión de Puertos](#-ports---gestión-de-puertos)
  - [🛡️ Firewall - UFW](#-firewall---ufw)
  - [🛡️ AppArmor - Sandboxing](#-apparmor---sandboxing)
  - [🔒 VPN - WireGuard](#-vpn---wireguard)
  - [📦 Setup - Dependencias](#-setup---dependencias)
  - [🩺 Doctor - Diagnóstico](#-doctor---diagnóstico)
  - [🔄 Update - Advisories](#-update---advisories)
  - [🔐 Verify - Verificación PQC](#-verify---verificación-pqc)
  - [🗑️ Uninstall - Desinstalación](#-uninstall---desinstalación)
  - [📚 Docs - Documentación Offline](#-docs---documentación-offline)
  - [📄 License - Licencia](#-license---licencia)
  - [📖 Quickref - Referencia Rápida](#-quickref---referencia-rápida)
  - [🌐 Web - Dashboard Local](#-web---dashboard-local)
  - [⚙️ Config - Configuración](#-config---configuración)
- [Ejemplos Prácticos](#-ejemplos-prácticos)
- [Troubleshooting](#-troubleshooting)
- [Licencia](#-licencia)

---

## 📦 Instalación

### Desde GitHub Releases (Producción)

```bash
# Descargar y verificar (recomendado)
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash
```

El instalador descarga el binario, verifica SHA256 + firma minisign, e instala todo.
Ver guía completa: [verify-downloads.md](docs/user/verify/verify-downloads.md)

---

## 🎯 Uso Básico

```bash
# Requiere permisos de root
sudo enola-cli <comando> [subcomando] [opciones]

# Ayuda general
sudo enola-cli --help

# Ayuda de un módulo específico
sudo enola-cli tor --help

# Opciones globales
sudo enola-cli --format json tor list    # Salida en JSON
```

### Estructura de Comandos

```
enola-cli
├── tor         # Servicios Tor ocultos (.onion)
├── git         # Servidores Git (Forgejo)
├── wp          # Sitios WordPress
├── drupal      # Sitios Drupal (CMS)
├── ghost       # Blogs Ghost (CMS)
├── magnolia    # CMS Magnolia (Tomcat)
├── strapi      # Headless CMS Strapi
├── wagtail     # CMS Wagtail (Django)
├── files       # Compartir archivos seguros
├── maintenance # Operaciones de mantenimiento
├── diag        # Diagnósticos del sistema
├── test        # Ejecutar tests
├── logs        # Ver logs del sistema
├── ports       # Gestión de puertos
├── firewall    # Firewall UFW
├── apparmor    # Sandboxing con AppArmor
├── vpn         # Túneles WireGuard VPN
├── setup       # Instalar dependencias
├── doctor      # Verificar dependencias
├── update      # Feed de advisories y actualizaciones
├── verify      # Verificar autenticidad de descargas (PQC)
├── uninstall   # Desinstalación del CLI
├── docs        # Documentación embebida en el binario
├── license     # Texto de la licencia
├── quickref    # Referencia rápida Docker ↔ Enola
├── web         # Dashboard web local (GUI)
├── config-show    # Mostrar configuración efectiva
└── config-validate # Validar configuración
```


---

## 📚 Comandos por Módulo

---

## 🧅 Tor - Servicios Ocultos

Gestiona servicios ocultos de Tor (.onion) con diferentes arquitecturas.

### Listar Servicios

```bash
sudo enola-cli tor list
```

Muestra todos los servicios Tor activos con sus direcciones .onion y puertos.

### Crear Servicio

```bash
sudo enola-cli tor create [opciones]
```

| Opción | Descripción | Default |
|--------|-------------|---------|
| `-n, --name <NOMBRE>` | Nombre del servicio (requerido) | - |
| `-s, --service-type <TIPO>` | Tipo: `web`, `static`, `files`, `raw` | `web` |
| `-p, --virtual-port <PUERTO>` | Puerto público .onion | `80` |
| `-t, --target-port <PUERTO>` | Puerto de tu aplicación (Default: 8080 para web, igual a virtual para raw) | auto |
| `--ssl` | Habilitar HTTPS con certificado auto-firmado | `false` |

**Tipos de servicio:**

| Tipo | Arquitectura | Uso |
|------|--------------|-----|
| `web` / `proxy` | Tor → Nginx → App | Apps web (recomendado) |
| `static` | Tor → Nginx | Sitios estáticos |
| `files` | Tor → Nginx | Servidor de archivos |
| `raw` / `tcp` | Tor → App | SSH, bases de datos |

**Ejemplos:**

```bash
# Servicio web básico (HTTP)
sudo enola-cli tor create -n miapp -s web --target-port 3000

# Servicio web con HTTPS
sudo enola-cli tor create -n miapp-secure -s web --target-port 8080 --ssl

# Servidor de archivos
sudo enola-cli tor create -n mis-archivos -s files

# Sitio estático
sudo enola-cli tor create -n mi-blog -s static
```

### Iniciar/Detener Servicio

```bash
# Iniciar
sudo enola-cli tor start <nombre>

# Detener
sudo enola-cli tor stop <nombre>
```

### Editar Puertos

```bash
sudo enola-cli tor edit <nombre> [opciones]
```

| Opción | Descripción |
|--------|-------------|
| `-p, --virtual-port <PUERTO>` | Puerto público .onion |
| `-n, --nginx-port <PUERTO>` | Puerto interno de Nginx |
| `-t, --target-port <PUERTO>` | Puerto de tu aplicación |
| `--auto-ports` | Encontrar puertos libres automáticamente |

**Flujo de puertos:** `.onion:VIRTUAL → Nginx:NGINX_PORT → App:TARGET_PORT`

```bash
# Cambiar puerto virtual a 8080
sudo enola-cli tor edit miapp -p 8080

# Auto-asignar puertos libres
sudo enola-cli tor edit miapp --auto-ports

# Cambiar puerto de aplicación
sudo enola-cli tor edit miapp -t 4000
```

### Eliminar Servicio

```bash
sudo enola-cli tor remove <nombre> [--force]
```

### Rotar Identidad (.onion)

```bash
sudo enola-cli tor rotate <nombre>
```

Genera una nueva dirección .onion para el servicio (útil si la anterior fue comprometida).

### Autenticación de Clientes

Control de acceso a servicios Tor mediante criptografía de clave pública.

```bash
# Listar clientes autorizados
sudo enola-cli tor auth list <servicio>

# Habilitar autenticación
sudo enola-cli tor auth enable <servicio>

# Deshabilitar autenticación
sudo enola-cli tor auth disable <servicio>

# Añadir cliente autorizado
sudo enola-cli tor auth add <servicio> -c <cliente> -p <clave_publica>

# Revocar acceso a cliente
sudo enola-cli tor auth revoke <servicio> -c <cliente>

# Generar par de claves para cliente
sudo enola-cli tor auth generate -c <nombre_cliente>

# Rotar claves de un cliente (mitiga harvest-now-decrypt-later)
sudo enola-cli tor auth rotate <servicio> -c <cliente>
```

---


## 🐙 Git - Servidores Forgejo

Gestiona servidores Git (Forgejo) con integración Tor.

### Comandos Principales

```bash
# Listar servidores
sudo enola-cli git list

# Crear servidor (modo web: asistente en el navegador)
sudo enola-cli git create -n mi-git [--ssl] [--http-port <PUERTO>] [--ssh-port <PUERTO>]

# Crear servidor (modo CLI: admin creado automáticamente)
sudo enola-cli git create -n mi-git --admin-user alice --admin-password MiPass123

# Control de ciclo de vida
sudo enola-cli git start <nombre>
sudo enola-cli git stop <nombre>
sudo enola-cli git status <nombre>
sudo enola-cli git delete <nombre> [--force]

# Configurar registro de usuarios del servidor Git (habilitar/deshabilitar)
sudo enola-cli git registration <nombre> --enable
```

### Editar Puertos

```bash
sudo enola-cli git edit <nombre> [opciones]
```

| Opción | Descripción |
|--------|-------------|
| `--http-port <PUERTO>` | Puerto HTTP |
| `--https-port <PUERTO>` | Puerto HTTPS |
| `--ssh-port <PUERTO>` | Puerto SSH |
| `--auto-ports` | Auto-detectar puertos |

### Exponer en Tor

```bash
# Publicar en Tor
sudo enola-cli git publish <nombre> [--ssl]

# Ocultar de Tor
sudo enola-cli git hide <nombre>
```

### Gestión de Usuarios

```bash
# Listar usuarios
sudo enola-cli git user list <servidor>

# Crear usuario
sudo enola-cli git user create <servidor> -u usuario -e email@test.com -p password

# Eliminar usuario
sudo enola-cli git user delete <servidor> -u usuario
```

### Pipeline Watcher

```bash
# Ejecutar watcher de pipelines (foreground)
sudo enola-cli git watcher
```

---

## 🌐 WordPress - Sitios Web

Gestiona sitios WordPress con Docker y exposición en Tor.

### Comandos Principales

```bash
# Listar sitios
sudo enola-cli wp list

# Crear sitio
sudo enola-cli wp create -n mi-blog [--http-port <PUERTO>]   # auto: rango 8080-9000

# Control de ciclo de vida
sudo enola-cli wp start <nombre>
sudo enola-cli wp stop <nombre>
sudo enola-cli wp restart <nombre>
sudo enola-cli wp delete <nombre> [--force]

# Ver estado
sudo enola-cli wp status <nombre>

# Actualizar WordPress (con backup)
sudo enola-cli wp update <nombre>

# Configuración
sudo enola-cli wp config <nombre>
```

### Exposición en Tor

```bash
# Publicar en Tor
sudo enola-cli wp publish <nombre>

# Ocultar de Tor
sudo enola-cli wp hide <nombre>
```

### Editar Configuración

```bash
sudo enola-cli wp edit <nombre> [opciones]
```

| Opción | Descripción |
|--------|-------------|
| `--http-port <PUERTO>` | Puerto HTTP |
| `--https-port <PUERTO>` | Puerto HTTPS |
| `--ssl <true/false>` | Habilitar/deshabilitar SSL |
| `--auto-ports` | Auto-detectar puertos |

---

## 🌐 Drupal - CMS

Gestiona sitios Drupal. Stack: `drupal:10-apache` + `mariadb:10.11`. Datos en `/srv/enola-drupal/<nombre>/`.

```bash
# Listar sitios
sudo enola-cli drupal list

# Crear sitio (puerto HTTP interno requerido)
sudo enola-cli drupal create --name mi-sitio --http-port 8090

# Ciclo de vida
sudo enola-cli drupal start <nombre>
sudo enola-cli drupal stop <nombre>
sudo enola-cli drupal status <nombre>
sudo enola-cli drupal delete <nombre> [--force]

# Exposición en Tor
sudo enola-cli drupal publish <nombre>
sudo enola-cli drupal hide <nombre>

# Cambiar puerto HTTP (recrea el contenedor web atómicamente)
sudo enola-cli drupal edit <nombre> --http-port <PUERTO>
```

---

## ✍️ Ghost - Blogs

Gestiona blogs Ghost. Stack: `ghost:5-alpine` + SQLite embebido (un solo contenedor). Datos en `/srv/enola-ghost/<nombre>/content/`.

```bash
# Listar blogs
sudo enola-cli ghost list

# Crear blog (puerto HTTP interno requerido; el contenedor usa 2368)
sudo enola-cli ghost create --name mi-blog --http-port 8095

# Ciclo de vida
sudo enola-cli ghost start <nombre>
sudo enola-cli ghost stop <nombre>
sudo enola-cli ghost status <nombre>
sudo enola-cli ghost delete <nombre> [--force]

# Exposición en Tor
sudo enola-cli ghost publish <nombre>
sudo enola-cli ghost hide <nombre>

# Cambiar puerto HTTP
sudo enola-cli ghost edit <nombre> --http-port <PUERTO>
```

---

## ☕ Magnolia - CMS Java

Gestiona instancias Magnolia CMS. Stack: `magnolia-cms:6` (Tomcat, Java). **Requiere ≥4 GB de RAM disponibles.**

```bash
# Listar instancias
sudo enola-cli magnolia list

# Crear instancia (puerto HTTP interno requerido; Tomcat usa 8080)
sudo enola-cli magnolia create --name mi-sitio --http-port 8100

# Ciclo de vida
sudo enola-cli magnolia start <nombre>
sudo enola-cli magnolia stop <nombre>
sudo enola-cli magnolia status <nombre>
sudo enola-cli magnolia delete <nombre> [--force]

# Exposición en Tor
sudo enola-cli magnolia publish <nombre>
sudo enola-cli magnolia hide <nombre>
```

---

## 🚀 Strapi - Headless CMS

Gestiona instancias Strapi. Stack: `enola/strapi:5.49.0` + `postgres:16-alpine`. Genera secrets con permisos 0600 por instancia.

```bash
# Construir la imagen Docker de producción (una vez, antes del primer create; ~5-10 min)
sudo enola-cli strapi build-image [--force]

# Listar instancias
sudo enola-cli strapi list

# Crear instancia (puerto HTTP interno requerido; Strapi usa 1337)
sudo enola-cli strapi create --name mi-api --http-port 1337

# Ciclo de vida
sudo enola-cli strapi start <nombre>
sudo enola-cli strapi stop <nombre>
sudo enola-cli strapi status <nombre>
sudo enola-cli strapi delete <nombre> [--force]

# Exposición en Tor
sudo enola-cli strapi publish <nombre>
sudo enola-cli strapi hide <nombre>
```

---

## 🐦 Wagtail - CMS Django

Gestiona instancias Wagtail. Stack: Wagtail (Python/Django) + `postgres:16-alpine`.

```bash
# Listar instancias
sudo enola-cli wagtail list

# Crear instancia (puerto HTTP interno requerido; Wagtail usa 8000)
sudo enola-cli wagtail create --name mi-sitio --http-port 8200

# Ciclo de vida
sudo enola-cli wagtail start <nombre>
sudo enola-cli wagtail stop <nombre>
sudo enola-cli wagtail status <nombre>
sudo enola-cli wagtail delete <nombre> [--force]

# Exposición en Tor
sudo enola-cli wagtail publish <nombre>
sudo enola-cli wagtail hide <nombre>
```

---

## 📁 Files - Compartir Archivos

Crea servidores de archivos seguros accesibles vía Tor.

```bash
# Listar shares
sudo enola-cli files list

# Crear share
sudo enola-cli files create -n mis-archivos [-a] [--ssl]

# Editar puerto
sudo enola-cli files edit <nombre> -p 8080

# Corregir permisos
sudo enola-cli files fix-perms <nombre>

# Eliminar share
sudo enola-cli files delete <nombre> [-f]
```

**Directorio de archivos:** `/srv/enola-files/<nombre>/`

```bash
# Añadir archivos al share
sudo cp archivo.pdf /srv/enola-files/mis-archivos/
sudo cp -r carpeta/ /srv/enola-files/mis-archivos/
```

---


## 🔧 Maintenance - Mantenimiento

Operaciones de mantenimiento del sistema.

```bash
# Ver estado del sistema
sudo enola-cli maintenance status

# Ejecutar smoke test
sudo enola-cli maintenance smoke-test

# Health checks automáticos
sudo enola-cli maintenance enable-checks
sudo enola-cli maintenance disable-checks
sudo enola-cli maintenance timer-status

# Configurar SSH check
sudo enola-cli maintenance ssh-config

# Endurecer SSH con algoritmos post-cuánticos (OpenSSH 9.0+)
sudo enola-cli maintenance ssh-harden-pqc [--dry-run] [--force]

# Crear backup del sistema
sudo enola-cli maintenance backup

# Limpiar archivos temporales y datos residuales
sudo enola-cli maintenance cleanup [--target all|logs|docker] [--dry-run] [--keep-days 7]
```

---

## 🩺 Diagnostics - Diagnósticos

Verifica el estado de los componentes del sistema.

```bash
# Resumen de todos los servicios
sudo enola-cli diag summary

# Verificar componentes individuales
sudo enola-cli diag nginx
sudo enola-cli diag tor
sudo enola-cli diag ssh
sudo enola-cli diag wordpress

# Sincronización WordPress/Nginx
sudo enola-cli diag wp-sync

# Probar configuración de Nginx
sudo enola-cli diag nginx-test

# Ver recursos del sistema (RAM, Disco, GPU)
sudo enola-cli diag resources
```

---

## 🧪 Test - Pruebas del Sistema

Ejecuta tests automatizados del sistema.

```bash
# Ejecutar todos los tests
sudo enola-cli test run

# Ejecutar con filtro
sudo enola-cli test run -f "tor"

# Listar tests disponibles
sudo enola-cli test list

# Ejecutar benchmarks
sudo enola-cli test benchmark

# Ver últimos resultados
sudo enola-cli test results

# Limpiar artefactos de tests
sudo enola-cli test clean
```

---

## 📝 Logs - Gestión de Logs

Ver y gestionar logs del sistema.

```bash
# Listar fuentes de logs
sudo enola-cli logs list

# Ver logs de una fuente (default: 50 líneas)
sudo enola-cli logs view <fuente> [-l 50] [-f]

# Fuentes disponibles: system, tor, nginx, docker, etc.

# Ver logs de instalación
sudo enola-cli logs install

# Ver logs de smoke test
sudo enola-cli logs smoke-test
```

---

## 🔌 Ports - Gestión de Puertos

Muestra todos los puertos en uso por servicios Enola (Tor, Nginx, Docker).

```bash
# Listar todos los puertos
sudo enola-cli ports list
```

Incluye contenedores detenidos que retienen port bindings de Docker.

---

## 🛡️ Firewall - UFW

Gestiona el firewall UFW del host.

```bash
# Configurar política segura por defecto
sudo enola-cli firewall setup

# Ver estado del firewall
sudo enola-cli firewall status

# Permitir/denegar puertos
sudo enola-cli firewall allow --port <puerto>
sudo enola-cli firewall deny --port <puerto>
```

---

## 🛡️ AppArmor - Sandboxing

Gestiona perfiles AppArmor para aislamiento de procesos.

```bash
# Instalar perfiles base (nginx, tor, docker)
sudo enola-cli apparmor setup

# Ver estado de perfiles
sudo enola-cli apparmor status

# Cambiar modo (enforce/complain/disable)
sudo enola-cli apparmor mode --enforce
```

---

## 🔒 VPN - WireGuard

Gestiona túneles VPN WireGuard para acceso remoto autenticado.

```bash
# Crear interfaz VPN
sudo enola-cli vpn create <nombre> [--port 51820] [--subnet 10.8.0.0/24] [--autostart] [--sync-firewall]

# Listar interfaces
sudo enola-cli vpn list

# Gestionar interfaz
sudo enola-cli vpn start <nombre>
sudo enola-cli vpn stop <nombre>
sudo enola-cli vpn status <nombre>
sudo enola-cli vpn delete <nombre> [--force] [--sync-firewall]

# Gestionar peers
sudo enola-cli vpn peer add <interfaz> <peer> --endpoint <host> [--dns <ip>] [--psk] [--ip <ip>]
sudo enola-cli vpn peer add-pubkey <interfaz> <peer> <clave_publica> <ip>
sudo enola-cli vpn peer remove <interfaz> <public_key>
```

---

## 📦 Setup - Dependencias

Instala dependencias del sistema (Docker, Nginx, Tor, WireGuard, UFW, AppArmor).

```bash
# Instalar dependencias core
sudo enola-cli setup

# Instalar todo (dependencias core + VPN + seguridad)
sudo enola-cli setup --all

# Instalar solo VPN
sudo enola-cli setup --vpn

# Instalar solo seguridad (UFW, AppArmor)
sudo enola-cli setup --security

# Instalar stack PQC TLS (OpenSSL 3.5 + Nginx)
sudo enola-cli setup --pqc-tls
```

---

## 🩺 Doctor - Diagnóstico

Verifica qué dependencias están instaladas y cuáles faltan.

```bash
# Verificación básica
sudo enola-cli doctor

# Auditoría de seguridad (hardening, configs, secrets)
sudo enola-cli doctor --security
```

---

## 🔄 Update - Advisories

Feed de advisories de seguridad y actualizaciones.

```bash
# Comprobar actualizaciones disponibles
sudo enola-cli update check

# Salida JSON (CI)
sudo enola-cli update check --json

# Ver esquema del feed
sudo enola-cli update schema

# Verificar feed manualmente
sudo enola-cli update verify-feed <url-o-path>

# Descargar última versión
sudo enola-cli update download

# Descargar y aplicar
sudo enola-cli update download --yes

# Aplicar una actualización ya descargada
sudo enola-cli update apply [--binary <ruta>]
```

---

## 🔐 Verify - Verificación PQC

Verifica autenticidad de descargas con firma post-cuántica ML-DSA-65 (FIPS 204).

```bash
# Verificar un release descargado
enola-cli verify enola-cli-v0.1.2-alpha-x86_64-linux.tar.gz

# Con firma alternativa y salida JSON
enola-cli verify <archivo> --pqsig <firma.pqsig> --json
```

No requiere red ni herramientas externas — la clave pública está embebida en el binario.

---

## 🗑️ Uninstall - Desinstalación

Desinstala Enola CLI del sistema de forma limpia.

```bash
# Dry-run (no borra, solo lista)
sudo enola-cli uninstall

# Borrar todo
sudo enola-cli uninstall --yes

# Preservar datos
sudo enola-cli uninstall --yes --keep-data

# Solo secciones específicas
sudo enola-cli uninstall --yes --only tor,nginx

# También borrar dependencias instaladas
sudo enola-cli uninstall --yes --remove-deps
```

---

## 📚 Docs - Documentación Offline

Documentación embebida en el binario — funciona sin conexión.

```bash
# Guía de inicio rápido
enola-cli docs quickstart

# Referencia de comandos
enola-cli docs commands [GRUPO]

# Conceptos clave
enola-cli docs concepts [TEMA]

# Preguntas frecuentes
enola-cli docs faq [TÉRMINO]

# Ejemplos de uso
enola-cli docs examples [CASO]

# Buscar en toda la documentación
enola-cli docs search TÉRMINO

# Guías avanzadas
enola-cli docs quantum-security
enola-cli docs verify-downloads
enola-cli docs security
enola-cli docs install-from-iso
```

---

## 📄 License - Licencia

Muestra el texto completo de la licencia propietaria.

```bash
enola-cli license
enola-cli license | less
```

---

## 📖 Quickref - Referencia Rápida

Tabla de equivalencias entre comandos Docker y Enola CLI.

```bash
enola-cli quickref
```

---

## 🌐 Web - Dashboard Local

Inicia un dashboard web local para gestionar servicios desde el navegador.

```bash
sudo enola-cli web --port 8090
```

El servidor binda solo a `127.0.0.1`. Se genera un token aleatorio que se muestra en el terminal. Abre `http://127.0.0.1:8090` e introduce el token.

Más detalles: [docs/user/web/README.md](docs/user/web/README.md)

---

## ⚙️ Config - Configuración

Inspecciona y valida la configuración centralizada (`config.toml`).

```bash
# Mostrar configuración efectiva (con fuente de cada valor)
enola-cli config-show

# Salida JSON (CI, jq)
enola-cli config-show --json

# Validar configuración (offline)
enola-cli config-validate

# Validar con ping HTTP a URLs
enola-cli config-validate --reachable

# Salida JSON estructurada
enola-cli config-validate --json
```

---

## 💡 Ejemplos Prácticos


### Crear un Blog Anónimo con WordPress

```bash
# 1. Crear sitio WordPress
sudo enola-cli wp create -n mi-blog

# 2. Esperar a que inicie (puede tardar 1-2 minutos)
sudo enola-cli wp status mi-blog

# 3. Publicar en Tor
sudo enola-cli wp publish mi-blog

# 4. Obtener dirección .onion
sudo enola-cli tor list
```


### Servidor Git Seguro

```bash
# 1. Crear servidor con HTTPS
sudo enola-cli git create -n codigo --ssl

# 2. Crear usuario admin
sudo enola-cli git user create codigo -u admin -e admin@local.onion -p MiPassword123

# 3. Exponer en Tor
sudo enola-cli git publish codigo --ssl

# 4. Obtener dirección .onion
sudo enola-cli tor list
```


---

## 🔧 Troubleshooting

### Error: "Root privileges required"

```bash
# Solución: Ejecutar con sudo
sudo enola-cli <comando>
```

### Servicio no accesible vía Tor

```bash
# 1. Verificar que Tor está activo
sudo systemctl status tor

# 2. Verificar servicio
sudo enola-cli tor list

# 3. Ver logs de Tor
sudo enola-cli logs view tor -l 100
```

### Puerto en uso

```bash
# Usar auto-ports para encontrar puertos libres
sudo enola-cli tor edit miservicio --auto-ports

# O especificar manualmente
sudo enola-cli tor edit miservicio -p 8081 -t 9000
```

### Verificar diagnósticos completos

```bash
sudo enola-cli diag summary
sudo enola-cli diag resources
```

---

### Guías de usuario destacadas

- [docs/user/general/SECURITY.md](docs/user/general/SECURITY.md)
- [docs/user/general/concepts.md](docs/user/general/concepts.md)
- [docs/user/general/faq.md](docs/user/general/faq.md)
- [docs/user/guia/quickstart.md](docs/user/guia/quickstart.md)
- [docs/user/guia/install-from-iso.md](docs/user/guia/install-from-iso.md)
- [docs/user/verify/verify-downloads.md](docs/user/verify/verify-downloads.md)
- [docs/user/uninstall/uninstall.md](docs/user/uninstall/uninstall.md)

Para asistentes de IA y crawlers LLM: ver [llms.txt](llms.txt) y [llms-full.txt](llms-full.txt) — la legibilidad por IA no constituye concesión de licencia; ver [LICENSE](LICENSE).

---

## 📄 Licencia

**Enola CLI es software propietario de código visible.** Copyright © 2026 Salvador Palma Rodriguez. Todos los derechos reservados.

- 📖 **Código visible** — se permite ver, leer y compilar el código fuente para uso personal.
- ✅ **Uso personal gratuito** — la licencia es gratuita para uso personal no comercial.
- 🚫 **No redistribución** — no se permite redistribuir, publicar, vender ni poner el software a disposición de terceros.
- 🚫 **No uso empresarial** — no se permite uso comercial, empresarial o generador de ingresos.
- ⚠️ **Sin garantía de continuidad** — el software puede discontinuarse en cualquier momento sin aviso.
- ⚠️ **No responsabilidad del autor** — el autor NO es responsable del uso del software ni de posibles daños.
- 🛡️ **Divulgación coordinada** — las vulnerabilidades deben reportarse al autor en un máximo de 72 horas. **Prohibida** su divulgación pública hasta que sean remediadas.
- ⚖️ **Jurisdicción** — España / Unión Europea.

Licencia completa: [LICENSE](LICENSE) · Contacto: salvadorpalmarodriguez@gmail.com

### Licencias de terceros

Este software utiliza dependencias de terceros (Rust crates) licenciadas
bajo MIT, Apache-2.0, BSD, ISC, MPL-2.0 y otras licencias permisivas. El listado completo
de dependencias y sus textos de licencia está disponible en:
[THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt)

---

<div align="center">

**Enola CLI** - Privacidad por diseño 🔐

[Documentación](docs/) · [Issues](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/issues) · [Releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases)

</div>
