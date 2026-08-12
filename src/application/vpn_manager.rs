use crate::domain::vpn::{validate_public_key, validate_vpn_name, VpnError, VpnServer};
use crate::ports::vpn::{VpnInterfaceStatus, VpnPort};
/// VPN Application Manager (Tarea 162)
///
/// Orchestrates WireGuard VPN operations through VpnPort.
/// Follows hexagonal architecture: depends only on the VpnPort trait.
use std::sync::Arc;

const DEFAULT_VPN_PORT: u16 = 51820;
const DEFAULT_VPN_SUBNET: &str = "10.8.0.0/24";

// ─────────────────────────────────────────────────────────────────────────────
// VpnManager
// ─────────────────────────────────────────────────────────────────────────────

pub struct VpnManager {
    vpn: Arc<dyn VpnPort>,
}

impl VpnManager {
    pub fn new(vpn: Arc<dyn VpnPort>) -> Self {
        Self { vpn }
    }

    // ── Setup ────────────────────────────────────────────────────────────────

    /// Create and start a new WireGuard VPN server.
    ///
    /// Returns the public key of the new server.
    pub fn create_vpn(
        &self,
        interface: &str,
        port: Option<u16>,
        subnet: Option<&str>,
        autostart: bool,
    ) -> Result<String, VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;

        if !self.vpn.is_installed() {
            return Err(VpnError::WireGuardNotInstalled);
        }

        let port = port.unwrap_or(DEFAULT_VPN_PORT);
        let subnet = subnet.unwrap_or(DEFAULT_VPN_SUBNET);

        let private_key = self.vpn.generate_private_key()?;
        let public_key = self.vpn.derive_public_key(&private_key)?;

        let mut server = VpnServer::new(interface, port, subnet);
        server.public_key = public_key.clone();

        self.vpn.create_server_config(&server, &private_key)?;
        self.vpn.start_interface(interface)?;

        if autostart {
            let _ = self.vpn.enable_autostart(interface);
        }

        Ok(public_key)
    }

    /// Start an existing WireGuard interface.
    pub fn start_vpn(&self, interface: &str) -> Result<(), VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        self.vpn.start_interface(interface)
    }

    /// Stop a WireGuard interface.
    pub fn stop_vpn(&self, interface: &str) -> Result<(), VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        self.vpn.stop_interface(interface)
    }

    /// Stop and delete config for a WireGuard interface.
    pub fn delete_vpn(&self, interface: &str) -> Result<(), VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        // Stop first (ignore error if already stopped)
        let _ = self.vpn.stop_interface(interface);
        let _ = self.vpn.disable_autostart(interface);
        self.vpn.delete_config(interface)
    }

    // ── Peer management ──────────────────────────────────────────────────────

    /// Add a new peer to a running VPN and return its client config.
    ///
    /// Generates a new key pair for the peer and returns the .conf content
    /// ready to copy to the client device (or display as QR code).
    #[allow(clippy::too_many_arguments)]
    pub fn add_peer(
        &self,
        interface: &str,
        peer_name: &str,
        server_endpoint: &str,
        server_port: u16,
        server_public_key: &str,
        peer_ip: &str,
        use_preshared_key: bool,
        dns: Option<&str>,
    ) -> Result<String, VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;

        if peer_name.is_empty() {
            return Err(VpnError::InvalidConfig(
                "Peer name cannot be empty".to_string(),
            ));
        }

        // Generate peer key pair
        let peer_private_key = self.vpn.generate_private_key()?;
        let peer_public_key = self.vpn.derive_public_key(&peer_private_key)?;

        let psk = if use_preshared_key {
            Some(self.vpn.generate_preshared_key()?)
        } else {
            None
        };

        // Add peer to running interface
        self.vpn
            .add_peer(interface, &peer_public_key, peer_ip, psk.as_deref())?;

        // Generate client config
        let client_config = self.vpn.generate_peer_config(
            peer_name,
            &peer_private_key,
            peer_ip,
            server_public_key,
            server_endpoint,
            server_port,
            dns,
            psk.as_deref(),
        );

        Ok(client_config)
    }

    /// Add a peer with a known public key (client supplies their own key pair).
    pub fn add_peer_by_pubkey(
        &self,
        interface: &str,
        peer_name: &str,
        peer_public_key: &str,
        peer_ip: &str,
    ) -> Result<(), VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        validate_public_key(peer_public_key).map_err(VpnError::InvalidConfig)?;

        if peer_name.is_empty() {
            return Err(VpnError::InvalidConfig(
                "Peer name cannot be empty".to_string(),
            ));
        }

        self.vpn.add_peer(interface, peer_public_key, peer_ip, None)
    }

    /// Remove a peer by its public key.
    pub fn remove_peer(&self, interface: &str, public_key: &str) -> Result<(), VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        self.vpn.remove_peer(interface, public_key)
    }

    // ── Status ───────────────────────────────────────────────────────────────

    /// Get VPN interface status.
    pub fn get_status(&self, interface: &str) -> Result<VpnInterfaceStatus, VpnError> {
        validate_vpn_name(interface).map_err(VpnError::InvalidConfig)?;
        self.vpn.get_interface_status(interface)
    }

    /// List all WireGuard interfaces.
    pub fn list_vpns(&self) -> Result<Vec<String>, VpnError> {
        if !self.vpn.is_installed() {
            return Ok(vec![]);
        }
        self.vpn.list_interfaces()
    }

    /// Format a status report string.
    pub fn format_status(&self, status: &VpnInterfaceStatus) -> String {
        let mut out = format!(
            "🔒 WireGuard VPN: {}\n\
             ─────────────────────────────────────\n\
             Public Key:   {}\n\
             Listen Port:  {}\n\
             Peers:        {}\n",
            status.interface,
            status.public_key,
            status.listen_port,
            status.peers.len(),
        );

        if !status.peers.is_empty() {
            out.push_str("─────────────────────────────────────\n");
            for peer in &status.peers {
                let ips = peer.allowed_ips.join(", ");
                let endpoint = peer.endpoint.as_deref().unwrap_or("(not connected)");
                let handshake = peer.latest_handshake.as_deref().unwrap_or("never");
                let rx = format_bytes(peer.transfer_rx_bytes);
                let tx = format_bytes(peer.transfer_tx_bytes);
                out.push_str(&format!(
                    "  Peer: {} → {} | {} | ↓{} ↑{}\n",
                    ips, endpoint, handshake, rx, tx
                ));
            }
        }
        out
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::vpn::VpnPeerStatus;

    // ── Mock adapter ─────────────────────────────────────────────────────────

    struct MockVpn {
        installed: bool,
        fail_start: bool,
    }

    impl VpnPort for MockVpn {
        fn is_installed(&self) -> bool {
            self.installed
        }
        fn generate_private_key(&self) -> Result<String, VpnError> {
            Ok("MOCK_PRIVATE_KEY_BASE64_44CHARS_PADDING====".to_string())
        }
        fn derive_public_key(&self, _pk: &str) -> Result<String, VpnError> {
            Ok("xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=".to_string())
        }
        fn generate_preshared_key(&self) -> Result<String, VpnError> {
            Ok("MOCK_PSK_44_CHARS_BASE64_PADDING_MOCK_PSK===".to_string())
        }
        fn create_server_config(&self, _s: &VpnServer, _pk: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn start_interface(&self, interface: &str) -> Result<(), VpnError> {
            if self.fail_start {
                Err(VpnError::SystemError(format!(
                    "wg-quick up {} failed (mock)",
                    interface
                )))
            } else {
                Ok(())
            }
        }
        fn stop_interface(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn get_interface_status(&self, interface: &str) -> Result<VpnInterfaceStatus, VpnError> {
            Ok(VpnInterfaceStatus {
                interface: interface.to_string(),
                public_key: "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=".to_string(),
                listen_port: 51820,
                peers: vec![VpnPeerStatus {
                    public_key: "PEER_PUB_KEY_44_CHARS_BASE64_PADDING====".to_string(),
                    allowed_ips: vec!["10.8.0.2/32".to_string()],
                    endpoint: Some("1.2.3.4:51820".to_string()),
                    latest_handshake: Some("2 minutes ago".to_string()),
                    transfer_rx_bytes: 1024,
                    transfer_tx_bytes: 2048,
                }],
            })
        }
        fn list_interfaces(&self) -> Result<Vec<String>, VpnError> {
            Ok(vec!["wg0".to_string(), "enola-vpn".to_string()])
        }
        fn add_peer(
            &self,
            _i: &str,
            _pk: &str,
            _ip: &str,
            _psk: Option<&str>,
        ) -> Result<(), VpnError> {
            Ok(())
        }
        fn remove_peer(&self, _i: &str, _pk: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn generate_peer_config(
            &self,
            peer_name: &str,
            _ppk: &str,
            peer_ip: &str,
            _spk: &str,
            endpoint: &str,
            port: u16,
            dns: Option<&str>,
            _psk: Option<&str>,
        ) -> String {
            format!(
                "[Interface]\nAddress = {}/32\n\n[Peer]\nEndpoint = {}:{}\n# peer: {}\nDNS = {}\n",
                peer_ip,
                endpoint,
                port,
                peer_name,
                dns.unwrap_or("")
            )
        }
        fn enable_autostart(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn disable_autostart(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn delete_config(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
    }

    fn make_manager(installed: bool) -> VpnManager {
        VpnManager::new(Arc::new(MockVpn {
            installed,
            fail_start: false,
        }))
    }

    fn make_manager_fail_start() -> VpnManager {
        VpnManager::new(Arc::new(MockVpn {
            installed: true,
            fail_start: true,
        }))
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_create_vpn_not_installed() {
        let mgr = make_manager(false);
        let result = mgr.create_vpn("wg0", None, None, false);
        assert!(matches!(result, Err(VpnError::WireGuardNotInstalled)));
    }

    #[test]
    fn test_create_vpn_invalid_name() {
        let mgr = make_manager(true);
        let result = mgr.create_vpn("this-is-too-long-name", None, None, false);
        assert!(matches!(result, Err(VpnError::InvalidConfig(_))));
    }

    #[test]
    fn test_create_vpn_ok() {
        let mgr = make_manager(true);
        let pub_key = mgr
            .create_vpn("wg0", Some(51820), Some("10.8.0.0/24"), false)
            .unwrap();
        assert_eq!(pub_key.len(), 44);
    }

    #[test]
    fn test_create_vpn_start_failure() {
        let mgr = make_manager_fail_start();
        let result = mgr.create_vpn("wg0", None, None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_vpn_invalid_name() {
        let mgr = make_manager(true);
        assert!(mgr.stop_vpn("bad name!").is_err());
    }

    #[test]
    fn test_stop_vpn_ok() {
        let mgr = make_manager(true);
        assert!(mgr.stop_vpn("wg0").is_ok());
    }

    #[test]
    fn test_delete_vpn_ok() {
        let mgr = make_manager(true);
        assert!(mgr.delete_vpn("wg0").is_ok());
    }

    #[test]
    fn test_add_peer_empty_name() {
        let mgr = make_manager(true);
        let result = mgr.add_peer(
            "wg0",
            "",
            "vpn.example.com",
            51820,
            "PUB",
            "10.8.0.2",
            false,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_add_peer_ok_generates_config() {
        let mgr = make_manager(true);
        let config = mgr
            .add_peer(
                "wg0",
                "laptop",
                "vpn.example.com",
                51820,
                "SERVER_PUB_KEY",
                "10.8.0.2",
                false,
                Some("1.1.1.1"),
            )
            .unwrap();
        assert!(config.contains("[Interface]"));
        assert!(config.contains("10.8.0.2"));
        assert!(config.contains("vpn.example.com"));
    }

    #[test]
    fn test_add_peer_with_psk() {
        let mgr = make_manager(true);
        let config = mgr
            .add_peer(
                "wg0", "phone", "1.2.3.4", 51820, "PUB_KEY", "10.8.0.3", true, None,
            )
            .unwrap();
        assert!(config.contains("[Peer]"));
    }

    #[test]
    fn test_add_peer_by_pubkey_invalid() {
        let mgr = make_manager(true);
        let result = mgr.add_peer_by_pubkey("wg0", "laptop", "tooshort", "10.8.0.2");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_peer_by_pubkey_ok() {
        let mgr = make_manager(true);
        let valid_key = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";
        assert!(mgr
            .add_peer_by_pubkey("wg0", "laptop", valid_key, "10.8.0.2")
            .is_ok());
    }

    #[test]
    fn test_remove_peer_ok() {
        let mgr = make_manager(true);
        assert!(mgr.remove_peer("wg0", "PUB_KEY").is_ok());
    }

    #[test]
    fn test_get_status() {
        let mgr = make_manager(true);
        let status = mgr.get_status("wg0").unwrap();
        assert_eq!(status.interface, "wg0");
        assert_eq!(status.peers.len(), 1);
    }

    #[test]
    fn test_list_vpns_not_installed() {
        let mgr = make_manager(false);
        let list = mgr.list_vpns().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_vpns_ok() {
        let mgr = make_manager(true);
        let list = mgr.list_vpns().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"wg0".to_string()));
    }

    #[test]
    fn test_format_status_with_peer() {
        let mgr = make_manager(true);
        let status = mgr.get_status("wg0").unwrap();
        let formatted = mgr.format_status(&status);
        assert!(formatted.contains("WireGuard VPN: wg0"));
        assert!(formatted.contains("Peers:        1"));
        assert!(formatted.contains("10.8.0.2/32"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1.0KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0GiB");
    }
}
