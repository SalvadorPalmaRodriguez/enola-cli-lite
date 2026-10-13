use crate::domain::error::Result;

/// Transporte de mensajes MCP (una línea JSON por mensaje).
///
/// Es el único seam de I/O del servidor MCP: la implementación stdio y los
/// tests en memoria implementan este trait, de modo que todo el protocolo
/// puede probarse sin procesos ni tuberías.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub(crate) trait McpTransport {
    /// Recibe el siguiente mensaje (una línea completa). `None` = EOF.
    async fn recv(&mut self) -> Result<Option<String>>;

    /// Envía un mensaje (una línea, sin salto de línea final).
    async fn send(&mut self, msg: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_delivers_messages_in_order() {
        let mut mock = MockMcpTransport::new();
        let mut seq = vec![Ok(Some("one".to_string())), Ok(Some("two".to_string()))].into_iter();
        mock.expect_recv()
            .times(2)
            .returning(move || seq.next().unwrap());
        assert_eq!(mock.recv().await.unwrap(), Some("one".to_string()));
        assert_eq!(mock.recv().await.unwrap(), Some("two".to_string()));
    }

    #[tokio::test]
    async fn mock_recv_returns_none_on_eof() {
        let mut mock = MockMcpTransport::new();
        mock.expect_recv().times(1).returning(|| Ok(None));
        assert_eq!(mock.recv().await.unwrap(), None);
    }

    #[tokio::test]
    async fn mock_send_delegates() {
        let mut mock = MockMcpTransport::new();
        mock.expect_send().times(1).returning(|_| Ok(()));
        mock.send("hello").await.unwrap();
    }
}
