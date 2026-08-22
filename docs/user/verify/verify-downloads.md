> **Documento usuario:** `docs/user/verify/verify-downloads.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de verificación de descargas**
> **Referencias:** SIGNING_GUIDE.md, quantum-security.md, SECURITY.md
> **English:** [`docs/en/verify-downloads.md`](../../en/verify-downloads.md)

# 🔐 Verificar tu Descarga de Enola CLI

> Cómo comprobar que tu copia de Enola CLI es auténtica y no ha sido modificada.

---

## ¿Por qué verificar?

Cuando descargas software de internet, hay tres riesgos que la verificación elimina:

| Riesgo | Qué puede pasar | Solución |
|--------|-----------------|----------|
| **Corrupción** | El archivo se dañó durante la descarga | Verificar SHA256 (integridad) |
| **Manipulación** | Alguien interceptó la descarga y modificó el binario | Verificar SHA256 (integridad) |
| **Impostor** | Alguien publica un binario falso desde otro sitio | Verificar firma minisign (autoría) |

**SHA256** confirma que el archivo es idéntico al original. **Minisign** confirma que
fue firmado por el autor legítimo (no por un impostor que copió el SHA256).

---

## Verificación rápida (solo integridad)

Si descargaste desde la web oficial, la verificación SHA256 es suficiente en la mayoría de casos:

```bash
# 1. Descarga el binario CLIENTE y su hash
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256

# 2. Verifica integridad
sha256sum -c enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256
# Resultado esperado:
# enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz: OK ✅
```

**Si ves `FAILED`**: el archivo está corrupto o fue modificado. Elimínalo y descárgalo de nuevo
desde la [página de releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases).

---

## Verificación completa (integridad + autoría)

La verificación con firma digital confirma que el binario fue creado por el autor original.
Es recomendable si:

- Descargaste desde un mirror o enlace de terceros
- Quieres verificación criptográfica completa
- Necesitas garantías para un entorno de producción

### Paso 1: Instalar minisign

[Minisign](https://jedisct1.github.io/minisign/) es una herramienta de firma digital
simple y segura creada por Frank Denis (autor de libsodium).

```bash
# Ubuntu 24.04+
sudo apt install minisign

# Ubuntu 22.04 o anterior (no está en los repos)
wget https://github.com/jedisct1/minisign/releases/download/0.11/minisign-0.11-linux.tar.gz
tar xf minisign-0.11-linux.tar.gz
sudo cp minisign-linux/x86_64/minisign /usr/local/bin/

# macOS
brew install minisign

# Windows
# Descargar desde: https://github.com/jedisct1/minisign/releases
```

### Paso 2: Obtener la clave pública

La clave pública del autor está disponible en múltiples ubicaciones (para dificultar
que un atacante las modifique todas):

| Ubicación | Cómo obtenerla |
|-----------|---------------|
| Repositorio | `minisign.pub` en la raíz del [repositorio GitHub](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite) |
| Releases | `minisign.pub` incluido en cada release de GitHub |
| Inline (copiar y pegar) | Ver abajo |

```
untrusted comment: minisign public key 45E47537137B222A
RWQqInsTN3XkRQKbGZ7pTsGnumqh5uLbZLYOFTQ7ku3SmgiDgOgxnNPP
```

Guarda este contenido en un archivo llamado `enola.pub`.

**Verificación cruzada**: si obtuviste la clave de un solo sitio, compárala con otro.
Si todas coinciden, la clave es auténtica.

### Paso 3: Descargar la firma

```bash
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.minisig
```

### Paso 4: Verificar

```bash
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz -p enola.pub
```

**Resultado esperado:**
```
Signature and comment signature verified
Trusted comment: Enola CLI vX.Y.Z — 2026-04-10
```

**Si ves `Signature verification failed`**: el binario fue modificado después de ser firmado,
o la firma no corresponde a este archivo. **No uses ese binario.** Descárgalo de nuevo
desde la web oficial.

### Verificación con clave inline (sin archivo .pub)

Si no quieres guardar un archivo `.pub`, puedes pasar la clave directamente:

```bash
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz \
  -P RWQqInsTN3XkRQKbGZ7pTsGnumqh5uLbZLYOFTQ7ku3SmgiDgOgxnNPP
```

---

## Script de verificación automática

Puedes usar este script para automatizar toda la verificación:

```bash
#!/bin/bash
# verify_enola.sh — Verifica integridad y autoría de Enola CLI
# Uso: bash verify_enola.sh enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz

FILE="${1:?Uso: bash verify_enola.sh <archivo.tar.gz>}"
PUBKEY="RWQqInsTN3XkRQKbGZ7pTsGnumqh5uLbZLYOFTQ7ku3SmgiDgOgxnNPP"

echo "🔍 Verificando: $FILE"
echo ""

# Paso 1: SHA256
if [ -f "${FILE}.sha256" ]; then
    if sha256sum -c "${FILE}.sha256" 2>/dev/null; then
        echo "✅ Integridad SHA256: OK"
    else
        echo "❌ Integridad SHA256: FALLÓ — archivo corrupto o modificado"
        exit 1
    fi
else
    echo "⚠️  Archivo .sha256 no encontrado — saltando verificación de integridad"
fi

echo ""

# Paso 2: Firma digital
if command -v minisign &>/dev/null; then
    if [ -f "${FILE}.minisig" ]; then
        if minisign -Vm "$FILE" -P "$PUBKEY" 2>/dev/null; then
            echo "✅ Firma digital: VERIFICADA"
        else
            echo "❌ Firma digital: FALLÓ — binario no auténtico"
            exit 1
        fi
    else
        echo "⚠️  Archivo .minisig no encontrado — saltando verificación de firma"
    fi
else
    echo "ℹ️  minisign no instalado — solo se verificó SHA256"
    echo "   Para verificar la firma: apt install minisign (o ver docs)"
fi

echo ""
echo "✅ Verificación completada"
```

---

## Archivos de una release

Cada release de Enola CLI incluye el artefacto client para usuarios:

```
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz          ← Binario público para usuarios
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz.sha256   ← Hash SHA256 (integridad)
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz.minisig  ← Firma minisign (autoría)
```

| Archivo | Qué verifica | Herramienta |
|---------|-------------|-------------|
| `.sha256` | Que el archivo no fue modificado | `sha256sum` (viene con Linux) |
| `.minisig` | Que fue firmado por el autor | `minisign` (instalar aparte) |

---

## Verificación post-cuántica (firma ML-DSA-65)

Cada release incluye una **segunda firma** basada en el algoritmo
post-cuántico **ML-DSA-65** (FIPS 204, también conocido como Dilithium). Esta firma
protege contra futuros ataques de computadores cuánticos.

### ¿Por qué una segunda firma?

La firma clásica (minisign/Ed25519) es segura hoy, pero un futuro computador cuántico
podría romperla. La firma ML-DSA-65 **resiste ataques cuánticos** según el estándar
NIST FIPS 204. Con ambas firmas tienes doble garantía:

| Firma | Algoritmo | ¿Segura hoy? | ¿Resistente a cuántica? |
|-------|-----------|--------------|------------------------|
| `.minisig` | Ed25519 | ✅ Sí | ❌ No |
| `.pqsig` | ML-DSA-65 | ✅ Sí | ✅ Sí |

### Archivos de una release (con firma PQC)

```
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz          ← Binario público para usuarios
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz.sha256   ← Hash SHA256 (integridad)
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz.minisig  ← Firma clásica (Ed25519)
enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz.pqsig    ← Firma post-cuántica (ML-DSA-65)
```

### Paso 1: Verificar con el propio `enola-cli` (recomendado)

No necesitas herramientas externas ni la clave pública: `enola-cli` lleva la
clave pública ML-DSA-65 **embebida** y verifica la firma post-cuántica con un
solo comando (no requiere red ni login):

```bash
# Descarga el release y su firma PQC
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.pqsig

# Verificar (usa la clave pública embebida y, si existe, el .sha256 hermano)
enola-cli verify enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
```

**Resultado esperado:**
```
Verificación de release: enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
  ✅ Firma post-cuántica ML-DSA-65 (clave pública embebida) — firma válida
  ✅ Integridad SHA-256 — hash coincide

✅ El archivo es legítimo: firmado por el proyecto Enola.
```

**Si ves `❌`**: el binario fue modificado o la firma no es auténtica.
**No uses ese binario.** Descárgalo de nuevo desde la web oficial.

> `enola-cli verify` devuelve código de salida `21` si la verificación falla
> (útil para scripts). Acepta `--json`, `--pqsig <ruta>` y `--pubkey <ruta>`.
>
> La herramienta `enola-sign-pqc` **NO** se distribuye al usuario: es una
> herramienta de desarrollo para *firmar* releases (feature `dev-tools`). Para
> *verificar* basta con `enola-cli verify`.

### Verificación completa (las 3 capas)

Para máxima seguridad, verifica las tres capas:

```bash
# 1. Integridad (SHA256)
sha256sum -c enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256

# 2. Autoría clásica (minisign/Ed25519)
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz -p enola.pub

# 3. Autoría post-cuántica (ML-DSA-65) — clave pública embebida en enola-cli
enola-cli verify enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
```

Si las tres verificaciones pasan: ✅ el binario es auténtico, íntegro y resistente a ataques cuánticos.

### Script de verificación automática (actualizado con PQC)

```bash
#!/bin/bash
# verify_enola.sh — Verifica integridad, autoría y firma PQC
# Uso: bash verify_enola.sh enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz

FILE="${1:?Uso: bash verify_enola.sh <archivo.tar.gz>}"
PUBKEY="RWQqInsTN3XkRQKbGZ7pTsGnumqh5uLbZLYOFTQ7ku3SmgiDgOgxnNPP"

echo "🔍 Verificando: $FILE"
echo ""

# Paso 1: SHA256
if [ -f "${FILE}.sha256" ]; then
    if sha256sum -c "${FILE}.sha256" 2>/dev/null; then
        echo "✅ Integridad SHA256: OK"
    else
        echo "❌ Integridad SHA256: FALLÓ"
        exit 1
    fi
else
    echo "⚠️  .sha256 no encontrado"
fi

echo ""

# Paso 2: Firma clásica (minisign)
if command -v minisign &>/dev/null; then
    if [ -f "${FILE}.minisig" ]; then
        if minisign -Vm "$FILE" -P "$PUBKEY" 2>/dev/null; then
            echo "✅ Firma clásica (Ed25519): VERIFICADA"
        else
            echo "❌ Firma clásica: FALLÓ"
            exit 1
        fi
    else
        echo "⚠️  .minisig no encontrado"
    fi
else
    echo "ℹ️  minisign no instalado — saltando firma clásica"
fi

echo ""

# Paso 3: Firma post-cuántica (ML-DSA-65) — vía enola-cli (clave embebida)
if command -v enola-cli &>/dev/null; then
    if [ -f "${FILE}.pqsig" ]; then
        if enola-cli verify "$FILE"; then
            echo "✅ Firma post-cuántica (ML-DSA-65): VERIFICADA"
        else
            echo "❌ Firma post-cuántica: FALLÓ"
            exit 1
        fi
    else
        echo "⚠️  .pqsig no encontrado"
    fi
else
    echo "ℹ️  enola-cli no disponible — saltando firma PQC"
fi

echo ""
echo "✅ Verificación completada"
```

---

## ¿Qué hacer si la verificación falla?

| Situación | Qué hacer |
|-----------|----------|
| SHA256: `FAILED` | Descarga corrupta. Elimina y descarga de nuevo. |
| Minisign: `verification failed` | Binario manipulado. **No lo uses.** Descarga de nuevo desde la web oficial. |
| Si falla repetidamente | Contacta al autor. Tu red podría estar comprometida. |
| `minisign: command not found` | Instala minisign (ver Paso 1 arriba). La verificación SHA256 sigue funcionando sin él. |

---

## Preguntas frecuentes

### ¿Es obligatorio verificar?
Para **descargas manuales** desde GitHub Releases: no es obligatorio pero es
altamente recomendable, especialmente si usas Enola para desplegar servicios
con datos sensibles.

Para el **mecanismo de auto-update** (`enola-cli update download/apply`):
la verificación minisign es **obligatoria**.
Si minisign no está instalado o la firma no verifica, la actualización se rechaza.
Ver `docs/user/update/commands-update.md` para detalles.

### ¿SHA256 no es suficiente?
SHA256 verifica que el archivo no se modificó, pero un atacante que comprometa
el servidor podría cambiar TANTO el binario como el .sha256. La firma minisign
es independiente — se verifica con una clave que el atacante no tiene.

### ¿Por qué minisign y no GPG?
Minisign es más simple, más seguro por defecto, y no requiere gestionar un
keyring complejo. Es el estándar moderno para firmar software (lo usa OpenBSD,
Zig, WireGuard, entre otros).

### ¿Dónde está la clave privada?
La clave privada NUNCA se publica ni se incluye en el repositorio.
Solo el autor tiene acceso a ella. La clave pública es suficiente para verificar.

### ¿Qué pasa si el operador rota la clave minisign?
El operador puede anunciar una nueva clave minisign en el feed de advisories
(campo `next_pubkey`), firmada con la clave actual. El cliente la verifica
automáticamente y la persiste en `~/.enola/trusted_minisign_keys.json` (0600).
Las verificaciones posteriores usan la nueva clave. Si la firma de rotación
no verifica, se ignora y se continúa con la clave anterior.
Ver `docs/user/update/commands-update.md` § Rotación de clave minisign.

---

*Documento creado: 2026-04-11 | Clave pública: `RWQqInsTN3XkRQKbGZ7pTsGnumqh5uLbZLYOFTQ7ku3SmgiDgOgxnNPP`*




## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`quantum-security.md`](../general/quantum-security.md) | Seguridad post-cuántica en Enola CLI |
| [`SECURITY.md`](../general/SECURITY.md) | Política de seguridad general |
| [`commands-update.md`](../update/commands-update.md) | Comandos de actualización del binario |
