pub mod apparmor;
pub mod docker;
pub mod document_extractor;
pub mod manifest; // UNINSTALL-MANIFEST-001 — file-backed ManifestPort // AA-003 (203) — AppArmor adapter (apparmor_parser, aa-status)

pub mod filesystem;
pub mod logging;
pub mod nginx;
pub mod port_checker; // PORTS-003 (177) — OS+Docker port availability check
pub mod security;
pub mod systemd;
pub mod ufw; // UFW firewall adapter (UFW-003)

pub mod dependencies;
pub mod vpn; // Tarea 162 — WireGuard VPN adapter (wg/wg-quick) // DEP-001..003 — System dependency adapter (apt/dnf/pacman)
pub mod vpn_bridge; // VPN over Tor — socat UDP→TCP bridge adapter (systemd unit)
