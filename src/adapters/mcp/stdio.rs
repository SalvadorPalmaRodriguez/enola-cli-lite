// MCP-002/003 — Transporte stdio NDJSON + guardia de stdout.
//
// El protocolo MCP viaja por stdout en exclusiva. El resto del código (476+
// `println!` en el árbol) escribe por stdout; para que no rompa el JSON-RPC,
// al arrancar `enola-cli mcp` se duplica stdout como canal de protocolo y se
// redirige stdout a stderr. El transporte escribe SOLO por el fd duplicado.

#[cfg(unix)]
use std::os::unix::io::{FromRawFd, RawFd};

use crate::adapters::mcp::framing;
use crate::domain::error::Result;
use crate::ports::mcp_transport::McpTransport;

/// RAII que guarda el stdout original como fd de protocolo y redirige stdout
/// a stderr mientras está viva. Al soltarla, restaura stdout.
#[cfg(unix)]
#[must_use = "the guard redirects stdout and restores it on drop; keep it alive for the server lifetime"]
pub struct StdoutGuard {
    protocol_fd: RawFd,
}

#[cfg(unix)]
fn dup_cloexec(fd: RawFd) -> std::io::Result<RawFd> {
    let new_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if new_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(new_fd)
}

#[cfg(unix)]
impl StdoutGuard {
    /// Instala la guardia: `fcntl(1, F_DUPFD_CLOEXEC)` (fd de protocolo) + `dup2(2, 1)`.
    pub fn install() -> std::io::Result<Self> {
        let protocol_fd = dup_cloexec(libc::STDOUT_FILENO)?;
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

/// Transporte stdio que lee de stdin y escribe por el fd de protocolo.
#[cfg(unix)]
pub struct StdioTransport {
    _guard: StdoutGuard,
    stdin: tokio::io::BufReader<tokio::io::Stdin>,
    writer: tokio::fs::File,
}

#[cfg(unix)]
impl StdioTransport {
    pub fn new() -> std::io::Result<Self> {
        let guard = StdoutGuard::install()?;
        let write_fd = dup_cloexec(guard.protocol_fd())?;
        let file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        let writer = tokio::fs::File::from_std(file);
        Ok(Self {
            _guard: guard,
            stdin: tokio::io::BufReader::new(tokio::io::stdin()),
            writer,
        })
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn recv(&mut self) -> Result<Option<String>> {
        framing::read_frame(&mut self.stdin)
            .await
            .map_err(Into::into)
    }

    async fn send(&mut self, msg: &str) -> Result<()> {
        framing::write_frame(&mut self.writer, msg)
            .await
            .map_err(Into::into)
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
    #[test]
    fn guard_sets_cloexec_on_protocol_fd() {
        let _g = FD_LOCK.lock().unwrap();

        let guard = StdoutGuard::install().unwrap();
        let flags = unsafe { libc::fcntl(guard.protocol_fd(), libc::F_GETFD) };
        assert!(flags >= 0, "fcntl(F_GETFD) failed");
        assert_ne!(
            flags & libc::FD_CLOEXEC,
            0,
            "protocol_fd must have FD_CLOEXEC set so child processes never inherit it"
        );
        drop(guard);
    }

    #[cfg(unix)]
    fn fstat_id(fd: RawFd) -> (u64, u64) {
        use std::mem::MaybeUninit;
        let mut st = MaybeUninit::<libc::stat>::uninit();
        let rc = unsafe { libc::fstat(fd, st.as_mut_ptr()) };
        assert_eq!(rc, 0, "fstat({fd}) failed");
        let st = unsafe { st.assume_init() };
        (st.st_dev, st.st_ino)
    }
}
