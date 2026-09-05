> **Documento usuario:** `docs/user/general/quantum-security.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de seguridad post-cuántica**
> **Referencias:** verify-downloads.md

# 🔬 Seguridad Post-Cuántica en Enola CLI

> Estado actual (2026-07-31): **todas las protecciones que dependen de nosotros están activas**.
> Las limitaciones restantes dependen de proyectos externos (Tor, Forgejo, WireGuard).

---

## ¿Qué es la amenaza cuántica?

Los ordenadores cuánticos pueden romper la criptografía asimétrica actual (RSA, ECDSA, Diffie-Hellman)
usando el **algoritmo de Shor**. El ataque más relevante hoy es **"harvest now, decrypt later" (HNDL)**:
un adversario graba tráfico cifrado ahora y lo descifra cuando tenga suficiente potencia cuántica.

**La criptografía simétrica** (AES-256, ChaCha20-Poly1305) **no es vulnerable** a Shor —
solo requiere duplicar el tamaño de clave (ya cumplido con AES-256).

---

## ¿Qué protege Enola CLI hoy?

### ✅ Protegido (criptografía resistente a computación cuántica)

| Componente | Algoritmo | Estado |
|-----------|-----------|--------|
| Firma de releases | **ML-DSA-65 (FIPS 204)** + Ed25519 dual | ✅ Firma post-cuántica activa |
| TLS entre servicios (con `setup --pqc-tls`) | **ML-KEM-768 (FIPS 203)** vía TLSv1.3 | ✅ KEX post-cuántico activo |
| SSH host (con `ssh-harden-pqc`) | **sntrup761+X25519** híbrido | ✅ KEX post-cuántico activo |
| TLS configs generadas | **TLSv1.3 exclusivo** | ✅ Único protocolo compatible con PQC |
| Tráfico Tor a .onion | ChaCha20-Poly1305 (simétrico) | ✅ Post-quantum safe |
| Cifrado de datos en reposo | AES-256-GCM | ✅ Post-quantum safe |
| Hashes de integridad | SHA-256 / SHA-512 | ✅ Post-quantum safe |
| MACs SSH en Forgejo | HMAC-SHA2-256-ETM, HMAC-SHA2-512-ETM | ✅ Mejorado |
| Certificados TLS | RSA-4096 + KEX PQC | ⚠️ Cert clásico, pero **KEX PQC protege tráfico** |

### ⚠️ Mitigado (protección parcial con medidas transitorias)

| Componente | Riesgo | Mitigación activa |
|-----------|--------|-------------------|
| Client auth Tor (.auth_private) | X25519 (vulnerable a Shor) | Rotar cada 90 días (`tor auth rotate`) |

### ❌ No protegido todavía (depende de terceros)

| Componente | Razón | Qué necesita ocurrir |
|-----------|-------|----------------------|
| SSH en Forgejo (KEX) | Go `crypto/ssh` no soporta PQC | Forgejo soportar modo OpenSSH externo |
| Handshake Tor circuit | X25519 ntor (protocolo Tor) | Tor Project implementar ntor-v3 + ML-KEM |
| Identidad .onion (Ed25519) | Definida por protocolo Tor v3 | Tor Project implementar identidad PQC |
| WireGuard (KEX) | X25519 nativo | Rosenpass publicar release 1.0 |

---

## Lo que puedes hacer tú ahora

### 1. Instalar el stack TLS post-cuántico

Instala OpenSSL 3.5 + Nginx con soporte ML-KEM en tu servidor:

```bash
# Instalar stack PQC TLS completo
sudo enola-cli setup --pqc-tls

# Verificar que el stack está activo
sudo enola-cli doctor
# → ✅ openssl-pqc    3.5.x
# → ✅ nginx-pqc      built with OpenSSL 3.5.x
```

Una vez instalado, **todas las configs SSL generadas por Enola** usarán automáticamente
`ssl_ecdh_curve X25519MLKEM768:X25519:prime256v1` — el grupo de curvas PQC híbrido
que protege el intercambio de claves contra computadores cuánticos.

### 2. Activar hardening SSH en tu servidor

Si tienes un servidor con OpenSSH 9.0+ (Ubuntu 24.04 LTS o superior):

```bash
# Ver qué cambiaría (sin aplicar)
sudo enola-cli maintenance ssh-harden-pqc --dry-run

# Aplicar el hardening
sudo enola-cli maintenance ssh-harden-pqc --force

# Verificar que sntrup761 está activo
ssh -Q kex | grep sntrup
```

Esto añade `sntrup761x25519-sha512@openssh.com` como primer KEX — el estándar híbrido
recomendado por el ANSSI (Francia) y el BSI (Alemania) para mitigación HNDL.

### 3. Rotar claves Tor periódicamente

Las claves X25519 de autenticación de cliente no son post-cuánticas, pero la rotación
periódica limita la ventana de ataque HNDL:

```bash
# Rotar claves cada 90 días (recomendado)
enola-cli tor auth rotate mi-servicio --client mi-cliente
```

El cliente debe importar las nuevas claves en Tor Browser:
`Tor Browser → Preferencias → Onion Services → Client authorization`

### 4. Verificar firma post-cuántica de releases

Cada release incluye firma ML-DSA-65 además de la firma clásica Ed25519:

```bash
# Verificar firma post-cuántica (clave pública embebida en enola-cli)
enola-cli verify enola-cli-vX.Y.Z-linux-x86_64.tar.gz
# → ✅ Firma post-cuántica ML-DSA-65 (clave pública embebida) — firma válida

# Verificación completa (3 capas)
sha256sum -c *.sha256                                          # integridad
minisign -Vm *.tar.gz -p enola.pub                             # autoría clásica
enola-cli verify *.tar.gz                                      # autoría PQC
```

> No necesitas `enola-sign-pqc` (es una herramienta de desarrollo para *firmar*):
> `enola-cli verify` ya lleva la clave pública ML-DSA-65 embebida.

Ver guía completa: [Verificar descargas](../verify/verify-downloads.md)

---

## Hoja de ruta post-cuántica

| Tarea | Estado | Detalle |
|-------|--------|---------|
| SSH MACs en Forgejo (ETM) | ✅ Completado | 2026-04 |
| SSH PQC híbrido en host | ✅ Completado (`ssh-harden-pqc`) | 2026-04 |
| Rotación de claves Tor | ✅ Completado (`tor auth rotate`) | 2026-04 |
| TLS PQC (OpenSSL 3.5 + ML-KEM) | ✅ Completado (`setup --pqc-tls`) | 2026-04 |
| TLSv1.3-only + RSA-4096 uniforme | ✅ Completado | 2026-04 |
| Firma binarios con ML-DSA-65 | ✅ Completado (`enola-sign-pqc`) | 2026-04 |
| Tor circuit PQC (ntor-v3) | ⏳ Bloqueado | Requiere Tor Project arti |
| WireGuard PQC (Rosenpass) | ⏳ Bloqueado | Requiere Rosenpass 1.0 |

### Dependencias upstream que estamos trackeando {#pqc-tracking}

> Tabla **sincronizada con el feed público** `/releases/advisories.json` →
> campo `pqc_milestones[]`. Política de honestidad en la
> comunicación: ver política de comunicación PQC
>.

| Bloquea | Estado | Upstream |
|---------|--------|----------|
| Tor circuit PQC | ⏳ pending | [Tor arti](https://gitlab.torproject.org/tpo/core/arti) — ML-KEM híbrido |
| TLS PQC (rustls puro) | ⏳ pending | [rustls](https://github.com/rustls/rustls) — KEMTLS / X25519MLKEM768 |
| Forgejo SSH PQC nativo | ⏳ pending | [Go crypto/ssh #64738](https://github.com/golang/go/issues/64738) |
| WireGuard PQC | ⏳ pending | [Rosenpass 1.0](https://rosenpass.eu/) |

Verificación rápida desde la línea de comandos (operador o cualquier auditor):

```bash
# Ver milestones actualmente publicados
curl -s https://salvadorpalmarodriguez.github.io/enola-cli-lite/releases/advisories.json \
  | jq '.pqc_milestones[]'
```

Cuando uno de estos milestones pase a `released`, lo anunciamos como advisory
informativo (`ENOLA-ADV-YYYY-NNN`) en el mismo feed y movemos la fila a la
tabla principal con su versión y comando de verificación.

---

## Preguntas frecuentes

**¿Mis servicios .onion son vulnerables a un ordenador cuántico?**

El tráfico Tor usa ChaCha20-Poly1305 para el cifrado de datos — esto es simétrico y
**no es vulnerable** al algoritmo de Shor. Con `setup --pqc-tls`, el TLS entre Nginx y
los backends también usa KEX post-cuántico (ML-KEM-768). El riesgo restante es el KEX
del circuito Tor (ntor), que el Tor Project está migrando a ML-KEM.

**¿Cuándo llegará protección PQC completa?**

Todo lo que depende de Enola CLI **ya está completado**. Lo que falta depende de terceros:
Tor Project (circuitos PQC), Forgejo (SSH PQC), y Rosenpass (WireGuard PQC).
La adopción completa se espera para 2027-2028.

**¿Necesito hacer algo urgente?**

Ejecuta estos 2 comandos para activar toda la protección PQC disponible:
```bash
sudo enola-cli setup --pqc-tls          # TLS con ML-KEM-768
sudo enola-cli maintenance ssh-harden-pqc --force  # SSH con sntrup761
```

**¿Qué algoritmo usa la firma post-cuántica?**

ML-DSA-65 (también conocido como Dilithium), estandarizado como NIST FIPS 204 en agosto 2024.
Es un algoritmo de firma digital basado en lattices, con seguridad de nivel 3 NIST
(equivalente a 128 bits post-cuánticos). Las firmas son de ~3.3 KB y la verificación es rápida.

**¿Qué es ML-KEM-768 y por qué lo usa Enola?**

ML-KEM-768 (antes llamado Kyber-768) es el estándar NIST FIPS 203 para intercambio de claves
post-cuántico. Enola lo usa en modo **híbrido** (`X25519MLKEM768`): si el PQC falla, X25519
sigue protegiendo; si llega un computador cuántico, ML-KEM protege. Es la recomendación
de Chrome, Firefox, Cloudflare y las agencias de seguridad europeas.

---

> 📖 Documento técnico completo: ver política PQC
>
> Última actualización: 2026-07-31

## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`verify-downloads.md`](../verify/verify-downloads.md) | Guía de verificación de descargas (incluye PQC) |
| [`SECURITY.md`](SECURITY.md) | Política de seguridad general |
| [`commands.md`](commands.md) | Índice de comandos |
