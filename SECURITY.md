# Security Policy

## Reporting a Vulnerability

**Do NOT open a public issue for security problems.**

If you discover a security vulnerability, bug, misconfiguration or weakness affecting the security, stability, integrity, confidentiality or availability of Enola CLI, you must report it **privately and exclusively** to:

📧 **salvadorpalmarodriguez@gmail.com**

- Report within **72 hours** of discovery.
- Public disclosure (issues, forums, social media, blogs, conferences) is **prohibited** until the issue has been remediated and written consent is given. This coordinated-disclosure embargo protects users from exploitation and is a binding condition of the [LICENSE](LICENSE) (§5).
- You will receive an acknowledgment and status updates by email.

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.0-alpha (latest release) | ✅ |
| Older builds | ❌ |

Only the latest release published at [GitHub Releases](https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases) receives security fixes. Check for updates with `sudo enola-cli update check`.

## Verifying Downloads

Every release is signed twice:

- **minisign (Ed25519)** — classic signature, verified automatically by the installer.
- **ML-DSA-65 (FIPS 204)** — post-quantum signature, verified offline with the key embedded in the binary:

```bash
enola-cli verify enola-cli-v0.1.0-alpha-x86_64-linux.tar.gz
```

Full guide: [docs/user/verify/verify-downloads.md](docs/user/verify/verify-downloads.md)

## Security Model

- All services bind to `127.0.0.1` only and are exposed exclusively through Tor hidden services.
- Defense in depth: UFW firewall (incl. DOCKER-USER chain), AppArmor profiles, per-instance secrets with 0600 permissions.
- Security self-audit: `sudo enola-cli doctor --security`.

User-facing security model: [docs/user/general/SECURITY.md](docs/user/general/SECURITY.md) · Post-quantum measures: [docs/user/general/quantum-security.md](docs/user/general/quantum-security.md)
