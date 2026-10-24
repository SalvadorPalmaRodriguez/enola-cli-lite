> **Documento usuario:** `docs/user/general/faq.md`
> **Versión:** 2.2 | **Actualizado:** 2026-10-24
> **Estado:** ✅ **VIGENTE — Preguntas frecuentes**
> **Referencias:** commands.md, concepts.md
> **English:** [`docs/en/faq.md`](../../en/faq.md)

# ❓ Preguntas Frecuentes — Enola CLI

## General

**¿Necesito una IP pública o un dominio?**
No. Enola usa la red Tor para exponer tus servicios. La dirección `.onion`
funciona sin IP pública, sin DNS, sin puertos abiertos en el router.

**¿Qué pasa si apago mi servidor?**
Los servicios dejan de estar accesibles. La dirección `.onion` es permanente
(asociada a claves en `/var/lib/tor/`) — al reiniciar el servidor y los servicios,
la misma dirección vuelve a estar disponible.

**¿Puedo tener múltiples servicios en el mismo servidor?**
Sí, sin límite. Cada servicio tiene su propia dirección `.onion`.

**¿Mi equipo debe suspenderse? (servidor 24/7 vs. equipo de escritorio)**
Mientras el equipo está suspendido, tus servicios `.onion` **no están
accesibles**, igual que si estuviera apagado. Elige uno de estos dos modos:

- **Servidor 24/7** (los servicios siempre disponibles): bloquea la suspensión.
  ```bash
  sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
  ```
  Si además quieres que cerrar la tapa o pulsar el botón de encendido no haga
  nada, crea un drop-in en `/etc/systemd/logind.conf.d/` con
  `HandleLidSwitch=ignore`, `HandlePowerKey=ignore`, `HandleSuspendKey=ignore`
  e `IdleAction=ignore`.

- **Equipo de escritorio** (vuelve a suspenderse como antes): revierte lo anterior.
  ```bash
  sudo systemctl unmask sleep.target suspend.target hibernate.target hybrid-sleep.target
  # Mueve (o borra) el drop-in de logind que creaste y reinicia el equipo
  # (o `sudo systemctl restart systemd-logind`, que cierra la sesión gráfica)
  ```
  Después configura el tiempo de inactividad en tu escritorio. Ejemplo en
  XFCE (3 minutos con corriente y con batería, modo suspender):
  ```bash
  for p in inactivity-on-ac inactivity-on-battery; do
    xfconf-query -c xfce4-power-manager -p /xfce4-power-manager/$p -n -t uint -s 3
  done
  for p in inactivity-sleep-mode-on-ac inactivity-sleep-mode-on-battery; do
    xfconf-query -c xfce4-power-manager -p /xfce4-power-manager/$p -n -t uint -s 1
  done
  ```
  En GNOME: **Configuración → Energía**.

> ⚠️ Si el equipo **nunca** se suspende, y tampoco lo hace con
> `systemctl suspend`, comprueba si los targets están enmascarados:
> `systemctl status suspend.target` (en ese caso muestra `Loaded: masked`).

---

## Tor

**Mi servicio tarda en aparecer (primera vez)**
La red Tor puede tardar 30-90 segundos en propagar una nueva dirección `.onion`.
Espera un minuto y vuelve a intentarlo.

**¿Cómo regenero mi dirección .onion?**
```
sudo enola-cli tor rotate mi-servicio
```
⚠️ La dirección anterior dejará de funcionar permanentemente.

**`curl: (7) Failed to connect`**
Asegúrate de usar el proxy SOCKS5 de Tor:
```
curl --proxy socks5h://127.0.0.1:9050 http://XXXXX.onion/
```
O usa el Navegador Tor.

---

## WordPress

**WordPress muestra error 500 / página de instalación**
Es normal en la primera ejecución. Abre el navegador Tor y accede a la
dirección `.onion` o a `http://localhost:PUERTO/`. Completa el asistente
de instalación de WordPress.

**¿Dónde están los archivos de WordPress?**
En `/srv/enola-wordpress/NOMBRE_wp/` (bind mount de Docker).

**¿Cómo actualizo WordPress?**
```
sudo enola-cli wp update mi-wordpress
```

---


## CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail)

**¿Cuál CMS elegir?**
Depende del caso de uso. Ver la tabla comparativa en `enola-cli docs concepts cms`.
Resumen rápido: blog simple → Ghost; plugins → WordPress; headless → Strapi;
corporativo → Magnolia; contenido estructurado → Wagtail; multilingüe → Drupal.

**¿Por qué Strapi necesita `build-image` antes de `create`?**
Strapi no tiene imagen Docker oficial preconstruida con la configuración de Enola.
El comando `build-image` genera una imagen personalizada con los secrets inyectados.
Sin este paso, `create` no encuentra la imagen.

**¿Dónde están los datos de mi CMS?**
Cada CMS guarda datos en `/srv/enola-{tipo}/{name}/` (bind mount de Docker).
Esto incluye archivos, base de datos y secrets.

**¿Puedo publicar Ghost en Tor?**
Sí. Los subcomandos `ghost publish`, `ghost hide` y `ghost edit` están implementados:
```
sudo enola-cli ghost publish miblog
sudo enola-cli ghost hide miblog
sudo enola-cli ghost edit miblog --http-port 8095
```

---

## VPN

**¿Cuándo usar VPN vs Tor?**
Tor para publicar contenido anónimo (nadie sabe quién eres). VPN para acceso
remoto autenticado entre equipos conocidos. Puedes usar ambos simultáneamente.

**¿Cómo añado un peer a mi VPN?**
```
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
```
Esto genera la configuración del peer con su clave privada/pública.

**¿Mi VPN expone puertos a internet?**
No. WireGuard solo escucha en el puerto configurado (default 51820) y requiere
autenticación con clave criptográfica. Sin un peer autorizado, no hay conexión.

---

## AppArmor

**¿Qué modo AppArmor debo usar?**
`complain` durante las primeras horas para detectar violaciones sin bloquear
servicios. Cambia a `enforce` cuando no haya más violaciones en los logs.

**¿AppArmor afecta el rendimiento?**
El overhead es mínimo (<1% en la mayoría de cargas). El beneficio de seguridad
compensa ampliamente.

---

## Actualizaciones

**¿Cómo sé si hay una actualización?**
```
sudo enola-cli update check
```
Consulta el feed de advisories y muestra si hay una versión nueva o avisos
de seguridad.

**¿Qué significa exit code 11 en `update check`?**
Hay un advisory crítico que afecta a tu versión actual. Actualiza cuanto antes
con `update download --yes`.

**¿Es obligatoria la firma minisign?**
Sí. El feed de advisories debe estar firmado con minisign. Sin firma válida,
el CLI rechaza el feed (exit code 21). Puedes usar `--allow-unsigned` solo en
entornos de desarrollo.

---

## Web Dashboard

**¿Puedo acceder al dashboard desde otro equipo?**
No. El servidor web bindea a `127.0.0.1` exclusivamente. Para acceso remoto,
usa un túnel SSH o VPN.

**¿El token cambia cada vez que arranco el dashboard?**
Sí. Se genera un token aleatorio nuevo en cada inicio. Se muestra en la terminal
donde ejecutaste `enola-cli web`.

---

## PQC (Post-cuántica)

**¿Qué es ML-DSA-65?**
Algoritmo de firma digital post-cuántica estandarizado por NIST (FIPS 204).
Resiste ataques de ordenadores cuánticos que romperían RSA/ECDSA.

**¿SSH sobre Tor es resistente a ordenadores cuánticos?**
SSH estándar no. Usa `maintenance ssh-harden-pqc` para configurar algoritmos
híbridos post-cuánticos (sntrup761x25519) que sí lo son.

---

## IA local (Ollama)

**`ai rag ingest` falla con «Ollama no devolvió embeddings» en vez de indexar**
Es intencionado: si Ollama no devuelve embeddings (instancia parada o sin el
modelo de embeddings), enola-cli **no indexa nada** antes que llenar el índice
con vectores de peor calidad sin avisar. Arranca la instancia
(`sudo enola-cli ai start <NAME>`) y reintenta. En una terminal interactiva se
ofrece reintentar / hash / cancelar; para indexar por hash a propósito usa
`--embedding-fallback hash`. Una colección no mezcla vías: si la creaste por
hash, sigue siendo hash.

**`ask --yes` me sigue pidiendo confirmación (o falla sin terminal)**
`--yes` solo aprueba operaciones **mutantes**. Las **destructivas** (borrar,
`--force`) siempre exigen confirmar `[s/N]` en una terminal interactiva; sin
terminal fallan aunque pases `--yes`. Motivo: el contenido que leen las
herramientas (documentos RAG, logs) podría inducir al modelo a pedir una
acción destructiva. Ver [`commands-ask.md`](../ai/commands-ask.md).

**La tool MCP `docs_lookup` devuelve `results: []`**
La colección de documentación (`enola_docs`) aún no está indexada. Ejecuta
`sudo enola-cli ai rag ingest-docs <NAME>` (y de nuevo tras actualizar
enola-cli) o usa `docs_search`, que no necesita embeddings. Tu índice propio
(`ai rag ingest`/`sync`) nunca se expone a `docs_lookup`.

---

## Configuración

**¿Dónde está el archivo de configuración?**
En `~/.enola/config.toml`. Copia `config.example.toml` como plantilla.
Permisos obligatorios: `chmod 0600 ~/.enola/config.toml`.

**¿Puedo usar una URL .onion para el feed de updates?**
Sí. Configura `[update].feed_url` con tu URL .onion. El CLI la enruta
automáticamente por Tor vía `[http].tor_socks_proxy`.

---

## Puertos y red

**¿Cómo veo qué puertos usa Enola?**
```
sudo enola-cli ports list
```

**Un puerto aparece como ocupado**
Los contenedores Docker parados retienen sus bindings. Verifica con:
```
docker ps -a --format "{{.Names}}: {{.Ports}}"
```

**¿Cómo configuro el firewall?**
```
sudo enola-cli firewall setup    # Configuración inicial (recomendado)
sudo enola-cli firewall status   # Ver estado
```

---

## Errores comunes

| Error | Causa probable | Solución |
|-------|---------------|----------|
| `Permission denied` | No se ejecutó con sudo | `sudo enola-cli ...` |
| `Docker not running` | Docker detenido | `sudo systemctl start docker` |
| `Tor service not running` | Tor detenido | `sudo systemctl start tor` |
| `Nginx config error` | Config rota | `sudo nginx -t` para ver detalles |
| `Port already in use` | Puerto ocupado | `sudo enola-cli ports list` |
---

## Más ayuda

```
sudo enola-cli docs quickstart          # Guía de inicio paso a paso
sudo enola-cli docs commands            # Referencia completa de comandos
sudo enola-cli docs concepts tor        # Conceptos de Tor
sudo enola-cli docs examples deploy     # Ejemplos de despliegue
sudo enola-cli --help                   # Ayuda del CLI
sudo enola-cli COMANDO --help           # Ayuda de un comando específico
```


## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`commands.md`](commands.md) | Índice de comandos |
| [`concepts.md`](concepts.md) | Conceptos clave (Tor, seguridad) |
| [`examples.md`](../guia/examples.md) | Ejemplos de uso prácticos |
