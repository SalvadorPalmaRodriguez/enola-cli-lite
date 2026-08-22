> **Documento usuario:** `docs/user/guia/quickstart.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de inicio rápido**
> **Referencias:** commands.md, concepts.md, examples.md, faq.md
> **English:** [`docs/en/quickstart.md`](../../en/quickstart.md)

# 🚀 Guía de Inicio Rápido — Enola CLI

Enola CLI te permite desplegar servicios web, servidores Git, CMS
y stacks completos, todo accesible de forma anónima a través de la red Tor.
Sin servidores en la nube, todo en tu máquina.

> **Importante**: esta guía asume que ya has instalado el binario `enola-cli`.

---

## Paso 1: Verificar la configuración

Muestra la configuración actual y valídala:

```bash
enola-cli config-show
enola-cli config-validate
```

---

## Paso 2: Tu primer file share en Tor

Crea un servidor de archivos estático accesible por dirección .onion:

```bash
sudo enola-cli files create --name mi-web
```

Verás algo como:
```
✅ Servicio creado: mi-web
🧅 Dirección .onion: abc123...xyz.onion
```

Copia el contenido de tu web en `/srv/enola-files/mi-web/`.

---

## Paso 3: Tu primer servidor Git

```bash
sudo enola-cli git create --name mi-repo --http-port 10000
```

Accede desde el navegador Tor a la dirección .onion que aparece.

---

## Paso 4: Explorar más

```bash
sudo enola-cli diag summary          # Ver estado general del sistema
sudo enola-cli ports list            # Ver puertos en uso
```


## Comandos de ayuda

```
sudo enola-cli docs commands          # Referencia de todos los comandos
sudo enola-cli docs concepts tor      # Entender cómo funciona Tor en Enola
sudo enola-cli docs examples deploy   # Ejemplos de despliegue
sudo enola-cli docs faq               # Preguntas frecuentes
sudo enola-cli docs search <término>  # Buscar en la documentación
```

---

## Conceptos clave

- **Tor**: Cada servicio tiene una dirección `.onion` única. El tráfico va
  cifrado por la red Tor. No necesitas un dominio ni IP pública.
- **Nginx**: Actúa como proxy inverso entre Tor y tu aplicación.
- **Cadena de puertos**: `.onion` → Nginx (127.0.0.1) → tu app (127.0.0.1)
  Los puertos internos nunca son accesibles desde fuera.
- **Actualizaciones**: El CLI comprueba actualizaciones automáticamente
  y verifica su propio hash al arrancar.


## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`commands.md`](../general/commands.md) | Índice de comandos |
| [`concepts.md`](../general/concepts.md) | Conceptos clave de Enola CLI |
| [`examples.md`](examples.md) | Ejemplos de despliegue |
| [`faq.md`](../general/faq.md) | Preguntas frecuentes |
