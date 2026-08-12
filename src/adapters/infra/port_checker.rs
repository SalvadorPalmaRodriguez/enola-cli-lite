use crate::domain::error::EnolaError;
use crate::ports::port_checker::{PortCheckResult, PortCheckerPort, Result};
/// Implementación concreta de PortCheckerPort.
/// Tarea PORTS-003 (177).
///
/// Verifica disponibilidad de puertos en CUATRO niveles:
///   1. OS-level: TcpListener::bind en 127.0.0.1 Y 0.0.0.0
///   2. Docker-level: `docker ps -a` — contenedores PARADOS también retienen binding
///   3. Nginx-level: lee configs en /etc/nginx/sites-available/* (listen PORT)
///   4. Tor-level: lee configs en /etc/tor/enola.d/* (HiddenServicePort)
///
/// Ver §13.7: contenedores parados retienen puertos.
/// Ver §13.16: Docker bindea a 127.0.0.1, cadena Tor→Nginx→Docker.
/// UFW y AppArmor no bloquean puertos de loopback entre procesos locales,
/// pero sí el acceso externo — por eso el checker solo verifica disponibilidad
/// de socket, no si UFW/AppArmor los permite (eso es responsabilidad del servicio).
use std::net::TcpListener;
use std::process::Command;

pub struct PortCheckerAdapter;

impl PortCheckerAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Nivel 1: OS-level — TcpListener::bind en loopback Y wildcard.
    /// Si cualquiera falla, el puerto está ocupado.
    fn is_free_os(port: u16) -> bool {
        // Verificar loopback (127.0.0.1) — para servicios internos
        let free_loopback = TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok();
        // Verificar wildcard (0.0.0.0) — para detectar procesos que bindean en todas interfaces
        let free_wildcard = TcpListener::bind(format!("0.0.0.0:{}", port)).is_ok();
        free_loopback && free_wildcard
    }

    /// Nivel 2: Docker-level — incluye contenedores parados.
    /// Docker retiene el binding aunque el contenedor esté stopped.
    /// Ver §13.7: TcpListener dice libre, pero Docker no puede reusar el puerto.
    fn is_free_docker(port: u16) -> bool {
        let output = Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Ports}}"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                // Docker puede bindear como "127.0.0.1:PORT->", "0.0.0.0:PORT->" o ":PORT->"
                !stdout.contains(&format!(":{port}->"))
                    && !stdout.contains(&format!("127.0.0.1:{port}->"))
                    && !stdout.contains(&format!("0.0.0.0:{port}->"))
            }
            // Si Docker no está disponible, asumimos libre
            _ => true,
        }
    }

    /// Nivel 3: Nginx-level — lee configs en disco.
    /// Detecta puertos en "listen 127.0.0.1:PORT;" o "listen PORT;" en todas las configs.
    /// Esto evita colisiones cuando Nginx ya tiene el puerto en config pero aún no ha
    /// recargado, o cuando el proceso OS aún no está reflejado en TcpListener.
    fn is_free_nginx(port: u16) -> bool {
        let dirs = [
            "/etc/nginx/sites-available",
            "/etc/nginx/sites-enabled",
            "/etc/nginx/conf.d",
        ];
        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            let t = line.trim();
                            if !t.starts_with("listen") {
                                continue;
                            }
                            // Detectar "listen 127.0.0.1:PORT" o "listen PORT"
                            let p_str = port.to_string();
                            if t.contains(&format!(":{};", p_str))
                                || t.contains(&format!(": {};", p_str))
                                || t.contains(&format!(":{} ", p_str))
                                || t.contains(&format!(":{}\t", p_str))
                                || t == format!("listen {};", p_str)
                                || t.starts_with(&format!("listen {} ", p_str))
                                || t.starts_with(&format!("listen {};", p_str))
                            {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Nivel 4: Tor-level — lee configs torrc en /etc/tor/enola.d/.
    /// Detecta puertos en "HiddenServicePort VPORT 127.0.0.1:TARGET".
    fn is_free_tor(port: u16) -> bool {
        let dirs = ["/etc/tor/enola.d", "/etc/tor"];
        let p_str = port.to_string();
        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    // Solo leer archivos .conf de Enola, no los de sistema
                    if path.extension().map(|e| e != "conf").unwrap_or(true)
                        && !path.to_string_lossy().contains("enola")
                    {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            let t = line.trim();
                            if t.starts_with('#') {
                                continue;
                            }
                            // HiddenServicePort 80 127.0.0.1:BACKEND_PORT
                            // SocksPort PORT
                            if (t.starts_with("HiddenServicePort") || t.starts_with("SocksPort"))
                                && (t.ends_with(&format!(":{}", p_str))
                                    || t.contains(&format!(" {}", p_str))
                                    || t.contains(&format!("\t{}", p_str)))
                            {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Verificación completa: puerto libre en los 4 niveles.
    /// Este es el método que usa find_free_port internamente.
    pub fn is_port_fully_free(port: u16) -> bool {
        Self::is_free_os(port)
            && Self::is_free_docker(port)
            && Self::is_free_nginx(port)
            && Self::is_free_tor(port)
    }
}

impl Default for PortCheckerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PortCheckerPort for PortCheckerAdapter {
    fn check_port(&self, port: u16) -> Result<PortCheckResult> {
        if port == 0 {
            return Err(EnolaError::ValidationError(
                "Port 0 is not a valid port number".to_string(),
            ));
        }
        Ok(PortCheckResult {
            port,
            free_os: Self::is_free_os(port),
            // free_docker agrupa Docker + Nginx + Tor (todos los niveles no-OS)
            free_docker: Self::is_free_docker(port)
                && Self::is_free_nginx(port)
                && Self::is_free_tor(port),
        })
    }

    fn find_free_port(&self, start: u16, end: u16) -> Result<u16> {
        if start > end {
            return Err(EnolaError::ValidationError(format!(
                "Invalid range: start ({}) > end ({})",
                start, end
            )));
        }
        for port in start..=end {
            if Self::is_port_fully_free(port) {
                return Ok(port);
            }
        }
        Err(EnolaError::ValidationError(format!(
            "No free port found in range {}-{}. \
                 All ports are occupied by OS processes, Docker containers (including stopped), \
                 Nginx configs or Tor services. Free up ports or widen the range.",
            start, end
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_port_zero_is_error() {
        let checker = PortCheckerAdapter::new();
        assert!(checker.check_port(0).is_err());
    }

    #[test]
    fn test_find_free_port_invalid_range() {
        let checker = PortCheckerAdapter::new();
        assert!(checker.find_free_port(5000, 4000).is_err());
    }

    #[test]
    fn test_find_free_port_in_high_range() {
        let checker = PortCheckerAdapter::new();
        let result = checker.find_free_port(59000, 59100);
        assert!(
            result.is_ok(),
            "Should find a free port in 59000-59100: {:?}",
            result
        );
        let port = result.unwrap();
        assert!((59000..=59100).contains(&port));
    }

    #[test]
    fn test_check_port_high_port_is_free() {
        let checker = PortCheckerAdapter::new();
        let result = checker.check_port(59999);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_free_os_used_port() {
        // Use a fixed high port to avoid race conditions with parallel tests
        // that also use port 0 (let OS assign).
        let port: u16 = 58_321;
        // First make sure the port is free before we start
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(l) => l,
            Err(_) => {
                // Port already in use by something else — skip test instead of failing
                eprintln!(
                    "Port {} already in use, skipping test_is_free_os_used_port",
                    port
                );
                return;
            }
        };
        assert!(
            !PortCheckerAdapter::is_free_os(port),
            "Port {} should be occupied while listener is open",
            port
        );
        drop(listener);
        // Wait for OS to fully release the socket
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Retry up to 5 times (TIME_WAIT on some OSes)
        let mut free = false;
        for _ in 0..5 {
            if PortCheckerAdapter::is_free_os(port) {
                free = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(free, "Port {} should be free after dropping listener", port);
    }

    #[test]
    fn test_is_free_nginx_no_false_positive() {
        // Puerto muy alto sin config Nginx debe ser libre
        assert!(PortCheckerAdapter::is_free_nginx(59998));
    }

    #[test]
    fn test_is_free_tor_no_false_positive() {
        // Puerto muy alto sin config Tor debe ser libre
        assert!(PortCheckerAdapter::is_free_tor(59997));
    }

    #[test]
    fn test_is_port_fully_free_high_range() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        // OS may hold the port briefly after drop (TIME_WAIT).
        // Retry a few times with short sleeps.
        let mut ok = false;
        for _ in 0..20 {
            if PortCheckerAdapter::is_port_fully_free(port) {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(ok, "Port {} should be free after dropping listener", port);
    }

    #[test]
    fn test_find_free_port_skips_occupied() {
        // Bind un puerto y verifica que find_free_port lo salta
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Si el rango empieza en ese puerto, debe encontrar el siguiente libre
        if port < 59990 {
            let checker = PortCheckerAdapter::new();
            // Buscar en rango que incluye el puerto ocupado
            let result = checker.find_free_port(port, port + 20);
            if let Ok(found) = result {
                assert_ne!(found, port, "Should not return the occupied port {}", port);
            }
        }
        drop(listener);
    }

    #[test]
    fn test_find_free_port_error_message_informative() {
        let checker = PortCheckerAdapter::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Rango de 1 solo puerto ocupado
        let result = checker.find_free_port(port, port);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No free port") || err_msg.contains(&port.to_string()),
            "Error message should be informative: {}",
            err_msg
        );
        drop(listener);
    }
}
