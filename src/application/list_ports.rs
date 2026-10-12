//! `ports list` — typed use case that aggregates all host/port bindings
//! managed by enola-cli (git, WordPress, Nginx, Tor onion services).

use crate::application::list_wordpress_sites::ListWordPressSites;
use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use serde::Serialize;
use std::sync::Arc;

/// One host/virtual port binding exposed by an enola-managed service.
#[derive(Debug, Clone, Serialize)]
pub struct PortEntry {
    /// Logical service name (e.g. `myblog`, `git1`, `proxy_x`).
    pub service: String,
    /// Service family: `git`, `wordpress`, `nginx`, `tor`.
    pub service_type: String,
    /// Port role: `backend`, `ssh`, `nginx-listen`, `nginx-listen-ssl`,
    /// `onion-http`, `onion-https`.
    pub role: String,
    /// Bound port number.
    pub port: u16,
    /// Bind interface (`127.0.0.1`) or `virtual` for onion ports.
    pub interface: String,
    /// `running`/`stopped` for containers, `active` for nginx, `virtual` for tor.
    pub status: String,
}

/// Lists every port binding managed by enola-cli across all service kinds.
pub struct ListPorts {
    container: Arc<dyn ContainerPort + Send + Sync>,
}

impl ListPorts {
    /// Builds the use case with the injected container port (bollard adapter).
    pub fn new(container: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        Self { container }
    }

    /// Aggregates git, WordPress, Nginx and Tor port entries, sorted by
    /// service name then role.
    pub async fn execute(&self) -> Result<Vec<PortEntry>> {
        let mut entries = self.git_entries().await?;
        entries.extend(self.wp_entries().await?);
        entries.extend(nginx_entries());
        entries.extend(tor_entries());
        entries.sort_by(|a, b| a.service.cmp(&b.service).then(a.role.cmp(&b.role)));
        Ok(entries)
    }

    /// Git/Forgejo bindings: container `enola-git-*`, private 3000 → backend,
    /// private 22 → ssh. Bollard reports bindings for stopped containers too.
    async fn git_entries(&self) -> Result<Vec<PortEntry>> {
        let mut out = Vec::new();
        for c in self.container.list_containers(true).await? {
            let Some(name) = c.name.strip_prefix("enola-git-") else {
                continue;
            };
            let status = container_status(&c.status);
            for p in &c.ports {
                let role = match private_port(p) {
                    Some(3000) => "backend",
                    Some(22) => "ssh",
                    _ => continue,
                };
                if let Some(port) = public_port(p) {
                    out.push(entry(name, "git", role, port, "127.0.0.1", &status));
                }
            }
        }
        Ok(out)
    }

    /// WordPress bindings: one `backend` entry per site with a mapped port.
    async fn wp_entries(&self) -> Result<Vec<PortEntry>> {
        let sites = ListWordPressSites::new(self.container.clone())
            .execute()
            .await?;
        Ok(sites
            .iter()
            .filter_map(|s| {
                s.port
                    .map(|p| entry(&s.name, "wordpress", "backend", p, "127.0.0.1", &s.status))
            })
            .collect())
    }
}

fn entry(
    service: &str,
    service_type: &str,
    role: &str,
    port: u16,
    interface: &str,
    status: &str,
) -> PortEntry {
    PortEntry {
        service: service.to_string(),
        service_type: service_type.to_string(),
        role: role.to_string(),
        port,
        interface: interface.to_string(),
        status: status.to_string(),
    }
}

fn container_status(status: &str) -> String {
    if status.to_lowercase().contains("up") || status.to_lowercase().contains("running") {
        "running".to_string()
    } else {
        "stopped".to_string()
    }
}

/// Private (container-side) port of a `"0.0.0.0:8080->80/tcp"` binding.
fn private_port(s: &str) -> Option<u16> {
    s.split("->").nth(1)?.split('/').next()?.parse().ok()
}

/// Public (host-side) port of a `"0.0.0.0:8080->80/tcp"` binding.
fn public_port(s: &str) -> Option<u16> {
    s.split(':')
        .nth(1)
        .and_then(|s| s.split('-').next())
        .and_then(|s| s.parse().ok())
}

/// Nginx bindings: `listen 127.0.0.1:PORT` directives in sites-enabled.
fn nginx_entries() -> Vec<PortEntry> {
    let mut out = Vec::new();
    let nginx_dir = std::path::Path::new("/etc/nginx/sites-enabled");
    let Ok(dir) = std::fs::read_dir(nginx_dir) else {
        return out;
    };
    for file in dir.flatten() {
        let name = file.file_name().to_string_lossy().to_string();
        let Ok(content) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        out.extend(parse_nginx_listens(&name, &content));
    }
    out
}

fn parse_nginx_listens(file_name: &str, content: &str) -> Vec<PortEntry> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("listen 127.0.0.1:") else {
            continue;
        };
        let Some(port_str) = rest
            .split(';')
            .next()
            .and_then(|s| s.split_whitespace().next())
        else {
            continue;
        };
        let Ok(port) = port_str.parse::<u16>() else {
            continue;
        };
        let role = if trimmed.contains("ssl") {
            "nginx-listen-ssl"
        } else {
            "nginx-listen"
        };
        out.push(entry(
            file_name.trim_start_matches("proxy_"),
            "nginx",
            role,
            port,
            "127.0.0.1",
            "active",
        ));
    }
    out
}

/// Tor onion bindings: `HiddenServiceDir`/`HiddenServicePort` in torrc files.
fn tor_entries() -> Vec<PortEntry> {
    let mut files = vec!["/etc/tor/torrc".to_string()];
    if let Ok(dir) = std::fs::read_dir("/etc/tor/enola.d") {
        for e in dir.flatten() {
            let p = e.path().to_string_lossy().to_string();
            if p.ends_with(".conf") {
                files.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for f in &files {
        if let Ok(content) = std::fs::read_to_string(f) {
            out.extend(parse_torrc_ports(&content));
        }
    }
    out
}

fn parse_torrc_ports(content: &str) -> Vec<PortEntry> {
    let mut out = Vec::new();
    let mut current_service = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("HiddenServiceDir") {
            current_service = trimmed
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_end_matches('/')
                .split('/')
                .next_back()
                .unwrap_or("")
                .trim_start_matches("enola_")
                .to_string();
        } else if trimmed.starts_with("HiddenServicePort") && !current_service.is_empty() {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(virtual_port) = parts[1].parse::<u16>() {
                    let role = if virtual_port == 443 {
                        "onion-https"
                    } else {
                        "onion-http"
                    };
                    out.push(entry(
                        &current_service,
                        "tor",
                        role,
                        virtual_port,
                        "virtual",
                        "virtual",
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;

    fn container(
        name: &str,
        status: &str,
        ports: Vec<&str>,
    ) -> crate::ports::container::ContainerInfo {
        crate::ports::container::ContainerInfo {
            id: "id".to_string(),
            name: name.to_string(),
            image: "img".to_string(),
            status: status.to_string(),
            ports: ports.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn maps_git_and_wp_containers_to_port_entries() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                container(
                    "enola-git-main",
                    "Up 2 hours",
                    vec!["127.0.0.1:13000->3000/tcp", "127.0.0.1:13022->22/tcp"],
                ),
                container("wp-mysite", "Up 2 hours", vec!["127.0.0.1:18080->80/tcp"]),
                container("db-mysite", "Up 2 hours", vec!["3306/tcp"]),
            ])
        });
        let entries = ListPorts::new(Arc::new(mock)).execute().await.unwrap();
        let git: Vec<_> = entries.iter().filter(|e| e.service_type == "git").collect();
        assert_eq!(git.len(), 2);
        assert!(git.iter().any(|e| e.role == "backend" && e.port == 13000));
        assert!(git.iter().any(|e| e.role == "ssh" && e.port == 13022));
        let wp: Vec<_> = entries
            .iter()
            .filter(|e| e.service_type == "wordpress")
            .collect();
        assert_eq!(wp.len(), 1);
        assert_eq!(wp[0].port, 18080);
        assert_eq!(wp[0].service, "mysite");
    }

    #[tokio::test]
    async fn ignores_stopped_git_without_mapped_ports() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers()
            .returning(|_| Ok(vec![container("enola-git-old", "Exited (0)", vec![])]));
        let entries = ListPorts::new(Arc::new(mock)).execute().await.unwrap();
        assert!(entries.iter().all(|e| e.service_type != "git"));
    }

    #[test]
    fn parses_nginx_listen_lines() {
        let conf = "server {\n    listen 127.0.0.1:8080;\n    listen 127.0.0.1:8443 ssl;\n}\n";
        let out = parse_nginx_listens("proxy_myblog", conf);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].service, "myblog");
        assert_eq!(out[0].role, "nginx-listen");
        assert_eq!(out[1].role, "nginx-listen-ssl");
        assert_eq!(out[1].port, 8443);
    }

    #[test]
    fn parses_torrc_hidden_service_ports() {
        let torrc = "HiddenServiceDir /var/lib/tor/enola_blog/\nHiddenServicePort 80 127.0.0.1:8080\nHiddenServicePort 443 127.0.0.1:8443\n";
        let out = parse_torrc_ports(torrc);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].service, "blog");
        assert_eq!(out[0].role, "onion-http");
        assert_eq!(out[1].role, "onion-https");
    }
}
