> **Documento usuario:** `docs/user/general/faq.md`
> **Versión:** 2.0 | **Actualizado:** 2026-08-08
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
Los subcomandos `ghost publish/hide/edit` son stubs pendientes de implementación.
Mientras tanto, usa el comando equivalente de Tor:
```
sudo enola-cli tor create --name ghost-miblog --target-port 8095
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
