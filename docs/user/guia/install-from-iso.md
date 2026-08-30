> **Documento usuario:** `docs/user/guia/install-from-iso.md`
> **Versión:** 2.0 | **Actualizado:** 2026-07-31
> **Estado:** ✅ **VIGENTE — Guía de instalación desde ISO**
> **Referencias:** docs/user/guia/quickstart.md

# Instalar Enola CLI desde ISO o imagen VM
> ISO-004 / ISO-005 / ISO-017 — guía para usuarios que prefieren una máquina
> ya preparada con Docker, Tor, Nginx, UFW, AppArmor y `enola-cli` instalados.
> Cero pasos manuales: arrancas la VM y todo está listo.

## Flavor disponible

| Flavor | Filename | Para quién | Tamaño aprox |
|--------|----------|-----------|--------------|
| **Cliente** | `enola-cli-{ver}-{arch}-client.iso` | Usuario final que despliega sus servicios (Tor, Git, WordPress, Drupal, Ghost, archivos). | ~1.2 GB |

## Qué artefacto elegir
| Artefacto | Uso recomendado | Herramienta |
|-----------|-----------------|-------------|
| `.iso` | Instalación física en portátil, portátil o servidor bare-metal | Rufus, balenaEtcher, `dd` |
| `.qcow2` | KVM, QEMU, Proxmox | `qm importdisk`, `virt-manager`, `qemu-system-x86_64` |
| `.ova` | VirtualBox / VMware | Importar appliance |
## Verificar antes de instalar
```bash
sha256sum -c enola-cli-*.sha256
minisign -Vm enola-cli-*.iso -p minisign.pub
# Si se publica .pqsig: verificar también ML-DSA-65 según verify-downloads.md
```
## Grabar ISO a USB
### Rufus / balenaEtcher
1. Selecciona la ISO `enola-cli-<version>-amd64.iso`.
2. Elige el USB destino.
3. Modo recomendado: escritura directa / DD.
4. Arranca el portátil desde USB e instala.
### Linux (`dd`)
```bash
lsblk
sudo dd if=enola-cli-0.2.0-alpha-amd64.iso of=/dev/sdX bs=4M status=progress oflag=sync
sync
```
> Sustituye `/dev/sdX` por el dispositivo USB completo, no por una partición.
## Primer arranque
La imagen deja un marcador en `/etc/enola-image-info`:
```bash
cat /etc/enola-image-info
enola-cli --version
sudo systemctl status docker tor nginx
```
Después verifica tu sistema:
```bash
sudo enola-cli doctor
```
## Importar QCOW2 en Proxmox/KVM
```bash
qm create 9000 --name enola-cli --memory 4096 --cores 2 --net0 virtio,bridge=vmbr0
qm importdisk 9000 enola-cli-0.2.0-alpha.qcow2 local-lvm
qm set 9000 --scsihw virtio-scsi-pci --scsi0 local-lvm:vm-9000-disk-0
qm set 9000 --boot c --bootdisk scsi0
qm start 9000
```
## Importar OVA
- VirtualBox: `Archivo → Importar servicio virtualizado`.
- VMware: `File → Open` y selecciona `.ova`.
## ¿Y si no quiero usar ISO? (instalación binaria + deps automáticas)

```bash
# Una sola línea: descarga, verifica firma, instala binario y dependencias
curl -fsSL https://github.com/SalvadorPalmaRodriguez/enola-cli-lite/releases/latest/download/install.sh | sudo bash
```

Esto ejecuta el mismo `postinstall_deps.sh --flavor client` que usa la ISO,
así que tu sistema queda igual de preparado (Docker + Tor + Nginx + UFW +
AppArmor + `~/.enola` 0700) — sin instalar nada a mano.

## Referencias Cruzadas

| Documento | Propósito |
|-----------|-----------|
| [`quickstart.md`](quickstart.md) | Guía de inicio rápido tras instalar |
| [`commands.md`](../general/commands.md) | Índice de comandos |
