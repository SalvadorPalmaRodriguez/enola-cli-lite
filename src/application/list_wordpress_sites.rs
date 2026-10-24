// Lista de sitios WordPress (caso de uso tipado, MCP-004).
//
// Extraído de `cli::commands::wordpress::list()` para que el servidor MCP
// (application/) no dependa del CLI. Usa `ContainerPort` (bollard) en vez de
// `Command::new("docker")`.

use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Información de un sitio WordPress.
#[derive(Debug, Clone, Serialize)]
pub struct WordPressSiteInfo {
    pub name: String,
    pub wp_container: String,
    pub db_container: String,
    pub status: String,
    pub port: Option<u16>,
    pub onion_address: Option<String>,
}

/// Use Case: listar sitios WordPress.
pub struct ListWordPressSites {
    container: Arc<dyn ContainerPort + Send + Sync>,
}

impl ListWordPressSites {
    pub fn new(container: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        Self { container }
    }

    pub async fn execute(&self) -> Result<Vec<WordPressSiteInfo>> {
        let containers = self.container.list_containers(true).await?;
        let mut sites: HashMap<String, WordPressSiteInfo> = HashMap::new();

        for c in containers {
            if let Some(site_name) = c.name.strip_prefix("wp-") {
                let site = sites
                    .entry(site_name.to_string())
                    .or_insert_with(|| empty_site(site_name));
                site.wp_container = c.name.clone();
                site.status = running_or_stopped(&c.status);
                site.port = c.ports.iter().filter_map(|p| extract_port(p)).next();
            } else if let Some(site_name) = c.name.strip_prefix("db-") {
                let site = sites
                    .entry(site_name.to_string())
                    .or_insert_with(|| empty_site(site_name));
                site.db_container = c.name.clone();
            }
        }

        for site in sites.values_mut() {
            site.onion_address = get_wp_onion(&site.name);
        }

        let mut out: Vec<WordPressSiteInfo> = sites.into_values().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

fn empty_site(name: &str) -> WordPressSiteInfo {
    WordPressSiteInfo {
        name: name.to_string(),
        wp_container: String::new(),
        db_container: String::new(),
        status: "unknown".to_string(),
        port: None,
        onion_address: None,
    }
}

fn running_or_stopped(status: &str) -> String {
    if status.contains("Up") {
        "running".to_string()
    } else {
        "stopped".to_string()
    }
}

/// Extrae el puerto público de un string de ports bollard ("0.0.0.0:8080->80/tcp").
pub fn extract_port(ports_str: &str) -> Option<u16> {
    ports_str
        .split(':')
        .nth(1)
        .and_then(|s| s.split('-').next())
        .and_then(|s| s.parse().ok())
}

/// Lee la dirección .onion de un servicio WordPress desde el hostname de Tor.
pub fn get_wp_onion(name: &str) -> Option<String> {
    // Directorio canónico `enola_wp_<name>` (el que crea `wp publish`).
    let dir = crate::domain::naming::ServiceName::new(
        name,
        crate::domain::naming::ServiceType::WordPress,
    )
    .tor_dir_name();
    let hostname_path = format!("/var/lib/tor/{dir}/hostname");
    std::fs::read_to_string(&hostname_path)
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::{ContainerInfo, MockContainerPort};

    fn info(name: &str, status: &str, ports: Vec<String>) -> ContainerInfo {
        ContainerInfo {
            id: "id".into(),
            name: name.into(),
            image: "img".into(),
            status: status.into(),
            ports,
        }
    }

    #[tokio::test]
    async fn groups_wp_and_db_containers_by_site() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                info("wp-blog", "Up 2 hours", vec!["0.0.0.0:8080->80/tcp".into()]),
                info("db-blog", "Up 2 hours", vec![]),
            ])
        });

        let use_case = ListWordPressSites::new(Arc::new(mock));
        let sites = use_case.execute().await.unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "blog");
        assert_eq!(sites[0].wp_container, "wp-blog");
        assert_eq!(sites[0].db_container, "db-blog");
        assert_eq!(sites[0].status, "running");
        assert_eq!(sites[0].port, Some(8080));
    }

    #[test]
    fn extract_port_parses_bollard_format() {
        assert_eq!(extract_port("0.0.0.0:8080->80/tcp"), Some(8080));
        assert_eq!(extract_port("80/tcp"), None);
    }
}
