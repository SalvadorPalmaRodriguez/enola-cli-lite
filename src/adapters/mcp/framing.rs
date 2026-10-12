// MCP-002 — Framing NDJSON: una línea = un mensaje.
// Sin I/O de red: lee/escribe sobre tokio::io::AsyncBufRead/AsyncWrite.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Tamaño máximo de un frame entrante (1 MiB).
pub(crate) const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Lee el siguiente frame NDJSON (una línea no vacía), ignorando líneas en
/// blanco. Devuelve `Ok(None)` en EOF.
pub(crate) async fn read_frame<R>(reader: &mut R) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let msg = line.trim();
        if msg.is_empty() {
            continue;
        }
        if msg.len() > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame exceeds {MAX_FRAME_BYTES} bytes"),
            ));
        }
        return Ok(Some(msg.to_string()));
    }
}

/// Escribe `msg` como una línea NDJSON. Rechaza mensajes con `\n` sin escapar
/// (el mensaje debe ser una única línea).
pub(crate) async fn write_frame<W>(writer: &mut W, msg: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if msg.contains('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame must not contain an unescaped newline",
        ));
    }
    writer.write_all(msg.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn read_frame_reads_single_line() {
        let data = b"{\"jsonrpc\":\"2.0\"}\n";
        let mut r = BufReader::new(Cursor::new(data.to_vec()));
        assert_eq!(
            read_frame(&mut r).await.unwrap(),
            Some("{\"jsonrpc\":\"2.0\"}".to_string())
        );
    }

    #[tokio::test]
    async fn read_frame_skips_empty_lines() {
        let data = b"\n\n  \n{\"a\":1}\n";
        let mut r = BufReader::new(Cursor::new(data.to_vec()));
        assert_eq!(
            read_frame(&mut r).await.unwrap(),
            Some("{\"a\":1}".to_string())
        );
    }

    #[tokio::test]
    async fn read_frame_returns_none_on_eof() {
        let mut r = BufReader::new(Cursor::new(Vec::new()));
        assert_eq!(read_frame(&mut r).await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_frame_accepts_one_megabyte_line() {
        let line = "x".repeat(MAX_FRAME_BYTES);
        let mut data = line.clone().into_bytes();
        data.push(b'\n');
        let mut r = BufReader::new(Cursor::new(data));
        assert_eq!(read_frame(&mut r).await.unwrap(), Some(line));
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized_line() {
        let line = "x".repeat(MAX_FRAME_BYTES + 1);
        let mut data = line.into_bytes();
        data.push(b'\n');
        let mut r = BufReader::new(Cursor::new(data));
        assert!(read_frame(&mut r).await.is_err());
    }

    #[tokio::test]
    async fn write_frame_appends_newline() {
        let mut w = Cursor::new(Vec::new());
        write_frame(&mut w, "{\"a\":1}").await.unwrap();
        assert_eq!(w.into_inner(), b"{\"a\":1}\n");
    }

    #[tokio::test]
    async fn write_frame_rejects_unescaped_newline() {
        let mut w = Cursor::new(Vec::new());
        assert!(write_frame(&mut w, "line1\nline2").await.is_err());
    }

    #[tokio::test]
    async fn json_with_internal_newlines_stays_single_line() {
        let value = serde_json::json!({"text": "a\nb"});
        let msg = serde_json::to_string(&value).unwrap();
        assert!(!msg.contains('\n'));
        let mut w = Cursor::new(Vec::new());
        write_frame(&mut w, &msg).await.unwrap();
        let out = String::from_utf8(w.into_inner()).unwrap();
        assert_eq!(out, format!("{msg}\n"));
    }
}
