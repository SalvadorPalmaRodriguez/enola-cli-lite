/// VPN Bridge Port — injectable trait for the UDP-over-TCP socat bridge (VPN over Tor)
///
/// The bridge tunnels WireGuard's UDP traffic over TCP so it can be exposed
/// through a Tor hidden service (Tor only supports TCP).
///
/// All methods are synchronous (the adapter uses `std::process::Command`).
use crate::domain::vpn::VpnError;

/// Abstraction over the socat UDP→TCP bridge lifecycle.
///
/// Implemented by `SocatBridgeAdapter` in production.
/// Can be mocked in tests.
pub trait VpnBridgePort: Send + Sync {
    /// Start the bridge: listen on `tcp_port` and forward to `udp_port`.
    ///
    /// Creates and enables a systemd unit so the bridge survives reboots.
    fn start_bridge(&self, interface: &str, tcp_port: u16, udp_port: u16) -> Result<(), VpnError>;

    /// Stop and remove the bridge for an interface.
    fn stop_bridge(&self, interface: &str) -> Result<(), VpnError>;

    /// Check whether the bridge systemd unit is active.
    fn is_bridge_active(&self, interface: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBridge {
        active: bool,
    }

    impl VpnBridgePort for MockBridge {
        fn start_bridge(&self, _i: &str, _t: u16, _u: u16) -> Result<(), VpnError> {
            Ok(())
        }
        fn stop_bridge(&self, _i: &str) -> Result<(), VpnError> {
            Ok(())
        }
        fn is_bridge_active(&self, _i: &str) -> bool {
            self.active
        }
    }

    #[test]
    fn test_mock_bridge_start() {
        let mock = MockBridge { active: false };
        assert!(mock.start_bridge("wg0", 51821, 51820).is_ok());
    }

    #[test]
    fn test_mock_bridge_is_active() {
        let active = MockBridge { active: true };
        let inactive = MockBridge { active: false };
        assert!(active.is_bridge_active("wg0"));
        assert!(!inactive.is_bridge_active("wg0"));
    }
}
