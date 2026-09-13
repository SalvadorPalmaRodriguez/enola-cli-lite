use crate::domain::error::{EnolaError, Result};
use crate::ports::tor::{ClientKeypair, TorManagerPort};
use std::sync::Arc;
pub struct ManageClientAuth {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
}
impl ManageClientAuth {
    pub fn new(tor_manager: Arc<dyn TorManagerPort + Send + Sync>) -> Self {
        Self { tor_manager }
    }

    /// Valida el nombre de cliente. El nombre se usa para construir el
    /// fichero `{client}.auth` dentro de `authorized_clients`
    /// (`src/adapters/tor.rs`), así que un nombre con '/' escribiría fuera
    /// del directorio. Se rechazan vacío, >64 chars, prefijo '.' (mata "."
    /// y "..") y cualquier char fuera de [A-Za-z0-9._-].
    fn validate_client_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(EnolaError::ValidationError(
                "Client name cannot be empty".to_string(),
            ));
        }
        if name.len() > 64 {
            return Err(EnolaError::ValidationError(
                "Client name too long: maximum 64 characters".to_string(),
            ));
        }
        if name.starts_with('.') {
            return Err(EnolaError::ValidationError(format!(
                "Invalid client name '{}': must not start with '.'",
                name
            )));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid client name '{}': only alphanumeric, '.', '_' and '-' allowed",
                name
            )));
        }
        Ok(())
    }

    /// Valida la clave pública x25519 (base32 sin padding, 52 chars).
    /// Además de la longitud exige el alfabeto BASE32_NOPAD (A-Z, 2-7):
    /// una cadena de 52 chars con '\n' inyectaría una línea extra en el
    /// fichero `.auth`.
    fn validate_client_pubkey(key: &str) -> Result<()> {
        if key.len() != 52 {
            // Tor x25519 base32 keys are 52 chars
            return Err(EnolaError::ValidationError(
                "Invalid key length. Must be 52 chars base32".to_string(),
            ));
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
        {
            return Err(EnolaError::ValidationError(
                "Invalid public key: expected base32 (A-Z, 2-7)".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn add_client(
        &self,
        service_name: &str,
        client_name: &str,
        public_key: &str,
    ) -> Result<()> {
        Self::validate_client_name(client_name)?;
        Self::validate_client_pubkey(public_key)?;
        // Ensure auth is enabled
        self.tor_manager.enable_client_auth(service_name).await?;
        self.tor_manager
            .add_client_auth(service_name, client_name, public_key)
            .await
    }
    pub async fn list_clients(&self, service_name: &str) -> Result<Vec<String>> {
        let services = self.tor_manager.list_hidden_services().await?;
        let service = services
            .into_iter()
            .find(|s| s.name == service_name)
            .ok_or_else(|| EnolaError::NotFound(format!("Service {} not found", service_name)))?;
        Ok(service.clients)
    }
    pub async fn toggle_auth(&self, service_name: &str, enable: bool) -> Result<()> {
        if enable {
            self.tor_manager.enable_client_auth(service_name).await
        } else {
            self.tor_manager.disable_client_auth(service_name).await
        }
    }
    pub async fn revoke_client(&self, service_name: &str, client_name: &str) -> Result<()> {
        Self::validate_client_name(client_name)?;
        self.tor_manager
            .revoke_client_auth(service_name, client_name)
            .await
    }
    pub async fn generate_keys(&self, client_name: &str) -> Result<ClientKeypair> {
        self.tor_manager.generate_client_keys(client_name).await
    }

    /// Rotate a client's public key: replace the stored public key with a new
    /// one provided by the client (generated locally with `tor auth generate`).
    ///
    /// Strict semantics: the client MUST already exist (`NotFound` otherwise —
    /// a typo in `--client` must never silently authorize a brand-new client),
    /// and rotation does NOT enable client auth on the service (enabling it as
    /// a side effect would change the service's exposure).
    ///
    /// This is atomic by design: the same `{client}.auth` file is overwritten
    /// with the new public key in a single write, so there is no window where
    /// the client has neither the old nor the new key. The private key is never
    /// generated nor handled by the operator.
    pub async fn rotate_client(
        &self,
        service_name: &str,
        client_name: &str,
        new_public_key: &str,
    ) -> Result<()> {
        Self::validate_client_name(client_name)?;
        Self::validate_client_pubkey(new_public_key)?;
        let clients = self.list_clients(service_name).await?;
        if !clients.iter().any(|c| c == client_name) {
            return Err(EnolaError::NotFound(format!(
                "Client '{}' not found on service '{}'. Create it first with: \
                 enola-cli tor auth add {} --client {} --pubkey <key>",
                client_name, service_name, service_name, client_name
            )));
        }
        self.tor_manager
            .add_client_auth(service_name, client_name, new_public_key)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct MockTorManager {
        services: Vec<TorServiceInfo>,
        should_fail: bool,
        enable_auth_called: Mutex<bool>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                services: vec![],
                should_fail: false,
                enable_auth_called: Mutex::new(false),
            }
        }

        fn with_services(services: Vec<TorServiceInfo>) -> Self {
            Self {
                services,
                should_fail: false,
                enable_auth_called: Mutex::new(false),
            }
        }

        fn failing() -> Self {
            Self {
                services: vec![],
                should_fail: true,
                enable_auth_called: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("List failed".into()))
            } else {
                Ok(self.services.clone())
            }
        }
        async fn deploy_hidden_service(&self, _: &str, _: Vec<(u16, u16)>) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn remove_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn get_onion_address(&self, _: &str) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn reload_tor(&self) -> Result<()> {
            Ok(())
        }
        async fn generate_client_keys(&self, _: &str) -> Result<ClientKeypair> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Generate failed".into()))
            } else {
                Ok(ClientKeypair {
                    public_key: "PUBKEY12345678901234567890123456789012345678901234".into(),
                    private_key: "PRIVKEY1234567890123456789012345678901234567890AB".into(),
                })
            }
        }
        async fn add_client_auth(&self, _: &str, _: &str, _: &str) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Add failed".into()))
            } else {
                Ok(())
            }
        }
        async fn disable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke_client_auth(&self, _: &str, _: &str) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Revoke failed".into()))
            } else {
                Ok(())
            }
        }
        async fn enable_client_auth(&self, _: &str) -> Result<()> {
            *self.enable_auth_called.lock().unwrap() = true;
            Ok(())
        }
        async fn stop_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn start_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn rotate_hidden_service_identity(&self, _: &str) -> Result<String> {
            Ok("new.onion".into())
        }
    }

    #[tokio::test]
    async fn test_add_client_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor.clone());
        // Valid 52 char base32 key
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.add_client("myservice", "client1", key).await;

        assert!(result.is_ok());
        assert!(*tor.enable_auth_called.lock().unwrap());
    }

    #[tokio::test]
    async fn test_add_client_empty_name() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.add_client("myservice", "", key).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_add_client_invalid_key_length() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);
        let key = "TOOSHORT";

        let result = use_case.add_client("myservice", "client1", key).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("52 chars"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_list_clients_success() {
        let services = vec![TorServiceInfo {
            name: "myservice".into(),
            hostname: "abc.onion".into(),
            hidden_service_dir: "/var/lib/tor/enola_myservice".into(),
            ports: vec![(80, "127.0.0.1:8080".into())],
            active: true,
            auth_enabled: true,
            clients: vec!["client1".into(), "client2".into()],
        }];
        let tor = Arc::new(MockTorManager::with_services(services));
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.list_clients("myservice").await;

        assert!(result.is_ok());
        let clients = result.unwrap();
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0], "client1");
        assert_eq!(clients[1], "client2");
    }

    #[tokio::test]
    async fn test_list_clients_service_not_found() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.list_clients("nonexistent").await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(msg)) => {
                assert!(msg.contains("nonexistent"));
            }
            _ => panic!("Expected NotFound"),
        }
    }

    #[tokio::test]
    async fn test_toggle_auth_enable() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor.clone());

        let result = use_case.toggle_auth("myservice", true).await;

        assert!(result.is_ok());
        assert!(*tor.enable_auth_called.lock().unwrap());
    }

    #[tokio::test]
    async fn test_toggle_auth_disable() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.toggle_auth("myservice", false).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_client_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.revoke_client("myservice", "client1").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_client_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.revoke_client("myservice", "client1").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rotate_client_success() {
        let services = vec![TorServiceInfo {
            name: "myservice".into(),
            hostname: "abc.onion".into(),
            hidden_service_dir: "/var/lib/tor/enola_myservice".into(),
            ports: vec![(80, "127.0.0.1:8080".into())],
            active: true,
            auth_enabled: true,
            clients: vec!["client1".into()],
        }];
        let tor = Arc::new(MockTorManager::with_services(services));
        let use_case = ManageClientAuth::new(tor.clone());
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.rotate_client("myservice", "client1", key).await;

        assert!(result.is_ok());
        // Strict rotate must NOT enable client auth as a side effect.
        assert!(!*tor.enable_auth_called.lock().unwrap());
    }

    #[tokio::test]
    async fn test_rotate_client_not_found() {
        let services = vec![TorServiceInfo {
            name: "myservice".into(),
            hostname: "abc.onion".into(),
            hidden_service_dir: "/var/lib/tor/enola_myservice".into(),
            ports: vec![(80, "127.0.0.1:8080".into())],
            active: true,
            auth_enabled: true,
            clients: vec!["other".into()],
        }];
        let tor = Arc::new(MockTorManager::with_services(services));
        let use_case = ManageClientAuth::new(tor);
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.rotate_client("myservice", "client1", key).await;

        match result {
            Err(EnolaError::NotFound(msg)) => {
                assert!(msg.contains("client1"));
                assert!(msg.contains("tor auth add"));
            }
            _ => panic!("Expected NotFound"),
        }
    }

    #[test]
    fn test_validate_client_name_rejects_traversal_and_bad_chars() {
        for bad in ["../evil", "a/b", "", ".", "..", ".hidden"] {
            assert!(
                ManageClientAuth::validate_client_name(bad).is_err(),
                "debe rechazar {:?}",
                bad
            );
        }
        let long_name = "a".repeat(65);
        assert!(ManageClientAuth::validate_client_name(&long_name).is_err());
        assert!(ManageClientAuth::validate_client_name("alice.laptop-1_2").is_ok());
    }

    #[test]
    fn test_validate_client_pubkey() {
        let valid = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";
        assert!(ManageClientAuth::validate_client_pubkey(valid).is_ok());
        assert!(ManageClientAuth::validate_client_pubkey("TOOSHORT").is_err());
        let lower = "abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst";
        assert_eq!(lower.len(), 52);
        assert!(ManageClientAuth::validate_client_pubkey(lower).is_err());
        let mut injected = valid.to_string();
        injected.replace_range(51..52, "\n");
        assert_eq!(injected.len(), 52);
        assert!(ManageClientAuth::validate_client_pubkey(&injected).is_err());
    }

    #[tokio::test]
    async fn test_add_client_rejects_traversal_name() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.add_client("myservice", "../evil", key).await;

        match result {
            Err(EnolaError::ValidationError(_)) => {}
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_revoke_client_rejects_invalid_name() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.revoke_client("myservice", "a/b").await;

        match result {
            Err(EnolaError::ValidationError(_)) => {}
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_rotate_client_invalid_key_length() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case
            .rotate_client("myservice", "client1", "TOOSHORT")
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("52 chars"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_generate_keys_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.generate_keys("client1").await;

        assert!(result.is_ok());
        let keypair = result.unwrap();
        assert!(keypair.private_key.starts_with("PRIVKEY"));
        assert!(keypair.public_key.starts_with("PUBKEY"));
    }

    #[tokio::test]
    async fn test_generate_keys_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.generate_keys("client1").await;

        assert!(result.is_err());
    }
}
