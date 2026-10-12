// MCP-002 — Guardia de stdout.
//
// El protocolo MCP viaja por stdout en exclusiva. El resto del código (476+
// `println!` en el árbol) escribe por stdout; para que no rompa el JSON-RPC,
// al arrancar `enola-cli mcp` se duplica stdout como canal de protocolo y se
// redirige stdout a stderr. El transporte escribe SOLO por el fd duplicado.

#[cfg(unix)]
use std::os::unix::io::RawFd;

/// RAII que guarda el stdout original como fd de protocolo y redirige stdout
/// a stderr mientras está viva. Al soltarla, restaura stdout.
#[cfg(unix)]
#[allow(dead_code)] // MCP-003: instalado por execute_mcp (Fase 3)
#[must_use = "the guard redirects stdout and restores it on drop; keep it alive for the server lifetime"]
pub struct StdoutGuard {
    protocol_fd: RawFd,
}

#[cfg(unix)]
#[allow(dead_code)] // MCP-003: instalado por execute_mcp (Fase 3)
impl StdoutGuard {
    /// Instala la guardia: `dup(1)` (fd de protocolo) + `dup2(2, 1)`.
    pub fn install() -> std::io::Result<Self> {
        let protocol_fd = unsafe { libc::dup(libc::STDOUT_FILENO) };
        if protocol_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) } < 0 {
            let e = std::io::Error::last_os_error();
            unsafe {
                libc::close(protocol_fd);
            }
            return Err(e);
        }
        Ok(Self { protocol_fd })
    }

    /// fd por el que el transporte escribe los mensajes JSON-RPC.
    pub fn protocol_fd(&self) -> RawFd {
        self.protocol_fd
    }
}

#[cfg(unix)]
impl Drop for StdoutGuard {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.protocol_fd, libc::STDOUT_FILENO);
            libc::close(self.protocol_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializa los tests que manipulan fds del proceso (evita interferir con
    /// otros tests que corren en paralelo).
    static FD_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn guard_returns_distinct_fd_and_restores_on_drop() {
        let _g = FD_LOCK.lock().unwrap();

        let before = fstat_id(libc::STDOUT_FILENO);

        let guard = StdoutGuard::install().unwrap();
        let protocol_fd = guard.protocol_fd();
        assert_ne!(protocol_fd, libc::STDOUT_FILENO);
        assert!(protocol_fd > libc::STDERR_FILENO);
        // protocol_fd apunta al stdout original
        assert_eq!(fstat_id(protocol_fd), before);
        // stdout (1) ahora apunta a stderr (2)
        assert_eq!(fstat_id(libc::STDOUT_FILENO), fstat_id(libc::STDERR_FILENO));

        drop(guard);
        // stdout restaurado
        assert_eq!(fstat_id(libc::STDOUT_FILENO), before);
    }

    #[cfg(unix)]
    fn fstat_id(fd: RawFd) -> (u64, u64) {
        use std::mem::MaybeUninit;
        let mut st = MaybeUninit::<libc::stat>::uninit();
        let rc = unsafe { libc::fstat(fd, st.as_mut_ptr()) };
        assert_eq!(rc, 0, "fstat({fd}) failed");
        let st = unsafe { st.assume_init() };
        (st.st_dev as u64, st.st_ino as u64)
    }
}
