> **Documento usuario:** `docs/user/guia/examples.md`
> **Versión:** 3.0 | **Actualizado:** 2026-08-08
> **Estado:** ✅ **VIGENTE — Ejemplos de uso**

# 🛠 Ejemplos de Uso — Enola CLI

## Caso 1: Blog anónimo con WordPress

```bash
# 1. Crear WordPress
sudo enola-cli wp create --name mi-blog --http-port 8080

# 2. Publicar en Tor
sudo enola-cli wp publish mi-blog

# 3. Ver la dirección .onion
sudo enola-cli wp status mi-blog
# → 🧅 http://abc123.onion/
```

> Para habilitar HTTPS con certificado self-signed, usa `wp edit mi-blog --ssl`
> después de publicar.

---

## Caso 2: Servidor Git anónimo

```bash
# 1. Crear servidor Git (Forgejo)
sudo enola-cli git create --name mi-repo --http-port 10000

# 2. Ver la dirección .onion
sudo enola-cli git list
# → 🧅 http://xxxxx.onion/ — repositorios Git

# 3. Crear usuario admin
sudo enola-cli git user create mi-repo --username admin --email admin@example.onion --password MiPass123
```

---

## Caso 3: Servidor de archivos compartidos

```bash
# 1. Crear servidor de archivos
sudo enola-cli files create --name mis-archivos

# 2. Ver la dirección .onion
sudo enola-cli files list
# → http://xxxxx.onion/ — directorio de archivos

# 3. Añadir archivos (se sirven desde /srv/enola-files/mis-archivos/)
cp documento.pdf /srv/enola-files/mis-archivos/
```

---

## Caso 4: Configurar el firewall

```bash
# Configuración inicial (recomendado al instalar Enola)
sudo enola-cli firewall setup

# Ver estado
sudo enola-cli firewall status

# Permitir SSH (si necesitas acceso remoto al servidor)
sudo enola-cli firewall allow --port 22

# Ver todos los puertos que usa Enola
sudo enola-cli ports list
```

---

## Caso 5: Deploy de un CMS (Drupal)

```bash
# 1. Crear Drupal
sudo enola-cli drupal create --name mi-sitio --http-port 8090

# 2. Publicar en Tor
sudo enola-cli drupal publish mi-sitio

# 3. Ver la dirección .onion
sudo enola-cli drupal status mi-sitio
# → 🧅 http://xxxxx.onion/

# 4. Completar el wizard de instalación
#    Abre http://127.0.0.1:8090/ en tu navegador y sigue los pasos.
```

---

## Caso 6: Configurar VPN para acceso remoto

```bash
# 1. Crear interfaz WireGuard
sudo enola-cli vpn create wg0 --port 51820

# 2. Añadir un peer (ej. laptop remoto)
sudo enola-cli vpn peer add wg0 laptop --endpoint myhostname.com
# → Se genera la configuración del peer con claves.

# 3. Iniciar la VPN
sudo enola-cli vpn start wg0

# 4. Ver estado
sudo enola-cli vpn status wg0
```

---

## Caso 7: Hardening con AppArmor + firewall

```bash
# 1. Configurar firewall (UFW + Docker-USER chain)
sudo enola-cli firewall setup

# 2. Configurar AppArmor
sudo enola-cli apparmor setup

# 3. Modo complain inicial (detectar violaciones sin bloquear)
sudo enola-cli apparmor mode --complain

# 4. Tras verificar que no hay violaciones, cambiar a enforce
sudo enola-cli apparmor mode --enforce

# 5. Verificar estado de ambos
sudo enola-cli firewall status
sudo enola-cli apparmor status
```

---

## Caso 8: Actualizar el binario

```bash
# 1. Comprobar si hay actualizaciones
sudo enola-cli update check

# 2. Descargar y aplicar la actualización
sudo enola-cli update download --yes

# 3. Verificar la versión instalada
enola-cli --version
```

> La firma minisign se verifica automáticamente. Si el feed usa una URL .onion,
> el CLI la enruta por Tor automáticamente.

---

## Caso 9: Usar el web dashboard

```bash
# 1. Iniciar el dashboard
sudo enola-cli web --port 8090
# → Token: abc123def456... (se muestra en la terminal)

# 2. Abrir en el navegador
#    http://127.0.0.1:8090
#    Introduce el token mostrado en la terminal.

# 3. Gestionar servicios desde la interfaz web
#    - Ver todos los servicios en una tabla unificada
#    - Publicar/ocultar en Tor
#    - Ver logs, puertos, diagnósticos

# 4. Detener con Ctrl+C en la terminal
```

> El dashboard solo es accesible desde localhost (127.0.0.1). No expone puertos
> a internet.

---

## Deploy

Despliegue completo de un servidor Git anónimo desde cero:

```bash
# 1. Instalar dependencias
sudo enola-cli setup --all

# 2. Verificar que todo está listo
sudo enola-cli doctor

# 3. Crear servidor Git
sudo enola-cli git create --name mi-repo --http-port 10000

# 4. Publicar en Tor
sudo enola-cli git publish mi-repo

# 5. Verificar la dirección .onion
sudo enola-cli git list
# → 🧅 http://xxxxx.onion/ — repositorios Git

# 6. Crear usuario admin
sudo enola-cli git user create mi-repo --username admin --email admin@example.onion --password MiPass123
```

---

## Backup

Respaldo y restauración manual de servicios:

```bash
# 1. Crear backup del sistema
sudo enola-cli maintenance backup

# 2. Los datos de cada servicio están en /srv/enola-{tipo}/{name}/
#    Para respaldo manual:
sudo tar czf /backup/enola-wordpress-mi-blog.tar.gz /srv/enola-wordpress/mi-blog/

# 3. Para restaurar: detener, restaurar archivos, reiniciar
sudo enola-cli wp stop mi-blog
sudo tar xzf /backup/enola-wordpress-mi-blog.tar.gz -C /
sudo enola-cli wp start mi-blog
```

---


## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`commands.md`](../general/commands.md) | Índice de comandos |
| [`quickstart.md`](quickstart.md) | Guía de inicio rápido |
