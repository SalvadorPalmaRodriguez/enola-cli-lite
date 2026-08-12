> **Documento usuario:** `docs/user/uninstall/uninstall.md`
> **Versión:** 4.0 | **Actualizado:** 2026-08-03
> **Estado:** ✅ **VIGENTE — Guía de desinstalación**
> **Referencias:** commands-simple.md (comando `uninstall`)

# Desinstalar Enola CLI

Guía oficial para retirar Enola CLI de tu sistema de forma limpia y segura.

El proceso es **atómico**: primero se muestra qué se va a borrar (dry-run por defecto),
y solo con `--yes` se ejecutan las acciones.

> ⚠️ **Importante**: La desinstalación borra contenedores Docker, configuraciones
> de Nginx/Tor/UFW y (opcionalmente) los datos de tus servicios. Haz backup antes
> si tienes sitios WordPress o repos Git que quieras conservar.

---

## Desinstalación rápida (recomendada)

```bash
# 1. Dry-run — muestra qué se borraría, NO modifica nada
sudo enola-cli uninstall

# 2. Borrado real
sudo enola-cli uninstall --yes
```

## Opciones

| Opción | Qué hace |
|--------|----------|
| *(sin `--yes`)* | Dry-run por defecto: solo lista, no borra |
| `--yes` | Ejecuta el borrado real |
| `--keep-data` | Preserva `/srv/enola-*` y `~/.enola/` |
| `--only X,Y` | Borra solo categorías: `binary,config,services,tor,nginx,systemd,apparmor,docker,ufw,data,deps` |
| `--force` | Continuar ante errores no críticos (servicios no instalados) |
| `--remove-deps` | **También desinstala dependencias de terceros que Enola instaló** (Docker, Tor, Nginx, UFW). Solo borra las que Enola instaló según el manifiesto. Las que tú ya tenías **NUNCA** se tocan. |

## Qué se borra

El desinstalador usa el **manifiesto** (`/usr/local/share/enola/manifest`) como fuente de verdad para borrar exactamente lo que Enola creó. Si no hay manifiesto, usa detección heurística por prefijos.

1. **Docker**: contenedores registrados en el manifiesto (`docker_container`) + fallback por prefijos `enola-*`, `wp-*`, `db-*`, `strapi-*`, `wagtail-*`, `ghost-*`, `magnolia-*`, `drupal-*`; redes registradas (`docker_network`) + fallback `enola-*`, `strapi-*`, `wagtail-*`; imágenes `enola-*`; volúmenes `enola-*`.
2. **Nginx**: configs registradas en el manifiesto (`nginx_config`) + fallback `proxy_*`, `fileserver_*` en sites-enabled/available y conf.d + recarga nginx.
3. **Tor**: servicios registrados en el manifiesto (`tor_service`) → borra `/var/lib/tor/enola_{name}` y `/etc/tor/enola.d/{name}.conf` + fallback globbing + limpieza de torrc.
4. **UFW**: puertos registrados en el manifiesto (`ufw_rule`) → borra reglas TCP loopback y UDP exactas + fallback por comentario `# enola-cli`.
5. **systemd**: timers `enola-*` + servicios VPN registrados (`vpn_service` → `wg-quick@{name}`).
6. **AppArmor**: perfiles registrados en el manifiesto (`apparmor_profile`) + fallback `enola-*` en `/etc/apparmor.d/`.
7. **Binario**: `/usr/local/bin/enola-cli` + `/usr/local/share/enola/` + `/opt/enola/`.
8. **Datos** (sin `--keep-data`): `/srv/enola-*` + configs VPN (`vpn_config` → `/etc/wireguard/{name}.conf`) + certificados SSL (`ssl_cert`, `ssl_key`).
9. **Config usuario**: `~/.enola/` (session, config, config.toml, trusted_minisign_keys.json).

> **Tus herramientas están seguras**: por defecto el desinstalador NO ejecuta `apt remove` ni desinstala Docker, Tor, Nginx, UFW, Rust, Python o minisign. Solo con `--remove-deps` desinstala las dependencias que **Enola instaló** (según el manifiesto). Las que tú ya tenías antes de instalar Enola **NUNCA** se tocan, aunque uses `--remove-deps`.

## Manifiesto de instalación

Cuando instalas Enola CLI, `install.sh` genera un manifiesto en `/usr/local/share/enola/manifest`. Además, el propio CLI registra automáticamente en el manifiesto cada recurso que crea en runtime:

- **Instalación** (`install.sh`): rutas de binario, share_dir, config_dir, opt_dir, dependencias instaladas.
- **Runtime** (CLI): contenedores Docker (`docker_container`), redes Docker (`docker_network`), configs Nginx (`nginx_config`), certificados SSL (`ssl_cert`, `ssl_key`), servicios Tor (`tor_service`), reglas UFW (`ufw_rule`), configs VPN (`vpn_config`), servicios systemd VPN (`vpn_service`), perfiles AppArmor (`apparmor_profile`).

El desinstalador lee este manifiesto para saber **exactamente** qué borrar. Si no hay manifiesto (instalación antigua), usa detección heurística por prefijos.

### `--remove-deps`: desinstalar dependencias de terceros

```bash
# Ver qué deps instaló Enola (dry-run)
sudo enola-cli uninstall

# Desinstalar Enola + deps que Enola instaló
sudo enola-cli uninstall --yes --remove-deps

# Solo desinstalar deps (sin borrar Enola)
sudo enola-cli uninstall --yes --only deps --remove-deps
```

**Seguridad garantizada**:
- Si Docker ya estaba instalado antes de Enola → **NO se desinstala**
- Si Tor ya estaba instalado antes de Enola → **NO se desinstala**
- Si Nginx ya estaba instalado antes de Enola → **NO se desinstala**
- Si UFW ya estaba instalado antes de Enola → **NO se desinstala**
- Sin manifiesto → `--remove-deps` se niega a actuar (no puede saber qué instaló Enola)

## Desinstalación parcial

```bash
# Solo Tor
sudo enola-cli uninstall --yes --only tor

# Docker + Nginx, conservando datos y binario
sudo enola-cli uninstall --yes --only docker,nginx --keep-data

# Solo el binario
sudo enola-cli uninstall --yes --only binary

# Solo desinstalar deps de terceros que Enola instaló
sudo enola-cli uninstall --yes --only deps --remove-deps
```

## Verificación post-desinstalación

```bash
which enola-cli && echo "⚠️ aún existe" || echo "✅ binario borrado"
docker ps -a --format '{{.Names}}' | grep -E '^(enola-|wp-|db-)' && echo "⚠️" || echo "✅"
sudo ls /etc/nginx/sites-enabled/ | grep proxy_ && echo "⚠️" || echo "✅"
```

## Resolución de problemas

### "Permission denied" al borrar `/srv/enola-*`

```bash
sudo docker ps -a -q | xargs -r sudo docker rm -f
sudo enola-cli uninstall --yes
```

### Nginx sigue devolviendo 502

```bash
sudo nginx -t && sudo systemctl reload nginx
```

### Reglas UFW residuales

El desinstalador borra reglas por número de puerto (del manifiesto) y por comentario `# enola-cli`. Si quedan residuales:
```bash
sudo ufw status numbered
sudo ufw delete N
```

## Reinstalar después

```bash
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash
```

Si usaste `--keep-data`, los servicios **no se recuperan automáticamente**:
debes volver a ejecutar `wp create`, `git create`, etc. Los datos en `/srv` pueden
reutilizarse pasándolos como bind mount.

---

**Releases**: https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases


## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`commands-simple.md`](../general/commands-simple.md#uninstall) | Flags y ejemplos de `uninstall` |
| [`commands.md`](../general/commands.md) | Índice de comandos |
