# Megaplan: Optimización AI-indexing + GitHub para enola-cli-lite

Plan integral — verificado contra la licencia propietaria y con garantía de coste cero — para que crawlers de IA (ChatGPT, Claude, Perplexity, Gemini) y GitHub indexen y entiendan mejor el proyecto: README en inglés, `llms.txt`, ficheros estándar alineados con la licencia, GitHub Pages con docs clave traducidas, y metadatos del repo vía `gh` CLI.

---

## Objetivo

1. Máxima **indexabilidad por IAs**: `llms.txt`/`llms-full.txt`, README principal en inglés, docs clave en inglés, sitio GitHub Pages.
2. **Repo GitHub profesional y coherente con la licencia propietaria**: SECURITY, CHANGELOG, CONTRIBUTING restrictivo, formulario de bugs.
3. **Metadatos del repo optimizados**: topics, descripción, website, releases.

## Revisión de licencia (LICENSE — Source-Visible Proprietary)

El plan se ha verificado contra la licencia y se ajusta así:

- **§5 Divulgación coordinada** (vulnerabilidades SOLO por email, embargo público): el `SECURITY.md` raíz y los formularios de issues **refuerzan** esta cláusula — dirigen todo reporte de seguridad a `salvadorpalmarodriguez@gmail.com` y bloquean su publicación en issues.
- **§2.2/§2.3 No redistribución / no forks competidores**: no se acepta código de terceros → `CONTRIBUTING.md` lo declara explícitamente (evita PRs que legalmente no puedes integrar). Se descartan `CODE_OF_CONDUCT.md` y `CITATION.cff` (pensados para proyectos open-source con comunidad de colaboradores).
- **§3.2 No es open-source**: ningún fichero nuevo usará lenguaje que sugiera open-source ("contributions welcome", badges OSI, etc.). README EN mantendrá badge `license: Proprietary (source-visible)` y nota clara.
- **Publicar docs/Pages**: eres el titular de todos los derechos — publicar tu propia documentación en GitHub Pages no contradice la licencia (las restricciones §2 aplican a terceros, no a ti).
- Se **elimina** el objetivo "community profile al 100%" — ese checklist de GitHub presupone open-source y no aplica a software propietario.

## Garantía de coste cero (sin servicios de pago ni cuotas facturables)

| Servicio usado | Coste | ¿Puede cobrarte al pasar cuota? |
|----------------|-------|--------------------------------|
| Repo público GitHub | Gratis | No |
| GitHub Pages (repo público, deploy from branch) | Gratis | **No** — límite blando de 100 GB/mes de ancho de banda: si se supera, GitHub limita/avisa, **nunca factura automáticamente** (no hay tarjeta asociada) |
| Formularios de issues, SECURITY.md, etc. | Gratis (son ficheros del repo) | No |
| `gh` CLI | Gratis | No |

No se usa nada facturable: **sin** GitHub Actions propias, **sin** LFS, **sin** Codespaces, **sin** Copilot, **sin** servicios externos. El build de Pages lo hace GitHub internamente al pushear a la rama (gratuito e ilimitado en repos públicos).

## Restricciones (de AGENTS.md)

- **PROHIBIDO crear GitHub Actions** propias. Pages se configura con *deploy from branch* (el workflow interno `pages-build-deployment` de GitHub es automático y gratuito, no es CI creado por nosotros).
- **Anti-filtración**: ejecutar `bash docs/dev/audit/run_all.sh --profile cli --strict` antes de cada push. Nada de info del servidor/operator en ningún fichero nuevo.
- Todos los commits **firmados PGP** (`git commit -S`) y verificados.
- No hardcodear dominios del proyecto fuera de lo ya público (GitHub URLs OK).
- No tocar código Rust: esta tarea es 100% documentación + metadatos. `Cargo.toml` solo si se acuerda (keywords/repository/readme — ver Fase 6).

---

## Fase 0 — Corregir incongruencias código↔documentación (PREVIA, ya auditada)

Auditoría estructural completada (src/cli/defs.rs + executor.rs vs README + 26 docs de docs/user/). Resultado: **los docs por familia son muy coherentes**; casi todos los problemas están en el README y en versiones. Corregir ANTES de traducir (Fase 1) para no traducir errores.

**Principio acordado: el código es la fuente de verdad.** Comandos/flags/comportamiento → `defs.rs`+`executor.rs`; versión → `Cargo.toml`. La documentación se adapta al código, nunca al revés.

**Versión canónica decidida: `0.1.0-alpha`** (coincide con el release publicado en `dist/`). Implica:
- `Cargo.toml`: `version = "0.1.0"` → `"0.1.0-alpha"` (1 línea — única excepción aprobada al "no tocar código Rust"; usar `docs/dev/build/bump_version.sh` si aplica).
- `defs.rs:304`: doc-comment con ejemplo `v1.4.0` → `v0.1.0-alpha` (solo texto de ayuda).
- Unificar todos los ejemplos en docs (`v1.4.0`, `v1.0.0`) a `v0.1.0-alpha`.
- Ojo: `verify-downloads.md:202` dice "Desde la versión 1.4.0, cada release incluye una segunda firma" — reescribir sin referencia a versión inexistente.

### A. Errores factuales (el doc contradice al código)

| # | Problema | Ubicación | Realidad en código |
|---|----------|-----------|--------------------|
| A1 | `setup --all` documentado como "core + VPN + PQC TLS" | `README.md:573` | `--all` = core+VPN+**security**; PQC TLS solo con `--pqc-tls` explícito (`executor.rs:3019-3044`) |
| A2 | Versionado inconsistente: badge `0.1.0-alpha`, ejemplos `v1.4.0` y `v1.0.0` | `README.md:3,634`, `commands-simple.md:212`, `verify-downloads.md` (×6), `web/README.md:574`, `install-from-iso.md:56`, `defs.rs:304` | `Cargo.toml` = `0.1.0`. Decidir versión canónica y unificar todos los ejemplos |
| A3 | Badge "rust 1.75+" | `README.md:6` | `rust-toolchain.toml` fija `1.96.0` |

### B. Comandos/flags reales SIN documentar en README

| # | Falta en README | Existe en |
|---|-----------------|-----------|
| B1 | Secciones enteras de los 5 CMS: `drupal`, `ghost`, `magnolia`, `strapi`, `wagtail` (solo aparecen en el árbol, ni TOC ni sección) | `defs.rs` + docs de familia completos |
| B2 | `maintenance ssh-harden-pqc` y `maintenance cleanup` | `defs.rs:1527,1541` |
| B3 | `vpn list`, `vpn peer add-pubkey`, flags `--autostart`, `--sync-firewall`, `--psk`, `--dns`, `--ip` | `defs.rs:1820-1980` |
| B4 | `update apply` | `defs.rs:509` |
| B5 | `tor auth rotate` | `defs.rs:718` |
| B6 | `git create`: flags `--http-port`, `--ssh-port`, `--admin-user`, `--admin-password` | `defs.rs:745-793` (bien documentados en `commands-git.md:34-60`) |
| B7 | `wp create --http-port` | `defs.rs:984-997` |

### C. Única omisión en docs de familia

| # | Problema | Ubicación |
|---|----------|-----------|
| C1 | `git status <nombre>` (subcomando real) no aparece ni en `commands-git.md` ni en README | `defs.rs:807-811` |

### Acciones Fase 0

1. Corregir A1 y A3 en `README.md`; unificar A2 a `0.1.0-alpha` (decidido).
2. Añadir secciones CMS (B1) y comandos faltantes (B2-B7) al `README.md`.
3. Añadir `git status` a `commands-git.md` (C1).
4. Verificar build de docs embebidos (`cargo build`) y auditoría anti-filtración antes del commit.

> **Nota**: al iniciar la implementación, copiar este plan a la raíz del repo como `MEGAPLAN.md` (petición del usuario; en modo planificación solo se puede editar este fichero).

## Fase 1 — README en inglés + español secundario

| Acción | Archivo |
|--------|---------|
| Traducir README actual a inglés → pasa a ser el principal | `README.md` (reescrito en EN) |
| Mover contenido actual en español | `README.es.md` (nuevo) |
| Selector de idioma al inicio de ambos: `**English** · [Español](README.es.md)` | ambos |

Mejoras de contenido en el README EN (aplican a ambos):
- **Primer párrafo denso en keywords** (lo que las IAs citan): "Rust CLI for self-hosting Tor hidden services (.onion), Git servers (Forgejo), CMS (WordPress, Drupal, Ghost, Strapi, Wagtail, Magnolia), file sharing, WireGuard VPN, UFW firewall and AppArmor sandboxing on Debian/Linux — with post-quantum signed releases (ML-DSA)."
- Sección **Features** en bullets escaneables (las IAs extraen listas muy bien).
- Sección **Architecture** breve (hexagonal, módulos) — ayuda a los indexadores de código.
- Bloque de instalación de una línea arriba del todo.
- Badges actualizados + badge de docs (Pages) cuando exista.
- Enlace a `llms.txt` y al sitio Pages.

## Fase 2 — llms.txt + llms-full.txt (estándar llmstxt.org)

| Archivo | Contenido |
|---------|-----------|
| `llms.txt` (raíz) | Formato estándar: `# Enola CLI` + resumen 1 línea + secciones con enlaces anotados a docs clave (quickstart, commands, FAQ, security model, verify-downloads). En inglés. |
| `llms-full.txt` (raíz) | Versión expandida: concatenación curada del contenido esencial (descripción completa, lista de comandos por módulo, conceptos, FAQ) en un único fichero plano que un LLM puede ingerir de una vez. En inglés. |

Los enlaces apuntarán a las URLs de GitHub Pages (Fase 5) para que sean resolubles por crawlers.

## Fase 3 — Ficheros estándar de GitHub (versión alineada con licencia propietaria)

| Archivo nuevo | Contenido |
|---------------|-----------|
| `SECURITY.md` (raíz) | Refleja la §5 de la licencia: reportar vulnerabilidades SOLO a `salvadorpalmarodriguez@gmail.com` en 72h, prohibido publicarlas (embargo hasta remediación). GitHub lo muestra como pestaña "Security policy". Enlace a `docs/user/general/SECURITY.md` y verify-downloads. |
| `CHANGELOG.md` | Formato Keep a Changelog; entrada inicial `v0.1.0-alpha` con features actuales. |
| `CONTRIBUTING.md` | Corto y claro: software propietario source-visible; **no se aceptan contribuciones de código** (PRs de código serán cerrados); bugs no-seguridad vía issues; seguridad SOLO por email. |
| `.github/ISSUE_TEMPLATE/bug_report.yml` | Formulario de bugs con campos obligatorios (versión, OS, comando, output). Aviso destacado: temas de seguridad van por email, no aquí. |
| `.github/ISSUE_TEMPLATE/config.yml` | `blank_issues_enabled: false` (nadie puede saltarse el formulario) + enlaces de contacto: seguridad → email; dudas → FAQ/docs. |

**Notificaciones**: GitHub envía email automático al dueño por cada issue nuevo (comprobar en `github.com/settings/notifications` que "Issues → Email" está activo — paso manual del checklist).

**Descartados** (no aplican a software propietario): `CODE_OF_CONDUCT.md`, `CITATION.cff`, template de feature request (opcional; las peticiones pueden ir por el formulario de bug o email).

Ya existe `PULL_REQUEST_TEMPLATE.md` — se revisará para que indique que no se aceptan PRs de código de terceros.

## Fase 4 — Traducción de docs clave a inglés

Nueva carpeta `docs/en/` con traducciones (los originales ES se mantienen intactos):

| Original (ES) | Traducción (EN) |
|---------------|-----------------|
| `docs/user/guia/quickstart.md` | `docs/en/quickstart.md` |
| `docs/user/general/faq.md` | `docs/en/faq.md` |
| `docs/user/general/concepts.md` | `docs/en/concepts.md` |
| `docs/user/general/commands.md` | `docs/en/commands.md` |
| `docs/user/general/SECURITY.md` | `docs/en/security-model.md` |
| `docs/user/verify/verify-downloads.md` | `docs/en/verify-downloads.md` |

Cada doc EN con nota "Spanish original: [link]" y viceversa. Actualizar `docs/README.md` con sección "English docs".

## Fase 5 — GitHub Pages (deploy from branch, sin Actions propias)

1. **Estructura**: servir desde `main` carpeta `/docs` (opción nativa de Pages). Requiere:
   - `docs/index.md` — landing en inglés (resumen + enlaces a EN docs y a docs ES).
   - `docs/_config.yml` — Jekyll mínimo: `theme: jekyll-theme-cayman` (o `just-the-docs` remoto), `title`, `description` ricos en keywords.
   - Verificar que los `.md` existentes renderizan bien con Jekyll (los enlaces relativos `.md` funcionan en pages con `jekyll-relative-links`, incluido por defecto en Pages).
2. **Activación vía gh CLI**:
   ```bash
   gh api repos/SalvadorPalmaRodriguez/enola-cli-lite/pages -X POST -f 'source[branch]=main' -f 'source[path]=/docs'
   ```
3. **robots-friendly**: Pages es indexable por defecto; añadir `docs/robots.txt` no es necesario (GitHub lo gestiona), pero el sitemap lo genera Jekyll con `jekyll-sitemap` (añadir al `_config.yml`).
4. Actualizar `llms.txt` y README con la URL final `https://salvadorpalmarodriguez.github.io/enola-cli-lite/`.

## Fase 6 — Metadatos del repo vía gh CLI

Comandos que se propondrán para tu aprobación:

```bash
# Descripción + website
gh repo edit SalvadorPalmaRodriguez/enola-cli-lite \
  --description "Rust CLI for self-hosting Tor hidden services, Git (Forgejo), CMS (WordPress, Drupal, Ghost, Strapi, Wagtail), WireGuard VPN, firewall & AppArmor — post-quantum signed releases" \
  --homepage "https://salvadorpalmarodriguez.github.io/enola-cli-lite/"

# Topics (máx 20; GitHub y las IAs los usan para clasificar)
gh repo edit --add-topic rust --add-topic cli --add-topic tor \
  --add-topic hidden-services --add-topic onion --add-topic self-hosting \
  --add-topic wordpress --add-topic drupal --add-topic ghost-cms \
  --add-topic wireguard --add-topic firewall --add-topic apparmor \
  --add-topic post-quantum-cryptography --add-topic forgejo \
  --add-topic docker --add-topic privacy --add-topic linux --add-topic debian
```

Opcional en `Cargo.toml` (aunque `publish = false`, mejora metadata indexada del repo): añadir `repository`, `readme`, `keywords`, `categories`. Se decide en implementación.

Checklist manual (no automatizable por gh): social preview image (1280×640) en Settings → General.

## Fase 7 — Auditoría, commit y verificación

1. `bash docs/dev/audit/run_all.sh --profile cli --strict` → 0 errores.
2. Commits firmados por fase lógica (ej. `docs(readme): english-first README`, `docs(llms): add llms.txt`, `docs(community): add standards`, `docs(en): translate key docs`, `docs(pages): enable GitHub Pages`).
3. `git --no-pager log --show-signature -1` → "Good signature" antes de cada push.
4. Verificar en vivo: Pages online, `llms.txt` accesible, pestaña Security policy visible, formulario de bugs funcionando.

---

## Criterios de aceptación

- [ ] Incongruencias A1-A3, B1-B7 y C1 corregidas (Fase 0) antes de traducir.
- [ ] `README.md` en inglés con selector a `README.es.md`; contenido íntegro conservado.
- [ ] `llms.txt` y `llms-full.txt` en raíz, formato llmstxt.org, enlaces resolubles.
- [ ] `SECURITY.md`, `CHANGELOG.md`, `CONTRIBUTING.md` y formulario de bugs creados, todos coherentes con la licencia propietaria (§2, §3, §5).
- [ ] 6 docs clave traducidos en `docs/en/` con enlaces cruzados ES↔EN.
- [ ] GitHub Pages activo desde `main:/docs` con `_config.yml` + sitemap, sin Actions creadas por nosotros.
- [ ] Descripción, homepage y topics configurados vía `gh`.
- [ ] Auditoría anti-filtración en verde; todos los commits firmados y pusheados.

## Fuera de alcance

- Publicación en crates.io (licencia propietaria, `publish = false`).
- `CODE_OF_CONDUCT.md` y `CITATION.cff` (no aplican a software propietario sin comunidad de colaboradores).
- Cualquier servicio con facturación por cuota (Actions, LFS, Codespaces, servicios externos).
- Cambios en código Rust o en la licencia.
- Traducción completa de los ~26 docs de comandos (gradual, futuro).
- GitHub Actions / CI de cualquier tipo.

## Orden de ejecución y esfuerzo estimado

| # | Fase | Esfuerzo |
|---|------|----------|
| 0 | Corrección incongruencias docs↔código (auditoría hecha) | Medio |
| 1 | README EN + ES | Medio (traducción de ~880 líneas) |
| 2 | llms.txt / llms-full.txt | Bajo |
| 3 | Ficheros estándar (SECURITY, CHANGELOG, CONTRIBUTING, bug form) | Bajo |
| 4 | Traducción docs clave | Alto (6 documentos) |
| 5 | GitHub Pages | Bajo |
| 6 | Metadatos gh CLI | Bajo (requiere tu aprobación de comandos) |
| 7 | Auditoría + commits firmados | Bajo (pinentry interactivo tuyo) |
