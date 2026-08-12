> **Documento usuario:** `docs/user/general/concepts.md`
> **Versión:** 2.0 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Guía de conceptos**
> **Referencias:** commands.md, docs/user/general/commands.md

# 💡 Conceptos Clave — Enola CLI

## Tor y las direcciones .onion

Tor (The Onion Router) es una red que cifra y anonimiza el tráfico entre
múltiples nodos ("saltos"). Los **servicios ocultos** de Tor tienen una
dirección `.onion` — solo accesible desde dentro de la red Tor.

**¿Qué significa para ti?**
- Tu servicio no tiene IP pública ni dominio registrado.
- Los visitantes no saben desde qué servidor se sirve el contenido.
- Tú no necesitas abrir puertos en tu router ni configurar DNS.
- El tráfico va cifrado de extremo a extremo.

**Cómo acceder a un .onion:**
- Instala el [Navegador Tor](https://www.torproject.org/)
- Escribe la dirección `.onion` en la barra de direcciones

---

## La cadena de puertos: .onion → Nginx → App

Cuando Enola despliega un servicio web, la arquitectura es:

```
Visitante (Tor Browser)
    │
    │  red Tor cifrada
    ▼
Tor (en tu servidor)        ← HiddenServicePort 80 → 127.0.0.1:NGINX_PORT
    │
    │  127.0.0.1 (solo localhost)
    ▼
Nginx (proxy inverso)       ← listen 127.0.0.1:NGINX_PORT
    │                         proxy_pass 127.0.0.1:APP_PORT
    │  127.0.0.1 (solo localhost)
    ▼
Tu aplicación / Docker      ← -p 127.0.0.1:APP_PORT:PUERTO_INTERNO
```

**Puntos clave:**
- `NGINX_PORT` y `APP_PORT` son puertos **internos** — nunca accesibles desde fuera.
- Docker siempre bindea a `127.0.0.1`, nunca a `0.0.0.0`.
- El visitante solo ve la dirección `.onion`.

---



## Puertos y seguridad de red

Enola gestiona tres tipos de puertos:

| Tipo | Ejemplo | Quién lo ve |
|------|---------|-------------|
| Puerto virtual .onion | 80, 443 | Solo visitantes Tor |
| Puerto Nginx (listen) | 10000-20000 | Solo localhost |
| Puerto de app (backend) | 8080-9000 (WordPress), 10000-15000 (Git HTTP), 30000-35000 (Git SSH) | Solo localhost |

**UFW (Firewall):**
Docker puede saltarse las reglas de UFW directamente. Enola incluye el
comando `firewall setup` que configura la cadena `DOCKER-USER` para
bloquear acceso externo a los puertos de Docker.

```
sudo enola-cli firewall setup
```

---

## VPN y WireGuard

WireGuard es un protocolo VPN moderno y ligero. A diferencia de Tor, que proporciona
anonimato, una VPN proporciona **autenticación y cifrado** entre peers conocidos.

**Diferencia clave Tor vs VPN:**

| Característica | Tor | VPN (WireGuard) |
|----------------|-----|------------------|
| Anonimato | Sí (nadie sabe quién eres) | No (peers se conocen) |
| Autenticación | No | Sí (claves criptográficas) |
| Latencia | Alta (3 saltos) | Baja (1 salto) |
| Caso de uso | Publicar contenido anónimo | Acceso remoto autenticado |

**Modelo de peers:** cada interfaz WireGuard tiene una clave privada y N peers con
sus claves públicas. El tráfico entre peers va cifrado con ChaCha20-Poly1305.

```
sudo enola-cli vpn create wg0 --port 51820
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
```

---

## AppArmor y sandboxing

AppArmor es un módulo del kernel Linux que confina programas individuales a un
conjunto de recursos limitados (perfiles). Enola CLI lo usa para aislar servicios.

**Modos de operación:**

| Modo | Descripción |
|------|-------------|
| `enforce` | Bloquea acciones no permitidas por el perfil (recomendado en producción) |
| `complain` | Permite acciones pero las registra (útil para depurar perfiles) |
| `disable` | Sin confinement |

**Complemento con UFW:** AppArmor confina procesos; UFW controla puertos. Ambos
son necesarios para defensa en profundidad.

```
sudo enola-cli apparmor setup
sudo enola-cli apparmor mode --enforce
```

---

## Docker y arquitectura de contenedores

Enola usa Docker para aislar cada servicio. Cada CMS, Git, o file share corre en
su propio contenedor con recursos limitados.

**Principios de seguridad:**

- **Bind a `127.0.0.1`**: los puertos Docker nunca se exponen a `0.0.0.0`. Solo
  Nginx (proxy inverso) accede a ellos vía localhost.
- **Bind mounts a `/srv/`**: los datos persistentes viven en `/srv/enola-{tipo}/{name}/`.
  Esto permite que los datos sobrevivan a la recreación de contenedores.
- **Redes aisladas**: cada servicio tiene su propia red Docker (`enola_net_{tipo}_{name}`).
- **Docker secrets**: las contraseñas y tokens se montan como Docker secrets en
  `/run/secrets/` (read-only), no como variables de entorno en texto plano.

---

## CMS: catálogo y stacks

Enola soporta 6 CMS con diferentes stacks:

| CMS | Lenguaje | BD | Contenedores | RAM mínima | Puerto interno |
|-----|----------|----|--------------|------------|----------------|
| WordPress | PHP | MariaDB | 2 (web + db) | 512 MB | 80 |
| Drupal | PHP | MariaDB | 2 (web + db) | 768 MB | 80 |
| Ghost | Node.js | SQLite | 1 (web) | ~256 MB | 2368 |
| Magnolia | Java | — | 1 (Tomcat) | ≥4 GB | 8080 |
| Strapi | Node.js | Postgres | 2 (web + db) | 512 MB | 1337 |
| Wagtail | Python | Postgres | 2 (web + db) | 512 MB | 8000 |

**¿Cuál elegir?**
- Blog simple → Ghost (más ligero) o WordPress (más plugins)
- Sitio corporativo grande → Magnolia (Java empresarial)
- Headless/API-first → Strapi
- Contenido estructurado → Wagtail (Django)
- Multilingüe + permisos → Drupal

---

## Firmas post-cuánticas (PQC) — no implica anonimato cuántico

Enola CLI incorpora criptografía post-cuántica para prepararse contra futuros
ordenadores cuánticos que podrían romper los algoritmos actuales.

**ML-DSA-65 (FIPS 204):** algoritmo de firma digital post-cuántica basado en
retículos (lattice-based). Se usa para firmar los binarios de release.

**minisign:** sistema de firma tradicional usado para verificar el feed de
advisories. Cada release tiene un archivo `.minisig` verificado con la clave
pública embebida en el binario.

**SSH hardening con sntrup761x25519:** el comando `maintenance ssh-harden-pqc`
configura SSH para usar algoritmos híbridos post-cuánticos.

```
sudo enola-cli maintenance ssh-harden-pqc
```

**Roadmap PQC:** ver `enola-cli docs quantum-security` para el plan completo.

---

## Feed de advisories y actualizaciones

Enola CLI consulta un feed JSON firmado con minisign para detectar actualizaciones
y avisos de seguridad.

**Cómo funciona `update check`:**
1. Descarga el feed JSON desde la URL configurada (`[update].feed_url`)
2. Verifica la firma minisign (`{feed_url}.minisig`)
3. Compara la versión actual con la última del feed
4. Muestra advisories que afectan a la versión instalada

**Exit codes para CI/scripts:**

| Code | Significado |
|------|-------------|
| 0 | OK (incluye update disponible sin advisory crítico) |
| 11 | Advisory crítico afecta a la versión actual |
| 12 | Versión actual por debajo de mínima soportada |
| 20 | Feed inválido/no parseable/no alcanzable |
| 21 | Firma minisign inválida o ausente |

**Rotación de clave minisign:** si la clave pública cambia, se distribuye una
nueva `minisign.pub` y se actualiza la clave embebida en el siguiente release.

## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`commands.md`](commands.md) | Índice de comandos |
| [`../guia/quickstart.md`](../guia/quickstart.md) | Guía de inicio rápido |
| [`faq.md`](faq.md) | Preguntas frecuentes |
| [`../guia/examples.md`](../guia/examples.md) | Ejemplos de uso |
