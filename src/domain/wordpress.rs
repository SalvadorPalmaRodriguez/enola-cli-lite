use crate::domain::error::{EnolaError, Result};
use std::collections::HashSet;
use std::net::TcpListener;

/// Information about a WordPress site instance
#[derive(Debug, Clone)]
pub struct WordPressSiteInstance {
    pub name: String,
    pub http_port: u16,
    pub db_port: u16,
    pub status: String,
}

/// Port manager for WordPress sites
/// Ensures each WordPress instance gets unique, available ports
pub struct WordPressPortManager;

impl WordPressPortManager {
    // Port ranges for WordPress sites
    // Using high ports to avoid conflicts with system services
    const HTTP_PORT_START: u16 = 8000;
    const HTTP_PORT_END: u16 = 9999;

    const DB_PORT_START: u16 = 33060;
    const DB_PORT_END: u16 = 34000;

    /// Allocates a pair of ports (HTTP, DB) that are not currently in use.
    /// Checks both existing WordPress instances AND system-level port availability.
    pub fn allocate_ports(existing_instances: &[WordPressSiteInstance]) -> Result<(u16, u16)> {
        let mut used_http = HashSet::new();
        let mut used_db = HashSet::new();

        for instance in existing_instances {
            used_http.insert(instance.http_port);
            used_db.insert(instance.db_port);
        }

        let http_port = Self::find_free_port_with_system_check(
            Self::HTTP_PORT_START,
            Self::HTTP_PORT_END,
            &used_http,
        )
        .ok_or_else(|| {
            EnolaError::InfrastructureError(format!(
                "No available HTTP ports in range {}-{}",
                Self::HTTP_PORT_START,
                Self::HTTP_PORT_END
            ))
        })?;

        let db_port = Self::find_free_port_with_system_check(
            Self::DB_PORT_START,
            Self::DB_PORT_END,
            &used_db,
        )
        .ok_or_else(|| {
            EnolaError::InfrastructureError(format!(
                "No available DB ports in range {}-{}",
                Self::DB_PORT_START,
                Self::DB_PORT_END
            ))
        })?;

        Ok((http_port, db_port))
    }

    /// Allocates just an HTTP port for WordPress
    /// Useful when DB port is not exposed
    pub fn allocate_http_port(existing_instances: &[WordPressSiteInstance]) -> Result<u16> {
        let mut used_http = HashSet::new();

        for instance in existing_instances {
            used_http.insert(instance.http_port);
        }

        Self::find_free_port_with_system_check(
            Self::HTTP_PORT_START,
            Self::HTTP_PORT_END,
            &used_http,
        )
        .ok_or_else(|| {
            EnolaError::InfrastructureError(format!(
                "No available HTTP ports in range {}-{}",
                Self::HTTP_PORT_START,
                Self::HTTP_PORT_END
            ))
        })
    }

    /// Find a free port that is not in the used set AND not bound at system level
    fn find_free_port_with_system_check(start: u16, end: u16, used: &HashSet<u16>) -> Option<u16> {
        (start..=end).find(|port| !used.contains(port) && Self::is_port_available(*port))
    }

    /// Check if a port is available at the system level
    /// Tries to bind to the port - if successful, port is free
    fn is_port_available(port: u16) -> bool {
        TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
    }

    /// Find a single free port in a range
    #[allow(dead_code)]
    pub fn find_free_port(start: u16, end: u16) -> Option<u16> {
        (start..=end).find(|port| Self::is_port_available(*port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_ports_empty() {
        let existing: Vec<WordPressSiteInstance> = vec![];
        let result = WordPressPortManager::allocate_ports(&existing);
        assert!(result.is_ok());
        let (http, db) = result.unwrap();
        assert!(
            (WordPressPortManager::HTTP_PORT_START..=WordPressPortManager::HTTP_PORT_END)
                .contains(&http)
        );
        assert!(
            (WordPressPortManager::DB_PORT_START..=WordPressPortManager::DB_PORT_END).contains(&db)
        );
    }

    #[test]
    fn test_allocate_http_port_avoids_used() {
        let existing = vec![
            WordPressSiteInstance {
                name: "site1".to_string(),
                http_port: 8000,
                db_port: 33060,
                status: "running".to_string(),
            },
            WordPressSiteInstance {
                name: "site2".to_string(),
                http_port: 8001,
                db_port: 33061,
                status: "running".to_string(),
            },
        ];

        let result = WordPressPortManager::allocate_http_port(&existing);
        assert!(result.is_ok());
        let http = result.unwrap();

        // Should not use ports already in use
        assert_ne!(http, 8000);
        assert_ne!(http, 8001);
        // Should be the next available
        assert!(http >= 8002);
    }
}
