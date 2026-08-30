> **User document:** `docs/en/verify-downloads.md`
> **Version:** 2.0 | **Updated:** 2026-07-31
> **Status:** ✅ **CURRENT — Download Verification Guide**
> **References:** security-model.md, commands.md
> **Spanish original:** [`docs/user/verify/verify-downloads.md`](../user/verify/verify-downloads.md)

# 🔐 Verify Your Enola CLI Download

> How to verify that your copy of Enola CLI is authentic and has not been modified.

---

## Why verify?

When you download software from the internet, there are three risks that verification eliminates:

| Risk | What could happen | Solution |
|------|-------------------|----------|
| **Corruption** | The file was damaged during download | Verify SHA256 (integrity) |
| **Tampering** | Someone intercepted the download and modified the binary | Verify SHA256 (integrity) |
| **Impersonation** | Someone publishes a fake binary from another site | Verify minisign signature (authorship) |

**SHA256** confirms the file is identical to the original. **Minisign** confirms it
was signed by the legitimate author (not an impostor who copied the SHA256).

---

## Quick verification (integrity only)

If you downloaded from the official website, SHA256 verification is sufficient in most cases:

```bash
# 1. Download the CLIENT binary and its hash
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256

# 2. Verify integrity
sha256sum -c enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256
# Expected result:
# enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz: OK ✅
```

**If you see `FAILED`**: the file is corrupt or was modified. Delete it and download again
from the [releases page](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases).

---

## Complete verification (integrity + authorship)

Digital signature verification confirms the binary was created by the original author.
It is recommended if:

- You downloaded from a mirror or third-party link
- You want complete cryptographic verification
- You need guarantees for a production environment

### Step 1: Install minisign

[Minisign](https://jedisct1.github.io/minisign/) is a simple and secure digital signature
tool created by Frank Denis (author of libsodium).

```bash
# Ubuntu 24.04+
sudo apt install minisign

# Ubuntu 22.04 or earlier (not in repos)
wget https://github.com/jedisct1/minisign/releases/download/0.11/minisign-0.11-linux.tar.gz
tar xf minisign-0.11-linux.tar.gz
sudo cp minisign-linux/x86_64/minisign /usr/local/bin/

# macOS
brew install minisign

# Windows
# Download from: https://github.com/jedisct1/minisign/releases
```

### Step 2: Get the public key

The author's public key is available in multiple locations (to make it harder for
an attacker to modify all of them):

| Location | How to get it |
|----------|---------------|
| Repository | `minisign.pub` at the root of the [GitHub repository](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite) |
| Releases | `minisign.pub` included in each GitHub release |
| Inline (copy and paste) | See below |

```
untrusted comment: minisign public key 34B4F35407C4C064
RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e
```

Save this content in a file named `enola.pub`.

**Cross-verification**: if you obtained the key from only one site, compare it with another.
If they all match, the key is authentic.

### Step 3: Download the signature

```bash
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.minisig
```

### Step 4: Verify

```bash
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz -p enola.pub
```

**Expected result:**
```
Signature and comment signature verified
Trusted comment: Enola CLI vX.Y.Z — 2026-04-10
```

**If you see `Signature verification failed`**: the binary was modified after signing,
or the signature doesn't match this file. **Do not use this binary.** Download it again
from the official website.

### Inline key verification (without a .pub file)

If you don't want to save a `.pub` file, you can pass the key directly:

```bash
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz \
  -P RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e
```

---

## Automated verification script

You can use this script to automate the entire verification:

```bash
#!/bin/bash
# verify_enola.sh — Verify integrity and authorship of Enola CLI
# Usage: bash verify_enola.sh enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz

FILE="${1:?Usage: bash verify_enola.sh <file.tar.gz>}"
PUBKEY="RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e"

echo "🔍 Verifying: $FILE"
echo ""

# Step 1: SHA256
if [ -f "${FILE}.sha256" ]; then
    if sha256sum -c "${FILE}.sha256" 2>/dev/null; then
        echo "✅ SHA256 integrity: OK"
    else
        echo "❌ SHA256 integrity: FAILED — file corrupt or modified"
        exit 1
    fi
else
    echo "⚠️  .sha256 file not found — skipping integrity verification"
fi

echo ""

# Step 2: Digital signature
if command -v minisign &>/dev/null; then
    if [ -f "${FILE}.minisig" ]; then
        if minisign -Vm "$FILE" -P "$PUBKEY" 2>/dev/null; then
            echo "✅ Digital signature: VERIFIED"
        else
            echo "❌ Digital signature: FAILED — binary not authentic"
            exit 1
        fi
    else
        echo "⚠️  .minisig file not found — skipping signature verification"
    fi
else
    echo "ℹ️  minisign not installed — only SHA256 was verified"
    echo "   To verify signature: apt install minisign (or see docs)"
fi

echo ""
echo "✅ Verification complete"
```

---

## Release files

Each Enola CLI release includes the client artifact for users:

```
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz          ← Public binary for users
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz.sha256   ← SHA256 hash (integrity)
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz.minisig  ← Minisign signature (authorship)
```

| File | What it verifies | Tool |
|------|-----------------|------|
| `.sha256` | That the file was not modified | `sha256sum` (included with Linux) |
| `.minisig` | That it was signed by the author | `minisign` (install separately) |

---

## Post-quantum verification (ML-DSA-65 signature)

Each release includes a **second signature** based on the
post-quantum algorithm **ML-DSA-65** (FIPS 204, also known as Dilithium). This signature
protects against future attacks from quantum computers.

### Why a second signature?

The classic signature (minisign/Ed25519) is secure today, but a future quantum computer
could break it. The ML-DSA-65 signature **resists quantum attacks** according to the
NIST FIPS 204 standard. With both signatures you have dual guarantees:

| Signature | Algorithm | Secure today? | Quantum-resistant? |
|-----------|-----------|---------------|-------------------|
| `.minisig` | Ed25519 | ✅ Yes | ❌ No |
| `.pqsig` | ML-DSA-65 | ✅ Yes | ✅ Yes |

### Release files (with PQC signature)

```
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz          ← Public binary for users
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz.sha256   ← SHA256 hash (integrity)
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz.minisig  ← Classic signature (Ed25519)
enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz.pqsig    ← Post-quantum signature (ML-DSA-65)
```

### Step 1: Verify with `enola-cli` itself (recommended)

You don't need external tools or the public key: `enola-cli` has the
ML-DSA-65 public key **embedded** and verifies the post-quantum signature with a
single command (no network or login required):

```bash
# Download the release and its PQC signature
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
wget https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.pqsig

# Verify (uses embedded public key and, if present, the sibling .sha256)
enola-cli verify enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
```

**Expected result:**
```
Release verification: enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
  ✅ Post-quantum ML-DSA-65 signature (embedded public key) — valid signature
  ✅ SHA-256 integrity — hash matches

✅ The file is legitimate: signed by the Enola project.
```

**If you see `❌`**: the binary was modified or the signature is not authentic.
**Do not use this binary.** Download it again from the official website.

> `enola-cli verify` returns exit code `21` if verification fails
> (useful for scripts). Accepts `--json`, `--pqsig <path>`, and `--pubkey <path>`.
>
> The `enola-sign-pqc` tool is **NOT** distributed to users: it's a
> development tool for *signing* releases (feature `dev-tools`). To
> *verify*, `enola-cli verify` is sufficient.

### Complete verification (all 3 layers)

For maximum security, verify all three layers:

```bash
# 1. Integrity (SHA256)
sha256sum -c enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz.sha256

# 2. Classic authorship (minisign/Ed25519)
minisign -Vm enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz -p enola.pub

# 3. Post-quantum authorship (ML-DSA-65) — embedded public key in enola-cli
enola-cli verify enola-cli-vX.Y.Z-x86_64-linux-client.tar.gz
```

If all three verifications pass: ✅ the binary is authentic, intact, and resistant to quantum attacks.

### Automated verification script (updated with PQC)

```bash
#!/bin/bash
# verify_enola.sh — Verify integrity, authorship, and PQC signature
# Usage: bash verify_enola.sh enola-cli-v0.2.0-alpha-x86_64-linux.tar.gz

FILE="${1:?Usage: bash verify_enola.sh <file.tar.gz>}"
PUBKEY="RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e"

echo "🔍 Verifying: $FILE"
echo ""

# Step 1: SHA256
if [ -f "${FILE}.sha256" ]; then
    if sha256sum -c "${FILE}.sha256" 2>/dev/null; then
        echo "✅ SHA256 integrity: OK"
    else
        echo "❌ SHA256 integrity: FAILED"
        exit 1
    fi
else
    echo "⚠️  .sha256 not found"
fi

echo ""

# Step 2: Classic signature (minisign)
if command -v minisign &>/dev/null; then
    if [ -f "${FILE}.minisig" ]; then
        if minisign -Vm "$FILE" -P "$PUBKEY" 2>/dev/null; then
            echo "✅ Classic signature (Ed25519): VERIFIED"
        else
            echo "❌ Classic signature: FAILED"
            exit 1
        fi
    else
        echo "⚠️  .minisig not found"
    fi
else
    echo "ℹ️  minisign not installed — skipping classic signature"
fi

echo ""

# Step 3: Post-quantum signature (ML-DSA-65) — via enola-cli (embedded key)
if command -v enola-cli &>/dev/null; then
    if [ -f "${FILE}.pqsig" ]; then
        if enola-cli verify "$FILE"; then
            echo "✅ Post-quantum signature (ML-DSA-65): VERIFIED"
        else
            echo "❌ Post-quantum signature: FAILED"
            exit 1
        fi
    else
        echo "⚠️  .pqsig not found"
    fi
else
    echo "ℹ️  enola-cli not available — skipping PQC signature"
fi

echo ""
echo "✅ Verification complete"
```

---

## What to do if verification fails?

| Situation | What to do |
|-----------|------------|
| SHA256: `FAILED` | Corrupt download. Delete and download again. |
| Minisign: `verification failed` | Tampered binary. **Do not use it.** Download again from the official website. |
| If it fails repeatedly | Contact the author. Your network may be compromised. |
| `minisign: command not found` | Install minisign (see Step 1 above). SHA256 verification still works without it. |

---

## FAQ

### Is verification mandatory?
For **manual downloads** from GitHub Releases: not mandatory but
highly recommended, especially if you use Enola to deploy services
with sensitive data.

For the **auto-update mechanism** (`enola-cli update download/apply`):
minisign verification is **mandatory**.
If minisign is not installed or the signature doesn't verify, the update is rejected.
See `docs/user/update/commands-update.md` for details.

### Isn't SHA256 enough?
SHA256 verifies the file wasn't modified, but an attacker who compromises
the server could change BOTH the binary and the .sha256. The minisign signature
is independent — it's verified with a key the attacker doesn't have.

### Why minisign and not GPG?
Minisign is simpler, more secure by default, and doesn't require managing a
complex keyring. It's the modern standard for signing software (used by OpenBSD,
Zig, WireGuard, among others).

### Where is the private key?
The private key is NEVER published or included in the repository.
Only the author has access to it. The public key is sufficient for verification.

### What happens if the operator rotates the minisign key?
The operator can announce a new minisign key in the advisory feed
(`next_pubkey` field), signed with the current key. The client verifies it
automatically and persists it in `~/.enola/trusted_minisign_keys.json` (0600).
Subsequent verifications use the new key. If the rotation signature
doesn't verify, it's ignored and the previous key is used.
See `docs/user/update/commands-update.md` § Minisign key rotation.

---

*Document created: 2026-04-11 | Public key: `RWRkwMQHVPO0NGUahoNT1sLqJKM8QzlkfOOmSM0P+80x80GIw9P7BB8e`*

## Cross-references

| Document | Purpose |
|----------|---------|
| [`security-model.md`](security-model.md) | General security policy |
| [`concepts.md`](concepts.md) | Key concepts (Tor, PQC, security) |
| `docs/user/update/commands-update.md` | Binary update commands |
