/// VPN over Tor application manager
///
/// Orchestrates exposing a WireGuard VPN through a Tor hidden service using a
/// socat UDP→TCP bridge. Follows hexagonal architecture: depends only on ports.
///
/// This use case is async because `TorManagerPort` is async, while `VpnPort`
/// and `VpnBridgePort` remain synchronous.
use crate::domain::vpn::{bridge_tcp_port, VpnError};
use crate::ports::tor::TorManagerPort;
use crate::ports::vpn::VpnPort;
use crate::ports::vpn_bridge::VpnBridgePort;
use std::sync::Arc;

pub struct VpnTorManager {
    vpn: Arc<dyn VpnPort>,
    bridge: Arc<dyn VpnBridgePort>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
}

impl VpnTorManager {
    pub fn new(
        vpn: Arc<dyn VpnPort>,
        bridge: Arc<dyn VpnBridgePort>,
        tor: Arc<dyn TorManagerPort + Send + Sync>,
    ) -> Self {
        Self { vpn, bridge, tor }
    }

    /// Tor hidden service name for a VPN interface (e.g. "vpn-wg0").
    fn tor_service_name(interface: &str) -> String {
        format!("vpn-{}", interface)
    }

    /// Expose a running VPN through Tor.
    ///
    /// Starts the socat bridge and deploys the hidden service. Returns the
    /// generated `.onion` address.
    pub async fn enable_tor(&self, interface: &str) -> Result<String, VpnError> {
        let status = self.vpn.get_interface_status(interface)?;
        let wg_port = status.listen_port;
        let tcp_port = bridge_tcp_port(wg_port);

        self.bridge.start_bridge(interface, tcp_port, wg_port)?;

        let onion = self
            .tor
            .deploy_hidden_service(
                &Self::tor_service_name(interface),
                vec![(wg_port, tcp_port)],
            )
            .await
            .map_err(|e| VpnError::SystemError(e.to_string()))?;

        Ok(onion)
    }

    /// Remove the Tor exposure for a VPN interface (hidden service + bridge).
    pub async fn disable_tor(&self, interface: &str) -> Result<(), VpnError> {
        // Ignore NotFound if the hidden service was already removed.
        let _ = self
            .tor
            .remove_hidden_service(&Self::tor_service_name(interface))
            .await;
        self.bridge.stop_bridge(interface)?;
        Ok(())
    }

    /// Get the `.onion` address for a VPN interface.
    pub async fn get_onion(&self, interface: &str) -> Result<String, VpnError> {
        self.tor
            .get_onion_address(&Self::tor_service_name(interface))
            .await
            .map_err(|e| VpnError::SystemError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::tor::MockTorManagerPort;
    use crate::ports::vpn::VpnInterfaceStatus;

    struct MockVpn;
    impl VpnPort for MockVpn {
        fn is_installed(&self) -> bool {
            true
        }
        fn generate_private_key(&self) -> Result<String, VpnError> {
            Ok("PRIV".into())
        }
        fn derive_public_key(&self, _pk: &str) -> Result<String, VpnError> {
            Ok("PUB".into())
        }
        fn generate_preshared_key(&self) -> Result<String, VpnError> {
            Ok("PSK".into())
        }
        fn create_server_config(
            &self,
            _s: &crate::domain::vpn::VpnServer,
            _pk: &str,
        ) -> Result<(), VpnError> {
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
                public_key: "PUB".into(),
                listen_port: 51820,
                peers: vec![],
            })
        }
        fn list_interfaces(&self) -> Result<Vec<String>, VpnError> {
            Ok(vec!["wg0".into()])
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
            _peer_name: &str,
            _ppk: &str,
            peer_ip: &str,
            _spk: &str,
            _ep: &str,
            _port: u16,
            _dns: Option<&str>,
            _psk: Option<&str>,
        ) -> String {
            format!("[Interface]\nAddress = {}/32\n", peer_ip)
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

    struct MockBridge {
        fail_start: bool,
    }
    impl VpnBridgePort for MockBridge {
        fn start_bridge(&self, _i: &str, _t: u16, _u: u16) -> Result<(), VpnError> {
            if self.fail_start {
                Err(VpnError::SystemError("bridge start failed".into()))
            } else {
                Ok(())
            }
        }
        fn stop_bridge(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn is_bridge_active(&self, _i: &str) -> bool {
            true
        }
    }

    fn make_manager(fail_bridge: bool) -> VpnTorManager {
        let mut tor = MockTorManagerPort::new();
        tor.expect_deploy_hidden_service()
            .returning(|_, _| Ok("abc123.onion".into()));
        tor.expect_remove_hidden_service().returning(|_| Ok(()));
        tor.expect_get_onion_address()
            .returning(|_| Ok("abc123.onion".into()));

        VpnTorManager::new(
            Arc::new(MockVpn),
            Arc::new(MockBridge {
                fail_start: fail_bridge,
            }),
            Arc::new(tor),
        )
    }

    #[tokio::test]
    async fn test_enable_tor_returns_onion() {
        let mgr = make_manager(false);
        let onion = mgr.enable_tor("wg0").await.unwrap();
        assert_eq!(onion, "abc123.onion");
    }

    #[tokio::test]
    async fn test_enable_tor_bridge_failure() {
        let mgr = make_manager(true);
        assert!(mgr.enable_tor("wg0").await.is_err());
    }

    #[tokio::test]
    async fn test_disable_tor_ok() {
        let mgr = make_manager(false);
        assert!(mgr.disable_tor("wg0").await.is_ok());
    }

    #[tokio::test]
    async fn test_get_onion_ok() {
        let mgr = make_manager(false);
        assert_eq!(mgr.get_onion("wg0").await.unwrap(), "abc123.onion");
    }
}
