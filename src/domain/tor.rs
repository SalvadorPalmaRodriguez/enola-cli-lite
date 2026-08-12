use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TorServiceInfo {
    pub name: String,
    pub hostname: String, // .onion address
    pub hidden_service_dir: String,
    pub ports: Vec<(u16, String)>, // (Public Port, Target Addr)
    pub clients: Vec<String>,      // List of authorized client names
    pub active: bool,              // Added active status
    pub auth_enabled: bool,        // Added auth status
}

impl std::fmt::Display for TorServiceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status_icon = if self.active { "🟢" } else { "🔴" };
        writeln!(f, "{} {} ", status_icon, self.name)?;
        writeln!(f, "   🧅 {}", self.hostname)?;
        for (pub_port, target) in &self.ports {
            writeln!(f, "   Port {} → {}", pub_port, target)?;
        }
        if self.auth_enabled {
            write!(f, "   🔐 Auth: ON")?;
            if !self.clients.is_empty() {
                write!(f, " (clients: {})", self.clients.join(", "))?;
            }
        } else {
            write!(f, "   🔓 Auth: OFF")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_service_info_display_active() {
        let info = TorServiceInfo {
            name: "web-service".to_string(),
            hostname: "abc123xyz.onion".to_string(),
            hidden_service_dir: "/var/lib/tor/enola_web-service".to_string(),
            ports: vec![(80, "127.0.0.1:8080".to_string())],
            active: true,
            auth_enabled: false,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("🟢"));
        assert!(display.contains("web-service"));
        assert!(display.contains("abc123xyz.onion"));
        assert!(display.contains("Port 80 → 127.0.0.1:8080"));
        assert!(display.contains("Auth: OFF"));
    }

    #[test]
    fn test_tor_service_info_display_inactive() {
        let info = TorServiceInfo {
            name: "stopped-svc".to_string(),
            hostname: "def456.onion".to_string(),
            hidden_service_dir: "/var/lib/tor/enola_stopped".to_string(),
            ports: vec![],
            active: false,
            auth_enabled: false,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("🔴"));
    }

    #[test]
    fn test_tor_service_info_display_with_auth() {
        let info = TorServiceInfo {
            name: "secure-svc".to_string(),
            hostname: "secure.onion".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![(443, "127.0.0.1:8443".to_string())],
            active: true,
            auth_enabled: true,
            clients: vec!["alice".to_string(), "bob".to_string()],
        };
        let display = format!("{}", info);
        assert!(display.contains("Auth: ON"));
        assert!(display.contains("alice"));
        assert!(display.contains("bob"));
    }

    #[test]
    fn test_tor_service_info_display_multiple_ports() {
        let info = TorServiceInfo {
            name: "multi-port".to_string(),
            hostname: "multi.onion".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![
                (80, "127.0.0.1:8080".to_string()),
                (443, "127.0.0.1:8443".to_string()),
                (22, "127.0.0.1:22".to_string()),
            ],
            active: true,
            auth_enabled: false,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("Port 80"));
        assert!(display.contains("Port 443"));
        assert!(display.contains("Port 22"));
    }

    // ── Error-path / edge-case tests ──

    #[test]
    fn test_tor_service_info_display_auth_no_clients() {
        let info = TorServiceInfo {
            name: "secure-svc".to_string(),
            hostname: "secure.onion".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![(443, "127.0.0.1:8443".to_string())],
            active: true,
            auth_enabled: true,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("Auth: ON"));
        assert!(!display.contains("clients:"));
    }

    #[test]
    fn test_tor_service_info_display_empty_hostname() {
        let info = TorServiceInfo {
            name: "pending".to_string(),
            hostname: "".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![],
            active: false,
            auth_enabled: false,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("🔴"));
        assert!(display.contains("pending"));
    }

    #[test]
    fn test_tor_service_info_display_empty_name() {
        let info = TorServiceInfo {
            name: "".to_string(),
            hostname: "abc.onion".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![(80, "127.0.0.1:8080".to_string())],
            active: true,
            auth_enabled: false,
            clients: vec![],
        };
        let display = format!("{}", info);
        assert!(display.contains("🟢"));
        assert!(display.contains("abc.onion"));
    }

    #[test]
    fn test_tor_service_info_display_inactive_with_auth() {
        let info = TorServiceInfo {
            name: "stopped-secure".to_string(),
            hostname: "xyz.onion".to_string(),
            hidden_service_dir: "/tmp".to_string(),
            ports: vec![],
            active: false,
            auth_enabled: true,
            clients: vec!["alice".to_string()],
        };
        let display = format!("{}", info);
        assert!(display.contains("🔴"));
        assert!(display.contains("Auth: ON"));
    }
}
