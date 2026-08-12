// MED-01: Prevent unwrap/expect in non-test code — panics in nginx config
// generation can leave broken proxy configs that take down sites.
#![warn(clippy::unwrap_used, clippy::expect_used)]
use crate::domain::error::{EnolaError, Result};
use crate::infrastructure::file_lock::FileLock;
use crate::ports::web::{
    NginxFileServerConfig, NginxManagerPort, NginxProxyConfig, NginxProxyConfigWithSsl,
    NginxSiteConfig,
};
use std::path::PathBuf;
use tokio::fs;
use tokio::process::Command;

pub struct NginxAdapter {
    sites_available: PathBuf,
    sites_enabled: PathBuf,
}

impl Default for NginxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl NginxAdapter {
    pub fn new() -> Self {
        Self {
            sites_available: PathBuf::from("/etc/nginx/sites-available"),
            sites_enabled: PathBuf::from("/etc/nginx/sites-enabled"),
        }
    }

    /// SEC-EXT-RACE-012: reserva un puerto y devuelve el lock RAII asociado.
    ///
    /// El lock debe mantenerse vivo hasta que el caller consuma el puerto
    /// (p. ej. al escribir config + recargar nginx/crear servicio Docker).
    pub async fn find_available_port_with_lock(
        &self,
        range_start: u16,
        range_end: u16,
    ) -> Result<(u16, FileLock)> {
        use rand::Rng;
        use std::io::ErrorKind;
        use std::net::TcpListener;

        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let port = rng.gen_range(range_start..range_end);
            if TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
                continue;
            }

            let lock = match crate::infrastructure::port_lock::acquire_port_lock(port) {
                Ok(g) => g,
                Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(e) => {
                    return Err(EnolaError::InfrastructureError(format!(
                        "Failed to acquire lock for port {}: {}",
                        port, e
                    )))
                }
            };

            // Re-verifica tras tomar lock por si un proceso externo ocupó el puerto.
            if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
                return Ok((port, lock));
            }
        }

        if let Ok(listener) = TcpListener::bind("127.0.0.1:0") {
            if let Ok(addr) = listener.local_addr() {
                let port = addr.port();
                if let Ok(lock) = crate::infrastructure::port_lock::acquire_port_lock(port) {
                    if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
                        return Ok((port, lock));
                    }
                }
            }
        }

        Err(EnolaError::InfrastructureError(format!(
            "No available port found in range {}-{}",
            range_start, range_end
        )))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod sec_regression_tests {
    use super::*;
    use crate::ports::web::{NginxManagerPort, NginxProxyConfig};

    fn free_local_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind localhost")
            .local_addr()
            .expect("local addr")
            .port()
    }

    fn test_adapter() -> (NginxAdapter, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let available = tmp.path().join("sites-available");
        let enabled = tmp.path().join("sites-enabled");
        std::fs::create_dir_all(&available).expect("available dir");
        std::fs::create_dir_all(&enabled).expect("enabled dir");
        (
            NginxAdapter {
                sites_available: available,
                sites_enabled: enabled,
            },
            tmp,
        )
    }

    #[tokio::test]
    async fn proxy_config_enforces_localhost_and_security_headers() {
        let (adapter, _tmp) = test_adapter();
        let listen_port = free_local_port();
        let backend_port = free_local_port();
        let cfg = NginxProxyConfig {
            service_name: "svc-a".to_string(),
            listen_port,
            backend_port,
            server_name: "localhost".to_string(),
            rate_limit: None,
        };

        adapter
            .create_proxy_config(cfg)
            .await
            .expect("config created");

        let path = adapter.sites_available.join("proxy_svc-a");
        let content = std::fs::read_to_string(path).expect("read generated config");
        assert!(content.contains(&format!("listen 127.0.0.1:{};", listen_port)));
        assert!(content.contains(&format!("proxy_pass http://127.0.0.1:{};", backend_port)));
        assert!(content.contains("add_header X-Frame-Options"));
        assert!(content.contains("add_header Content-Security-Policy"));
    }

    #[tokio::test]
    async fn proxy_config_with_rate_limit_emits_limit_req_directives() {
        let (adapter, _tmp) = test_adapter();
        let listen_port = free_local_port();
        let backend_port = free_local_port();
        let cfg = NginxProxyConfig {
            service_name: "svc-b".to_string(),
            listen_port,
            backend_port,
            server_name: "localhost".to_string(),
            rate_limit: Some("10r/s".to_string()),
        };

        adapter
            .create_proxy_config(cfg)
            .await
            .expect("config created");

        let path = adapter.sites_available.join("proxy_svc-b");
        let content = std::fs::read_to_string(path).expect("read generated config");
        assert!(
            content.contains("limit_req_zone $binary_remote_addr zone=zone_svc-b:10m rate=10r/s;")
        );
        assert!(content.contains("limit_req zone=zone_svc-b burst=10 nodelay;"));
    }
}

#[async_trait::async_trait]
impl NginxManagerPort for NginxAdapter {
    async fn create_site_config(&self, config: NginxSiteConfig) -> Result<()> {
        /*
         Template:
         server {
             listen {port};
             root {root};
             index {indexes};
             server_name {domain};

             location / {
                 try_files $uri $uri/ =404;
                 {autoindex}
             }
         }
        */

        let index_str = config.index_files.join(" ");
        let autoindex_str = if config.autoindex {
            "autoindex on;"
        } else {
            "autoindex off;"
        };

        // Basic configuration content

        let content = format!(
            r#"
# Auto-generated by Enola Server
server {{
    listen {};
    server_name {};

    root {};
    index {};

    location / {{
        try_files $uri $uri/ =404;
        {}
    }}
}}
"#,
            config.listen_port, config.domain, config.root_dir, index_str, autoindex_str
        );

        let file_path = self.sites_available.join(&config.domain);

        fs::write(&file_path, content).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to write nginx config: {}", e))
        })?;

        Ok(())
    }

    async fn create_fileserver_config(&self, config: NginxFileServerConfig) -> Result<()> {
        let symlinks_directive = if config.disable_symlinks {
            "disable_symlinks on;"
        } else {
            "disable_symlinks off;"
        };

        let method_restriction = if !config.allow_upload {
            r#"
        limit_except GET HEAD {
            deny all;
        }
"#
        } else {
            ""
        };

        let content = format!(
            r#"
# Auto-generated by Enola Server (FileServer)
server {{
    # Listen only on localhost (Tor access only)
    listen 127.0.0.1:{};
    server_name localhost;

    root {};

    # File Server Settings
    autoindex on;
    autoindex_exact_size off;
    autoindex_localtime on;

    # Security: Symlinks
    {}

    # Security: Headers
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "DENY" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Permissions-Policy "camera=(), microphone=(), geolocation=(), interest-cohort=()" always;
    add_header Content-Security-Policy "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self';" always;

    server_tokens off;

    location / {{
        {}
        try_files $uri $uri/ =404;
    }}

    # Block hidden files
    location ~ /\. {{
        deny all;
        return 404;
    }}

    # SEC-018: Block dangerous file extensions
    location ~ \.(env|git|sql|pem|key|crt|der|p12|pfx)$ {{
        deny all;
        return 404;
    }}

    access_log /var/log/nginx/fileserver_{}_access.log;
    error_log /var/log/nginx/fileserver_{}_error.log;
}}
"#,
            config.listen_port,
            config.root_dir,
            symlinks_directive,
            method_restriction,
            config.service_name,
            config.service_name
        );

        let file_path = self
            .sites_available
            .join(format!("fileserver_{}", config.service_name));

        fs::write(&file_path, content).await.map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write nginx fileserver config: {}",
                e
            ))
        })?;

        Ok(())
    }

    async fn create_proxy_config(&self, config: NginxProxyConfig) -> Result<()> {
        // Verify port is available before creating config
        // This minimizes the race condition window
        use std::net::TcpListener;
        if TcpListener::bind(format!("127.0.0.1:{}", config.listen_port)).is_err() {
            return Err(EnolaError::InfrastructureError(format!(
                "Port {} is not available. Another service may be using it.",
                config.listen_port
            )));
        }

        // Rate limit configuration
        let (limit_zone, limit_directive) = if let Some(rate) = &config.rate_limit {
            let zone_name = format!("zone_{}", config.service_name);
            (
                format!(
                    "limit_req_zone $binary_remote_addr zone={}:10m rate={};",
                    zone_name, rate
                ),
                format!("limit_req zone={} burst=10 nodelay;", zone_name),
            )
        } else {
            (String::new(), String::new())
        };

        let content = format!(
            r#"
# Auto-generated by Enola Server (Reverse Proxy)
{}

server {{
    listen 127.0.0.1:{};
    server_name {};

    # Logs
    access_log /var/log/nginx/{}_access.log;
    error_log /var/log/nginx/{}_error.log;

    # Security Headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Permissions-Policy "camera=(), microphone=(), geolocation=()" always;
    add_header Content-Security-Policy "default-src 'self' http: https: data: blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'self'" always;

    server_tokens off;

    location / {{
        proxy_pass http://127.0.0.1:{};
        {}

        # Headers
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSockets support
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # Timeouts (optional, good defaults)
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }}
}}
"#,
            limit_zone,
            config.listen_port,
            config.server_name,
            config.service_name,
            config.service_name,
            config.backend_port,
            limit_directive
        );

        let file_path = self
            .sites_available
            .join(format!("proxy_{}", config.service_name));

        eprintln!("   [NGINX] Writing config to: {:?}", file_path);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        fs::write(&file_path, &content).await.map_err(|e| {
            eprintln!("   [NGINX] ERROR writing file: {}", e);
            EnolaError::InfrastructureError(format!("Failed to write nginx proxy config: {}", e))
        })?;

        eprintln!("   [NGINX] Config written successfully");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        Ok(())
    }

    async fn generate_self_signed_cert(&self, service_name: &str) -> Result<(String, String)> {
        let ssl_dir = PathBuf::from("/etc/nginx/ssl");

        // Create SSL directory if it doesn't exist
        fs::create_dir_all(&ssl_dir).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create SSL directory: {}", e))
        })?;

        let cert_path = ssl_dir.join(format!("{}.crt", service_name));
        let key_path = ssl_dir.join(format!("{}.key", service_name));

        eprintln!(
            "   [NGINX] Generating self-signed certificate for '{}'...",
            service_name
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // Generate self-signed certificate using openssl
        let key_str = key_path.to_string_lossy();
        let cert_str = cert_path.to_string_lossy();
        let output = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-nodes",
                "-days",
                "365",
                "-newkey",
                "rsa:4096",
                "-keyout",
                &key_str,
                "-out",
                &cert_str,
                "-subj",
                &format!("/CN={}.onion/O=Enola Server/C=XX", service_name),
            ])
            .output()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to run openssl: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EnolaError::InfrastructureError(format!(
                "Failed to generate certificate: {}",
                stderr
            )));
        }

        // Set proper permissions
        Command::new("chmod")
            .args(["600", &key_str])
            .output()
            .await
            .ok();

        eprintln!("   [NGINX] Certificate generated: {:?}", cert_path);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        Ok((
            cert_path.to_string_lossy().to_string(),
            key_path.to_string_lossy().to_string(),
        ))
    }

    async fn create_proxy_config_with_ssl(&self, config: NginxProxyConfigWithSsl) -> Result<()> {
        // Only verify that the HTTPS (listen) port is free.
        // The HTTP port is the backend port where the upstream service (e.g. Forgejo/Docker)
        // is already listening — Nginx proxies *to* it, so we must NOT expect it to be free.
        use std::net::TcpListener;
        if TcpListener::bind(format!("127.0.0.1:{}", config.https_port)).is_err() {
            return Err(EnolaError::InfrastructureError(format!(
                "HTTPS port {} is not available.",
                config.https_port
            )));
        }

        // Rate limit configuration
        let (limit_zone, limit_directive) = if let Some(rate) = &config.rate_limit {
            let zone_name = format!("zone_{}", config.service_name);
            (
                format!(
                    "limit_req_zone $binary_remote_addr zone={}:10m rate={};",
                    zone_name, rate
                ),
                format!("limit_req zone={} burst=10 nodelay;", zone_name),
            )
        } else {
            (String::new(), String::new())
        };

        let pqc_tls_directive = crate::infrastructure::pqc_tls::nginx_pqc_curve_directive();

        let content = format!(
            r#"
# Auto-generated by Enola Server (Reverse Proxy with SSL)
{}

# HTTP Server - Redirect to HTTPS or serve directly
server {{
    listen 127.0.0.1:{};
    server_name {};

    # Logs
    access_log /var/log/nginx/{}_http_access.log;
    error_log /var/log/nginx/{}_http_error.log;

    # Security Headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Permissions-Policy "camera=(), microphone=(), geolocation=()" always;
    add_header Content-Security-Policy "default-src 'self' http: https: data: blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'self'" always;

    server_tokens off;

    location / {{
        proxy_pass http://127.0.0.1:{};
        {}

        # Headers
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSockets support
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # Timeouts
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }}
}}

# HTTPS Server
server {{
    listen 127.0.0.1:{} ssl;
    server_name {};

    # SSL Configuration
    ssl_certificate {};
    ssl_certificate_key {};
    ssl_protocols TLSv1.3;
{}
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;
    ssl_session_cache shared:SSL:10m;
    ssl_session_timeout 1d;

    # Logs
    access_log /var/log/nginx/{}_https_access.log;
    error_log /var/log/nginx/{}_https_error.log;

    # Security Headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Permissions-Policy "camera=(), microphone=(), geolocation=()" always;
    add_header Content-Security-Policy "default-src 'self' http: https: data: blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'self'" always;
    add_header Strict-Transport-Security "max-age=31536000" always;

    server_tokens off;

    location / {{
        proxy_pass http://127.0.0.1:{};
        {}

        # Headers
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;

        # WebSockets support
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # Timeouts
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }}
}}
"#,
            limit_zone,
            // HTTP server block
            config.http_port,
            config.server_name,
            config.service_name,
            config.service_name,
            config.backend_port,
            limit_directive,
            // HTTPS server block
            config.https_port,
            config.server_name,
            config.ssl_cert_path,
            config.ssl_key_path,
            pqc_tls_directive,
            config.service_name,
            config.service_name,
            config.backend_port,
            limit_directive
        );

        let file_path = self
            .sites_available
            .join(format!("proxy_{}", config.service_name));

        eprintln!("   [NGINX] Writing SSL config to: {:?}", file_path);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        fs::write(&file_path, &content).await.map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write nginx SSL proxy config: {}",
                e
            ))
        })?;

        eprintln!("   [NGINX] SSL config written successfully");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        Ok(())
    }

    async fn enable_site(&self, domain: &str) -> Result<()> {
        // Try both with and without .conf extension for compatibility
        let available = self.sites_available.join(domain);
        let available_conf = self.sites_available.join(format!("{}.conf", domain));

        let (source_path, target_name) = if available.exists() {
            (available, domain.to_string())
        } else if available_conf.exists() {
            (available_conf, format!("{}.conf", domain))
        } else {
            return Err(EnolaError::NotFound(format!(
                "Site config '{}' not found in sites-available (tried with and without .conf)",
                domain
            )));
        };

        let enabled = self.sites_enabled.join(&target_name);

        if enabled.exists() {
            return Ok(()); // Already enabled
        }

        fs::symlink(&source_path, &enabled).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to symlink site: {}", e))
        })?;

        Ok(())
    }

    async fn disable_site(&self, domain: &str) -> Result<()> {
        // Try both with and without .conf extension for compatibility
        let link_path = self.sites_enabled.join(domain);
        let link_path_conf = self.sites_enabled.join(format!("{}.conf", domain));

        let path_to_remove = if link_path.exists() {
            Some(link_path)
        } else if link_path_conf.exists() {
            Some(link_path_conf)
        } else {
            None
        };

        if let Some(path) = path_to_remove {
            fs::remove_file(&path).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to disable site: {}", e))
            })?;
            self.reload().await?;
        }

        Ok(())
    }

    async fn delete_site_config(&self, domain: &str) -> Result<()> {
        // Try both with and without .conf extension for compatibility
        let config_path = self.sites_available.join(domain);
        let config_path_conf = self.sites_available.join(format!("{}.conf", domain));

        // Ensure disabled first
        self.disable_site(domain).await?;

        let path_to_delete = if config_path.exists() {
            Some(config_path)
        } else if config_path_conf.exists() {
            Some(config_path_conf)
        } else {
            None
        };

        if let Some(path) = path_to_delete {
            fs::remove_file(&path).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to delete site config: {}", e))
            })?;
        }

        Ok(())
    }

    async fn validate_config(&self) -> Result<bool> {
        let output = Command::new("nginx")
            .arg("-t")
            .output()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to run nginx -t: {}", e))
            })?;

        Ok(output.status.success())
    }

    async fn reload(&self) -> Result<()> {
        // First try reload
        let output = Command::new("systemctl")
            .args(["reload", "nginx"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => return Ok(()),
            _ => {
                // Reload failed, try restart
                eprintln!("   [NGINX] reload failed, trying restart...");
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }
        }

        // Try restart
        let output = Command::new("systemctl")
            .args(["restart", "nginx"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => return Ok(()),
            _ => {
                // Restart failed, try start
                eprintln!("   [NGINX] restart failed, trying start...");
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }
        }

        // Try start
        let output = Command::new("systemctl")
            .args(["start", "nginx"])
            .output()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to start nginx service: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EnolaError::InfrastructureError(format!(
                "Nginx start failed: {}",
                stderr
            )));
        }
        Ok(())
    }

    async fn update_proxy_ports(
        &self,
        domain: &str,
        listen_port: u16,
        backend_port: u16,
    ) -> Result<()> {
        // Try multiple naming patterns for the config file
        // Priority order matches how files are created:
        // 1. proxy_{domain} (created by create_proxy_config)
        // 2. {domain}.conf (legacy pattern)
        // 3. proxy_{domain}.conf (alternative pattern)
        // 4. {domain} (fallback without extension)
        let candidates = [
            self.sites_available.join(format!("proxy_{}", domain)),
            self.sites_available.join(format!("{}.conf", domain)),
            self.sites_available.join(format!("proxy_{}.conf", domain)),
            self.sites_available.join(domain),
        ];

        let config_path = candidates.iter().find(|p| p.exists());

        let config_path = match config_path {
            Some(path) => path.clone(),
            None => {
                return Err(EnolaError::NotFound(format!(
                    "Config file for {} not found (tried: proxy_{}, {}.conf, proxy_{}.conf, {})",
                    domain, domain, domain, domain, domain
                )));
            }
        };

        let content = fs::read_to_string(&config_path).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to read config: {}", e))
        })?;

        // Regex replacement would be robust, but simple string replace might work if format is consistent.
        // However, listening port can be "listen 127.0.0.1:PORT;"
        // And backend "proxy_pass http://127.0.0.1:PORT;"

        // We need regex crate for this to be safe.
        // Or we assume we can read valid port from structure?
        // Let's use simple line processing.

        let mut new_lines = Vec::new();
        let mut changed = false;

        for line in content.lines() {
            let trim_line = line.trim();
            if trim_line.starts_with("listen") && trim_line.contains("127.0.0.1:") {
                // Replace listen 127.0.0.1:OLD; with listen 127.0.0.1:NEW;
                // We don't know OLD, so we replace the whole line part
                let parts: Vec<&str> = line.split("127.0.0.1:").collect();
                if parts.len() > 1 {
                    // Keep indentation
                    let prefix = parts[0];
                    // Find suffix after ; (if any comments?)
                    new_lines.push(format!("{}127.0.0.1:{};", prefix, listen_port));
                    changed = true;
                    continue;
                }
            } else if trim_line.starts_with("proxy_pass") && trim_line.contains("127.0.0.1:") {
                let parts: Vec<&str> = line.split("127.0.0.1:").collect();
                if parts.len() > 1 {
                    let prefix = parts[0];
                    new_lines.push(format!("{}127.0.0.1:{};", prefix, backend_port));
                    changed = true;
                    continue;
                }
            }
            new_lines.push(line.to_string());
        }

        if changed {
            let new_content = new_lines.join("\n");
            fs::write(&config_path, new_content).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to write config: {}", e))
            })?;

            if self.validate_config().await? {
                self.reload().await?;
            } else {
                // Rollback?
                fs::write(&config_path, content).await.ok();
                return Err(EnolaError::ValidationError(
                    "New config invalid, rolled back".to_string(),
                ));
            }
        }

        Ok(())
    }

    async fn update_proxy_ports_with_ssl(
        &self,
        domain: &str,
        http_listen_port: u16,
        https_listen_port: Option<u16>,
        backend_port: u16,
    ) -> Result<()> {
        let candidates = [
            self.sites_available.join(format!("proxy_{}", domain)),
            self.sites_available.join(format!("{}.conf", domain)),
            self.sites_available.join(format!("proxy_{}.conf", domain)),
            self.sites_available.join(domain),
        ];

        let config_path = candidates.iter().find(|p| p.exists());

        let config_path = match config_path {
            Some(path) => path.clone(),
            None => {
                return Err(EnolaError::NotFound(format!(
                    "Config file for {} not found (tried: proxy_{}, {}.conf, proxy_{}.conf, {})",
                    domain, domain, domain, domain, domain
                )));
            }
        };

        let content = fs::read_to_string(&config_path).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to read config: {}", e))
        })?;

        let mut new_lines = Vec::new();
        let mut changed = false;

        for line in content.lines() {
            let trim_line = line.trim();

            if trim_line.starts_with("listen") && trim_line.contains("127.0.0.1:") {
                let parts: Vec<&str> = line.split("127.0.0.1:").collect();
                if parts.len() > 1 {
                    let prefix = parts[0];
                    if trim_line.contains("ssl") {
                        if let Some(https_port) = https_listen_port {
                            new_lines.push(format!("{}127.0.0.1:{} ssl;", prefix, https_port));
                            changed = true;
                            continue;
                        }
                    } else {
                        new_lines.push(format!("{}127.0.0.1:{};", prefix, http_listen_port));
                        changed = true;
                        continue;
                    }
                }
            } else if trim_line.starts_with("proxy_pass") && trim_line.contains("127.0.0.1:") {
                let parts: Vec<&str> = line.split("127.0.0.1:").collect();
                if parts.len() > 1 {
                    let prefix = parts[0];
                    new_lines.push(format!("{}127.0.0.1:{};", prefix, backend_port));
                    changed = true;
                    continue;
                }
            }
            new_lines.push(line.to_string());
        }

        if changed {
            let new_content = new_lines.join("\n");
            fs::write(&config_path, new_content).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to write config: {}", e))
            })?;
        }

        Ok(())
    }

    async fn list_enabled_sites(&self) -> Result<Vec<String>> {
        if !self.sites_enabled.exists() {
            return Ok(vec![]);
        }

        let mut sites = Vec::new();
        let mut entries = fs::read_dir(&self.sites_enabled).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                EnolaError::InfrastructureError(format!(
                    "Permission denied reading '{}'. Try: sudo enola-cli <command>",
                    self.sites_enabled.display()
                ))
            } else {
                EnolaError::InfrastructureError(format!(
                    "Cannot read Nginx sites directory '{}': {}",
                    self.sites_enabled.display(),
                    e
                ))
            }
        })?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                // Ignore hidden files?
                if !name.starts_with('.') {
                    sites.push(name);
                }
            }
        }

        sites.sort();
        Ok(sites)
    }

    async fn find_available_port(&self, range_start: u16, range_end: u16) -> Result<u16> {
        let (port, _lock) = self
            .find_available_port_with_lock(range_start, range_end)
            .await?;
        Ok(port)
    }

    async fn is_port_available(&self, port: u16) -> bool {
        use std::net::TcpListener;
        TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
    }
}

impl NginxAdapter {
    /// Detect if a configuration has SSL enabled, and extract ports and cert paths
    /// Returns (has_ssl, https_port, cert_path)
    pub async fn detect_ssl_config(
        &self,
        domain: &str,
    ) -> Result<(bool, Option<u16>, Option<String>)> {
        let candidates = [
            self.sites_available.join(format!("proxy_{}", domain)),
            self.sites_available.join(format!("{}.conf", domain)),
            self.sites_available.join(format!("proxy_{}.conf", domain)),
            self.sites_available.join(domain),
        ];

        let config_path = candidates.iter().find(|p| p.exists());

        if let Some(path) = config_path {
            let content = fs::read_to_string(path).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to read config: {}", e))
            })?;

            let has_ssl_cert = content.contains("ssl_certificate");
            let has_ssl_listen = content
                .lines()
                .any(|l| l.contains("listen") && l.contains("ssl"));

            if has_ssl_cert || has_ssl_listen {
                let https_port = content
                    .lines()
                    .find(|line| line.contains("listen") && line.contains("ssl"))
                    .and_then(|line| {
                        line.split("127.0.0.1:")
                            .nth(1)
                            .and_then(|s| s.split_whitespace().next())
                            .and_then(|s| {
                                s.trim_end_matches(';')
                                    .trim_end_matches("ssl")
                                    .trim()
                                    .parse()
                                    .ok()
                            })
                    });

                let cert_path = content
                    .lines()
                    .find(|line| {
                        line.contains("ssl_certificate") && !line.contains("ssl_certificate_key")
                    })
                    .and_then(|line| {
                        line.split_whitespace()
                            .nth(1)
                            .map(|s| s.trim_end_matches(';').to_string())
                    });

                return Ok((true, https_port, cert_path));
            }
        }

        Ok((false, None, None))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_default_constructor() {
        let adapter = NginxAdapter::default();
        assert_eq!(
            adapter.sites_available,
            PathBuf::from("/etc/nginx/sites-available")
        );
        assert_eq!(
            adapter.sites_enabled,
            PathBuf::from("/etc/nginx/sites-enabled")
        );
    }

    #[test]
    fn test_new_constructor() {
        let adapter = NginxAdapter::new();
        assert_eq!(
            adapter.sites_enabled,
            PathBuf::from("/etc/nginx/sites-enabled")
        );
    }

    #[tokio::test]
    async fn test_list_enabled_sites_nonexistent_dir() {
        let adapter = NginxAdapter {
            sites_available: PathBuf::from("/nonexistent/available"),
            sites_enabled: PathBuf::from("/nonexistent/enabled"),
        };
        let sites = adapter.list_enabled_sites().await.unwrap();
        assert!(sites.is_empty());
    }

    #[tokio::test]
    async fn test_list_enabled_sites_with_temp_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let enabled = dir.path().join("enabled");
        let available = dir.path().join("available");
        std::fs::create_dir_all(&enabled).unwrap();
        std::fs::create_dir_all(&available).unwrap();

        // Create a fake site config
        std::fs::write(enabled.join("test-site"), "server { listen 80; }").unwrap();

        let adapter = NginxAdapter {
            sites_available: available,
            sites_enabled: enabled,
        };
        let sites = adapter.list_enabled_sites().await.unwrap();
        assert_eq!(sites.len(), 1);
        assert!(sites[0].contains("test-site"));
    }

    #[tokio::test]
    async fn test_detect_ssl_config_no_ssl() {
        let dir = tempfile::TempDir::new().unwrap();
        let available = dir.path().join("available");
        std::fs::create_dir_all(&available).unwrap();
        std::fs::write(
            available.join("proxy_mysite"),
            "server { listen 80; server_name mysite.com; }",
        )
        .unwrap();

        let adapter = NginxAdapter {
            sites_available: available,
            sites_enabled: dir.path().join("enabled"),
        };
        let (has_ssl, port, cert) = adapter.detect_ssl_config("mysite").await.unwrap();
        assert!(!has_ssl);
        assert!(port.is_none());
        assert!(cert.is_none());
    }

    #[tokio::test]
    async fn test_detect_ssl_config_with_ssl() {
        let dir = tempfile::TempDir::new().unwrap();
        let available = dir.path().join("available");
        let enabled = dir.path().join("enabled");
        std::fs::create_dir_all(&available).unwrap();
        std::fs::create_dir_all(&enabled).unwrap();
        let content = r#"server {
    listen 127.0.0.1:8443 ssl;
    ssl_certificate /etc/ssl/certs/test.crt;
    ssl_certificate_key /etc/ssl/private/test.key;
}"#;
        // detect_ssl_config searches in sites_available with proxy_ prefix
        std::fs::write(available.join("proxy_secure-site"), content).unwrap();

        let adapter = NginxAdapter {
            sites_available: available,
            sites_enabled: enabled,
        };
        let (has_ssl, _port, cert) = adapter.detect_ssl_config("secure-site").await.unwrap();
        assert!(has_ssl);
        assert!(cert.is_some());
        assert!(cert.unwrap().contains("test.crt"));
    }

    #[tokio::test]
    async fn test_detect_ssl_config_nonexistent_domain() {
        let dir = tempfile::TempDir::new().unwrap();
        let enabled = dir.path().join("enabled");
        std::fs::create_dir_all(&enabled).unwrap();

        let adapter = NginxAdapter {
            sites_available: dir.path().join("available"),
            sites_enabled: enabled,
        };
        let (has_ssl, _, _) = adapter.detect_ssl_config("nonexistent").await.unwrap();
        assert!(!has_ssl);
    }
}
