> **Documento usuario:** `docs/user/maintenance/commands-maintenance.md`
> **Versión:** 2.4 | **Actualizado:** 2026-11-03
> **Estado:** ✅ **VIGENTE — Guía de usuario**
> **Referencias:** commands.md

# 🔧 Maintenance — Comandos `enola-cli maintenance`

Operaciones de mantenimiento del sistema: estado, smoke tests, health checks,
hardening SSH, backups y limpieza.

---

## `maintenance status`

Muestra el estado general del sistema.

```bash
sudo enola-cli maintenance status
```

Sin flags ni argumentos.

---

## `maintenance smoke-test`

Ejecuta un smoke test del sistema.

```bash
sudo enola-cli maintenance smoke-test
```

Sin flags ni argumentos.

---

## `maintenance enable-checks`

Habilita los health checks automáticos.

```bash
sudo enola-cli maintenance enable-checks
```

Sin flags ni argumentos.

---

## `maintenance disable-checks`

Deshabilita los health checks automáticos.

```bash
sudo enola-cli maintenance disable-checks
```

Sin flags ni argumentos.

---

## `maintenance timer-status`

Muestra el estado del timer de systemd para los health checks automáticos.

```bash
sudo enola-cli maintenance timer-status
```

Sin flags ni argumentos.

---

## `maintenance ssh-config`

Configura el check de SSH.

```bash
sudo enola-cli maintenance ssh-config
```

Sin flags ni argumentos.

---

## `maintenance ssh-harden-pqc`

Endurece la configuración SSH con algoritmos post-cuánticos.
Añade `sntrup761x25519-sha512` KEX (OpenSSH 9.0+) como primer algoritmo preferido.

```bash
sudo enola-cli maintenance ssh-harden-pqc [--force] [--dry-run]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--force` / `-f` | Bool | Aplica cambios sin confirmación |
| `--dry-run` | Bool | Muestra qué cambiaría sin aplicar |

> Medida transicional hasta que el PQC completo se estandarice. Ejecuta de nuevo tras actualizar OpenSSH.

---

## `maintenance backup`

Crea un backup de la **identidad** de la máquina como `tar.gz` en
`/var/backups/enola-server/`, preservando las rutas absolutas para que la
restauración recoloque cada elemento en su sitio. Incluye lo necesario para
reconstruir tus servicios `.onion`:

| Ruta | Qué es |
|------|--------|
| `/var/lib/tor` | Claves de los servicios `.onion` (perderlas = perder la dirección) |
| `/etc/tor/enola.d` | Fragmentos de Tor que declaran cada servicio |
| `/etc/nginx/sites-available`, `sites-enabled` | Sitios de Nginx |
| `/etc/nginx/ssl` | Certificados y claves TLS |
| `/etc/enola` | Configuración de Enola (incluye el manifiesto del backup) |
| `/opt/enola`, `/usr/local/share/enola/manifest` | Contextos de build y manifiesto de recursos |
| `/etc/apparmor.d/enola-*` | Perfiles AppArmor de Enola |

**No incluye** los datos de tus servicios (`/srv/enola-*`: WordPress, Git,
ficheros…): cópialos aparte (ver la guía de ejemplos). Tampoco `~/.enola` ni las
claves de confianza de actualizaciones.

El archivo lleva un manifiesto (`etc/enola/backup-manifest.json`) con la versión
del formato y la de `enola-cli`.

**Espacio en disco:** antes de empezar, el comando mide las carpetas y exige
espacio libre suficiente (tamaño de las carpetas + 100 MiB de reserva). Si no
hay, **no crea nada** y lo dice; si `tar` falla a mitad, borra el archivo
parcial.

Los backups contienen credenciales y configuración sensible: el directorio
queda en `0700` y cada artefacto (`*.tar.gz`, `*.bak`) en `0600`.

```bash
sudo enola-cli maintenance backup [--keep <N>]
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--keep` | usize | Conserva como máximo N backups por servicio solo en esta ejecución |

Tras crear el backup se rotan los antiguos del mismo identificador según la
política de retención (ver `maintenance backup-config`).

---

## `maintenance restore`

Restaura la identidad de la máquina (claves `.onion`, Tor, Nginx, certificados,
configuración de Enola) desde un backup hecho con `maintenance backup`.

```bash
# 1. Simulación (por defecto): valida el archivo y dice qué se sobrescribiría
sudo enola-cli maintenance restore /var/backups/enola-server/2026-11-03_21-00-00_system_full.tar.gz

# 2. Restaurar de verdad
sudo enola-cli maintenance restore /var/backups/enola-server/<archivo>.tar.gz --force
```

| Argumento / flag | Descripción |
|------------------|-------------|
| `<archivo>` | Backup `.tar.gz` creado por `maintenance backup` |
| `--force` | Aplica el restore. Sin él solo se muestra lo que haría |

Qué hace y qué **no** hace:

- **Solo restaura ficheros.** No para ni arranca nada: al terminar te dice qué
  reiniciar (`sudo systemctl restart tor`, Nginx, tus contenedores…) y lo haces tú
  cuando quieras.
- **Valida todo antes de escribir.** Si una sola entrada del archivo es sospechosa
  (ruta fuera de las carpetas de identidad, `..`, enlace duro, fichero especial,
  enlace simbólico que apunta fuera, escritura a través de un enlace), **rechaza el
  archivo entero** y no toca nada.
- **Guarda lo que hay antes de sobrescribir:** crea una copia `pre-restore` en
  `/var/backups/enola-server/` (con la misma comprobación de espacio en disco).
- Exige que el archivo lleve manifiesto y de la misma versión de formato; un backup
  hecho por una versión anterior no se puede restaurar.
- Repone los propietarios `root` y `debian-tor` (Tor debe estar instalado); cualquier
  otro propietario se restaura como `root`, y se descartan los bits setuid/setgid.
- **Solo desde la terminal:** no está disponible en el panel web ni por MCP.

---

## `maintenance backup-config`

Muestra o fija la política de retención de backups (cuántos se conservan por
servicio antes de borrar los más antiguos).

```bash
# Ver el valor efectivo y su origen
sudo enola-cli maintenance backup-config

# Persistir un nuevo valor en ~/.enola/config.toml → [backup].max_backups
sudo enola-cli maintenance backup-config --max-backups 3
```

| Flag | Tipo | Descripción |
|------|------|-------------|
| `--max-backups` | usize | Nº máximo de backups por servicio (mínimo 1). Omitir para solo consultar |

Prioridad de resolución (de mayor a menor): flag `--keep` de
`maintenance backup` > variable `ENOLA_MAX_BACKUPS` > `config.toml`
`[backup].max_backups` > valor por defecto (5).

> La configuración se guarda en el `~/.enola/config.toml` del usuario que
> invoca `sudo`, no en `/root/.enola/`, y conserva su propiedad.

---

## `maintenance cleanup`

Limpia archivos temporales y datos residuales.

```bash
sudo enola-cli maintenance cleanup [--target <TARGET>] [--dry-run] [--force] [--keep-days <DÍAS>]
```

| Flag | Tipo | Default | Descripción |
|------|------|---------|-------------|
| `--target` / `-t` | String | `all` | Objetivo: `all`, `logs`, `docker` |
| `--dry-run` | Bool | `false` | Muestra qué se borraría sin borrar |
| `--force` / `-f` | Bool | `false` | Limpia sin confirmación |
| `--keep-days` | u32 | `7` | Días de logs a conservar |

**Ejemplos:**
```bash
sudo enola-cli maintenance cleanup --dry-run
sudo enola-cli maintenance cleanup --target logs --keep-days 30
sudo enola-cli maintenance cleanup --force
```

---

## Ver también

- [Referencia de comandos](../general/commands.md) — catálogo completo de comandos.
- [Conceptos](../general/concepts.md) — arquitectura general (Tor, Nginx, Docker, secrets).

---
