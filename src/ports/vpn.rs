/// VPN Port — injectable trait for WireGuard operations (Tarea 162)
///
/// All methods are synchronous to support mockall easily.
/// The adapter uses std::process::Command internally (not tokio).
use crate::domain::vpn::{VpnError, VpnServer};

// ─────────────────────────────────────────────────────────────────────────────
// VpnPort trait
// ─────────────────────────────────────────────────────────────────────────────

/// Abstraction over WireGuard system operations.
///
/// Implemented by `WireGuardAdapter` in production.
/// Can be mocked with `mockall` in tests.
pub trait VpnPort: Send + Sync {
    /// Check if wg and wg-quick are installed and accessible.
    fn is_installed(&self) -> bool;

    /// Generate a new WireGuard private key.
    /// Returns the base64 private key.
    fn generate_private_key(&self) -> Result<String, VpnError>;

    /// Derive the public key from a private key.
    fn derive_public_key(&self, private_key: &str) -> Result<String, VpnError>;

    /// Generate a preshared key for extra security.
    fn generate_preshared_key(&self) -> Result<String, VpnError>;

    /// Create the WireGuard server configuration file.
    ///
    /// Writes to `/etc/wireguard/{interface}.conf`.
    fn create_server_config(&self, server: &VpnServer, private_key: &str) -> Result<(), VpnError>;

    /// Bring up the WireGuard interface (`wg-quick up {interface}`).
    fn start_interface(&self, interface: &str) -> Result<(), VpnError>;

    /// Bring down the WireGuard interface (`wg-quick down {interface}`).
    fn stop_interface(&self, interface: &str) -> Result<(), VpnError>;

    /// Get current interface status (`wg show {interface}`).
    fn get_interface_status(&self, interface: &str) -> Result<VpnInterfaceStatus, VpnError>;

    /// List all WireGuard interfaces on the system.
    fn list_interfaces(&self) -> Result<Vec<String>, VpnError>;

    /// Add a peer to a running WireGuard interface (`wg set`).
    fn add_peer(
        &self,
        interface: &str,
        public_key: &str,
        allowed_ip: &str,
        preshared_key: Option<&str>,
    ) -> Result<(), VpnError>;

    /// Remove a peer from a running WireGuard interface.
    fn remove_peer(&self, interface: &str, public_key: &str) -> Result<(), VpnError>;

    /// Generate a QR code config for a client peer (for mobile apps).
    #[allow(clippy::too_many_arguments)]
    fn generate_peer_config(
        &self,
        peer_name: &str,
        peer_private_key: &str,
        peer_ip: &str,
        server_public_key: &str,
        server_endpoint: &str,
        server_port: u16,
        dns: Option<&str>,
        preshared_key: Option<&str>,
    ) -> String;

    /// Enable the WireGuard interface to start on boot (`systemctl enable wg-quick@{interface}`).
    fn enable_autostart(&self, interface: &str) -> Result<(), VpnError>;

    /// Disable autostart.
    fn disable_autostart(&self, interface: &str) -> Result<(), VpnError>;

    /// Delete the configuration file for an interface.
    fn delete_config(&self, interface: &str) -> Result<(), VpnError>;
}

// ─────────────────────────────────────────────────────────────────────────────
// VpnInterfaceStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Runtime status of a WireGuard interface from `wg show`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VpnInterfaceStatus {
    pub interface: String,
    pub public_key: String,
    pub listen_port: u16,
    pub peers: Vec<VpnPeerStatus>,
}

/// Runtime status of a single peer from `wg show`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VpnPeerStatus {
    pub public_key: String,
    pub allowed_ips: Vec<String>,
    pub endpoint: Option<String>,
    pub latest_handshake: Option<String>,
    pub transfer_rx_bytes: u64,
    pub transfer_tx_bytes: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vpn::VpnServer;

    /// Minimal mock for unit tests (no mockall dependency needed)
    struct MockVpn {
        installed: bool,
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
            Ok("MOCK_PSK_BASE64_44CHARS_PADDING_MOCK_PSK====".to_string())
        }
        fn create_server_config(&self, _s: &VpnServer, _pk: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn start_interface(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn stop_interface(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn get_interface_status(&self, interface: &str) -> Result<VpnInterfaceStatus, VpnError> {
            Ok(VpnInterfaceStatus {
                interface: interface.to_string(),
                public_key: "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=".to_string(),
                listen_port: 51820,
                peers: vec![],
            })
        }
        fn list_interfaces(&self) -> Result<Vec<String>, VpnError> {
            Ok(vec!["wg0".to_string()])
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
            _ep: &str,
            _port: u16,
            _dns: Option<&str>,
            _psk: Option<&str>,
        ) -> String {
            format!("[Interface]\nAddress = {}\n# Peer: {}", peer_ip, peer_name)
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

    #[test]
    fn test_mock_vpn_is_installed() {
        let mock = MockVpn { installed: true };
        assert!(mock.is_installed());
        let mock2 = MockVpn { installed: false };
        assert!(!mock2.is_installed());
    }

    #[test]
    fn test_mock_generate_keys() {
        let mock = MockVpn { installed: true };
        let pk = mock.generate_private_key().unwrap();
        assert!(!pk.is_empty());
        let pub_key = mock.derive_public_key(&pk).unwrap();
        assert_eq!(pub_key.len(), 44);
    }

    #[test]
    fn test_mock_generate_peer_config_contains_ip() {
        let mock = MockVpn { installed: true };
        let config = mock.generate_peer_config(
            "laptop",
            "PRIV",
            "10.8.0.2",
            "PUB",
            "vpn.example.com",
            51820,
            None,
            None,
        );
        assert!(config.contains("10.8.0.2"));
        assert!(config.contains("laptop"));
    }

    #[test]
    fn test_mock_interface_status() {
        let mock = MockVpn { installed: true };
        let status = mock.get_interface_status("wg0").unwrap();
        assert_eq!(status.interface, "wg0");
        assert_eq!(status.listen_port, 51820);
        assert!(status.peers.is_empty());
    }

    #[test]
    fn test_mock_list_interfaces() {
        let mock = MockVpn { installed: true };
        let interfaces = mock.list_interfaces().unwrap();
        assert_eq!(interfaces, vec!["wg0"]);
    }

    #[test]
    fn test_vpn_interface_status_fields() {
        let status = VpnInterfaceStatus {
            interface: "wg0".to_string(),
            public_key: "key".to_string(),
            listen_port: 51820,
            peers: vec![VpnPeerStatus {
                public_key: "peer_key".to_string(),
                allowed_ips: vec!["10.8.0.2/32".to_string()],
                endpoint: Some("1.2.3.4:51820".to_string()),
                latest_handshake: Some("1 minute ago".to_string()),
                transfer_rx_bytes: 1024,
                transfer_tx_bytes: 2048,
            }],
        };
        assert_eq!(status.peers.len(), 1);
        assert_eq!(status.peers[0].transfer_rx_bytes, 1024);
    }
}
