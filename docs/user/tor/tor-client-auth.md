# Tor Client Authorization — Guía para usuarios

## ¿Qué es?

Client Authorization es el sistema de control de acceso de Tor v3 hidden services.
Funciona igual que las claves SSH de GitHub/GitLab: **tú generas tu par de claves
en tu equipo, la privada nunca sale de tu ordenador, y solo envías la pública
al operador del servicio**.

## Flujo paso a paso

### 1. Generar tu par de claves (CLIENTE)

```bash
enola-cli tor auth generate --client mi-nombre
```

Esto genera dos claves X25519:

```
🔐 Generated keypair for client 'mi-nombre'

📤 PUBLIC KEY (send this to the service operator):
ABCD1234EFGH5678IJKL9012MNOP3456...

📥 PRIVATE KEY (import in Tor Browser → Onion Services → Client auth):
WXYZ9876UVWT5432RSTQ2109PONO8765...
```

- **Guarda la privada** — impórtala en tu Tor Browser
- **Envía la pública** al operador del servicio (por Signal, PGP, email seguro, etc.)
- **NUNCA compartas la privada** con nadie

### 2. Importar la clave privada en Tor Browser

1. Abre Tor Browser
2. Ve a **Preferencias** → **Onion Services** → **Client Authorization**
3. Click **Add Client Authorization**
4. Introduce la dirección `.onion` que te dio el operador
5. Pega tu **clave privada**
6. Guarda

### 3. El operador añade tu clave pública

El operador ejecuta en su servidor:

```bash
sudo enola-cli tor auth add mi-servicio --client mi-nombre --pubkey ABCD1234...
```

Una vez añadido, ya puedes acceder al `.onion` desde tu Tor Browser.

## Rotación de claves

Por seguridad, las claves deben rotarse cada 90 días (recomendado).
El operador puede rotar tus claves:

```bash
sudo enola-cli tor auth rotate mi-servicio --client mi-nombre
```

Esto genera un nuevo par de claves. El operador te enviará la nueva
clave privada por canal seguro. Debes importarla en tu Tor Browser
reemplazando la anterior.

## Comparación con GitHub/GitLab

| Aspecto | GitHub/GitLab SSH | Enola Tor Auth |
|---------|-------------------|----------------|
| Quién genera las claves | El cliente | El cliente |
| Clave privada | Nunca sale del equipo | Nunca sale del equipo |
| Clave pública | Se sube al servidor | Se envía al operador |
| Algoritmo | Ed25519/RSA | X25519 (Curve25519) |
| Rotación | Manual | `tor auth rotate` |
| Post-cuántico | No (todavía) | No |

## Seguridad

- **X25519 NO es resistente a ataques cuánticos** (algoritmo de Shor)
- La rotación periódica mitiga el ataque "Harvest Now, Decrypt Later" (HNDL)
- El Tor Project está trabajando en autenticación post-cuántica (ML-KEM)
- Hasta entonces, rota cada 90 días

## Preguntas frecuentes

**¿Puedo generar las claves en otro equipo y copiarlas?**
Sí, pero no es recomendable. Lo más seguro es generarlas en el equipo
donde las vas a usar.

**¿Qué pasa si pierdo mi clave privada?**
Contacta al operador del servicio. Tendrá que revocar tu clave antigua
y añadir una nueva que generes.

**¿Puede el operador ver mi clave privada?**
No. Si tú generas las claves con `tor auth generate`, la privada solo
se muestra una vez en tu terminal. El operador nunca la ve.

**¿Puedo usar la misma clave para varios servicios?**
Técnicamente sí, pero no es recomendable. Usa claves separadas para
cada servicio por seguridad.
