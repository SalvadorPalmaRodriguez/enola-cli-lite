/// Domain types for WireGuard VPN management (Tarea 162)
///
/// Pure domain logic — no external dependencies (no tokio, no std::process).
/// Follows hexagonal architecture: domain knows nothing of adapters.
use std::fmt;

// ─────────────────────────────────────────────────────────────────────────────
// Tor bridge constants (VPN over Tor — UDP-over-TCP)
// ─────────────────────────────────────────────────────────────────────────────

/// Offset added to the WireGuard UDP port to derive the socat TCP bridge port.
pub const DEFAULT_BRIDGE_PORT_OFFSET: u16 = 1;

/// Default onion virtual port exposed to Tor clients (matches WireGuard default).
pub const DEFAULT_ONION_PORT: u16 = 51820;

/// Derive the socat TCP bridge port from the WireGuard UDP listen port.
///
/// The bridge listens on `wg_port + 1` and forwards TCP → UDP to `wg_port`.
/// Uses saturating add to avoid panicking on `u16` overflow.
pub fn bridge_tcp_port(wg_port: u16) -> u16 {
    wg_port.saturating_add(DEFAULT_BRIDGE_PORT_OFFSET)
}

// ─────────────────────────────────────────────────────────────────────────────
// VPN Peer — represents a WireGuard peer (client)
// ─────────────────────────────────────────────────────────────────────────────

/// A WireGuard VPN peer configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct VpnPeer {
    /// Human-readable name for this peer (e.g., "laptop", "phone")
    pub name: String,
    /// WireGuard public key (base64, 44 chars)
    pub public_key: String,
    /// Assigned IP address in the VPN subnet (e.g., "10.8.0.2")
    pub ip_address: String,
    /// Optional preshared key for extra security
    pub preshared_key: Option<String>,
    /// Optional DNS servers for this peer
    pub dns: Option<Vec<String>>,
    /// Allowed IPs that will route through the VPN
    pub allowed_ips: Vec<String>,
    /// Peer state
    pub state: VpnPeerState,
}

impl VpnPeer {
    /// Create a new peer with defaults.
    pub fn new(
        name: impl Into<String>,
        public_key: impl Into<String>,
        ip: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            public_key: public_key.into(),
            ip_address: ip.into(),
            preshared_key: None,
            dns: None,
            allowed_ips: vec!["0.0.0.0/0".to_string(), "::/0".to_string()],
            state: VpnPeerState::Active,
        }
    }
}

/// State of a VPN peer.
#[derive(Debug, Clone, PartialEq)]
pub enum VpnPeerState {
    Active,
    Disabled,
}

impl fmt::Display for VpnPeerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VpnPeerState::Active => write!(f, "active"),
            VpnPeerState::Disabled => write!(f, "disabled"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VPN Server — represents a WireGuard server interface
// ─────────────────────────────────────────────────────────────────────────────

/// A WireGuard VPN server instance.
#[derive(Debug, Clone)]
pub struct VpnServer {
    /// Interface name (e.g., "wg0", "enola-vpn")
    pub interface: String,
    /// Server listening port (default: 51820)
    pub port: u16,
    /// VPN subnet (e.g., "10.8.0.0/24")
    pub subnet: String,
    /// Server IP in the VPN (e.g., "10.8.0.1")
    pub server_ip: String,
    /// Server public key (base64)
    pub public_key: String,
    /// Whether the interface is up
    pub is_active: bool,
    /// Connected peers
    pub peers: Vec<VpnPeer>,
    /// Server .onion address (if exposed via Tor)
    pub onion_address: Option<String>,
}

impl VpnServer {
    /// Create a new VPN server config with sensible defaults.
    pub fn new(interface: impl Into<String>, port: u16, subnet: impl Into<String>) -> Self {
        let subnet_str: String = subnet.into();
        // Derive server IP: first address in the subnet (e.g. 10.8.0.0/24 → 10.8.0.1)
        let server_ip = derive_server_ip(&subnet_str);
        Self {
            interface: interface.into(),
            port,
            subnet: subnet_str,
            server_ip,
            public_key: String::new(),
            is_active: false,
            peers: Vec::new(),
            onion_address: None,
        }
    }

    /// Number of active peers.
    pub fn active_peer_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|p| p.state == VpnPeerState::Active)
            .count()
    }

    /// Find a peer by name.
    pub fn get_peer(&self, name: &str) -> Option<&VpnPeer> {
        self.peers.iter().find(|p| p.name == name)
    }

    /// Next available IP in the subnet for a new peer.
    ///
    /// Scans existing peer IPs and returns the next free one.
    pub fn next_peer_ip(&self) -> Option<String> {
        let base = derive_subnet_base(&self.subnet)?;
        let used: std::collections::HashSet<u8> = self
            .peers
            .iter()
            .filter_map(|p| p.ip_address.split('.').next_back()?.parse::<u8>().ok())
            .collect();
        // Server uses .1, peers start at .2
        for octet in 2u8..=254 {
            if !used.contains(&octet) {
                return Some(format!("{}.{}", base, octet));
            }
        }
        None
    }
}

/// Validate a WireGuard interface name (alphanumeric + hyphens, max 15 chars).
pub fn validate_vpn_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("VPN name cannot be empty".to_string());
    }
    if name.len() > 15 {
        return Err(format!(
            "VPN interface name '{}' is too long (max 15 chars for Linux interfaces)",
            name
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "VPN name '{}' contains invalid characters (use alphanumeric, - or _)",
            name
        ));
    }
    Ok(())
}

/// Validate a WireGuard base64 public key (44 chars).
pub fn validate_public_key(key: &str) -> Result<(), String> {
    if key.len() != 44 {
        return Err(format!(
            "Invalid WireGuard public key: expected 44 chars, got {}",
            key.len()
        ));
    }
    let valid_chars = key
        .chars()
        .all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=');
    if !valid_chars {
        return Err("Invalid WireGuard public key: contains non-base64 characters".to_string());
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (pure functions)
// ─────────────────────────────────────────────────────────────────────────────

fn derive_server_ip(subnet: &str) -> String {
    let base = derive_subnet_base(subnet).unwrap_or_else(|| "10.8.0".to_string());
    format!("{}.1", base)
}

fn derive_subnet_base(subnet: &str) -> Option<String> {
    let ip_part = subnet.split('/').next()?;
    let parts: Vec<&str> = ip_part.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
}

// ─────────────────────────────────────────────────────────────────────────────
// VPN Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum VpnError {
    #[error("VPN interface '{0}' already exists")]
    AlreadyExists(String),
    #[error("VPN interface '{0}' not found")]
    NotFound(String),
    #[error("Peer '{0}' not found in VPN '{1}'")]
    PeerNotFound(String, String),
    #[error("Peer '{0}' already exists in VPN '{1}'")]
    PeerAlreadyExists(String, String),
    #[error(
        "WireGuard is not installed. Install with: sudo apt install wireguard wireguard-tools"
    )]
    WireGuardNotInstalled,
    #[error("VPN subnet is full — no more IPs available in {0}")]
    SubnetFull(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("System error: {0}")]
    SystemError(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_tcp_port_default() {
        assert_eq!(bridge_tcp_port(51820), 51821);
    }

    #[test]
    fn test_bridge_tcp_port_zero() {
        assert_eq!(bridge_tcp_port(0), 1);
    }

    #[test]
    fn test_bridge_tcp_port_overflow_saturates() {
        assert_eq!(bridge_tcp_port(u16::MAX), u16::MAX);
    }

    #[test]
    fn test_vpn_server_new_derives_server_ip() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        assert_eq!(srv.server_ip, "10.8.0.1");
        assert_eq!(srv.port, 51820);
        assert!(!srv.is_active);
    }

    #[test]
    fn test_vpn_server_new_172_subnet() {
        let srv = VpnServer::new("wg1", 51821, "172.16.0.0/24");
        assert_eq!(srv.server_ip, "172.16.0.1");
    }

    #[test]
    fn test_next_peer_ip_empty() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        assert_eq!(srv.next_peer_ip(), Some("10.8.0.2".to_string()));
    }

    #[test]
    fn test_next_peer_ip_with_peers() {
        let mut srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        srv.peers
            .push(VpnPeer::new("peer1", "AAAA".repeat(11), "10.8.0.2"));
        srv.peers
            .push(VpnPeer::new("peer2", "BBBB".repeat(11), "10.8.0.3"));
        assert_eq!(srv.next_peer_ip(), Some("10.8.0.4".to_string()));
    }

    #[test]
    fn test_validate_vpn_name_ok() {
        assert!(validate_vpn_name("wg0").is_ok());
        assert!(validate_vpn_name("enola-vpn").is_ok());
        assert!(validate_vpn_name("my_vpn1").is_ok());
    }

    #[test]
    fn test_validate_vpn_name_too_long() {
        assert!(validate_vpn_name("this-is-too-long-name").is_err());
    }

    #[test]
    fn test_validate_vpn_name_empty() {
        assert!(validate_vpn_name("").is_err());
    }

    #[test]
    fn test_validate_vpn_name_invalid_chars() {
        assert!(validate_vpn_name("wg 0").is_err());
        assert!(validate_vpn_name("wg.0").is_err());
    }

    #[test]
    fn test_validate_public_key_valid() {
        // 44-char base64 string (WireGuard format)
        let key = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";
        assert_eq!(key.len(), 44);
        assert!(validate_public_key(key).is_ok());
    }

    #[test]
    fn test_validate_public_key_wrong_length() {
        assert!(validate_public_key("tooshort").is_err());
        assert!(validate_public_key(&"A".repeat(45)).is_err());
    }

    #[test]
    fn test_peer_state_display() {
        assert_eq!(VpnPeerState::Active.to_string(), "active");
        assert_eq!(VpnPeerState::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_vpn_server_active_peer_count() {
        let mut srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        srv.peers
            .push(VpnPeer::new("p1", "A".repeat(44), "10.8.0.2"));
        let mut p2 = VpnPeer::new("p2", "B".repeat(44), "10.8.0.3");
        p2.state = VpnPeerState::Disabled;
        srv.peers.push(p2);
        assert_eq!(srv.active_peer_count(), 1);
    }

    #[test]
    fn test_vpn_server_get_peer() {
        let mut srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        srv.peers
            .push(VpnPeer::new("laptop", "A".repeat(44), "10.8.0.2"));
        assert!(srv.get_peer("laptop").is_some());
        assert!(srv.get_peer("phone").is_none());
    }

    #[test]
    fn test_vpn_error_display() {
        let e = VpnError::AlreadyExists("wg0".to_string());
        assert!(e.to_string().contains("wg0"));
        let e2 = VpnError::WireGuardNotInstalled;
        assert!(e2.to_string().contains("wireguard"));
    }

    // ── Error-path tests ──

    #[test]
    fn test_next_peer_ip_invalid_subnet() {
        let srv = VpnServer::new("wg0", 51820, "invalid");
        assert_eq!(srv.next_peer_ip(), None);
    }

    #[test]
    fn test_next_peer_ip_empty_subnet() {
        let srv = VpnServer::new("wg0", 51820, "");
        assert_eq!(srv.next_peer_ip(), None);
    }

    #[test]
    fn test_next_peer_ip_partial_subnet() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0");
        assert_eq!(srv.next_peer_ip(), None);
    }

    #[test]
    fn test_validate_public_key_invalid_chars() {
        let key = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8D!X";
        assert_eq!(key.len(), 44);
        assert!(validate_public_key(key).is_err());
    }

    #[test]
    fn test_validate_public_key_with_spaces() {
        let key = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8D X";
        assert_eq!(key.len(), 44);
        assert!(validate_public_key(key).is_err());
    }

    #[test]
    fn test_validate_vpn_name_with_special_chars() {
        assert!(validate_vpn_name("wg@0").is_err());
        assert!(validate_vpn_name("wg/0").is_err());
        assert!(validate_vpn_name("wg#0").is_err());
    }

    #[test]
    fn test_vpn_error_all_variants_display() {
        let e = VpnError::NotFound("wg1".to_string());
        assert!(e.to_string().contains("wg1"));
        assert!(e.to_string().contains("not found"));

        let e = VpnError::PeerNotFound("alice".to_string(), "wg0".to_string());
        assert!(e.to_string().contains("alice"));
        assert!(e.to_string().contains("wg0"));

        let e = VpnError::PeerAlreadyExists("bob".to_string(), "wg1".to_string());
        assert!(e.to_string().contains("bob"));

        let e = VpnError::SubnetFull("10.8.0.0/24".to_string());
        assert!(e.to_string().contains("10.8.0.0/24"));

        let e = VpnError::InvalidConfig("bad value".to_string());
        assert!(e.to_string().contains("bad value"));

        let e = VpnError::SystemError("permission denied".to_string());
        assert!(e.to_string().contains("permission denied"));
    }

    #[test]
    fn test_get_peer_not_found() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        assert!(srv.get_peer("nonexistent").is_none());
    }

    #[test]
    fn test_active_peer_count_empty() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        assert_eq!(srv.active_peer_count(), 0);
    }

    #[test]
    fn test_next_peer_ip_skips_server_ip() {
        let srv = VpnServer::new("wg0", 51820, "10.8.0.0/24");
        let ip = srv.next_peer_ip().unwrap();
        assert_ne!(ip, "10.8.0.1");
        assert_eq!(ip, "10.8.0.2");
    }
}
