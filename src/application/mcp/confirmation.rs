// MCP-009 — Confirmación de dos pasos para operaciones destructivas.
//
// Modelo: una tool `Destructive` no se ejecuta en la primera llamada. El
// servidor emite un `confirm_token` (128-bit aleatorio) vinculado a la tool
// y al fingerprint canónico de sus argumentos efectivos; la ejecución real
// exige una segunda llamada con `confirm_token` y argumentos IDÉNTICOS.
//
// Propiedades:
//   - Single-use: consumir el token lo invalida (anti-replay).
//   - TTL: expira a los `CONFIRM_TTL_SECS` segundos (reloj monotónico).
//   - Vinculación: si cambia cualquier argumento, el token no casa.
//   - No es una autorización humana verificable: el token viaja por el mismo
//     canal que el agente. Su función es impedir ejecución accidental en un
//     solo paso y dar al cliente un punto de intercepción estable para
//     mostrar la operación al operador antes de confirmarla.
//
// El store vive en memoria del proceso (una sesión stdio = un proceso); los
// tokens no sobreviven a un reinicio del servidor — eso es intencionado.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Segundos de vida de un token de confirmación.
pub(crate) const CONFIRM_TTL_SECS: u64 = 300;

/// Máximo de confirmaciones pendientes retenidas en memoria (anti-DoS).
const MAX_PENDING: usize = 256;

/// Claves de meta-argumentos que NO forman parte de la operación confirmada.
/// Se excluyen del fingerprint para que `confirm_token`/`try_run` (y el
/// timeout de la meta-tool) no invaliden una confirmación legítima.
const META_ARG_KEYS: &[&str] = &["confirm_token", "try_run", "timeout_secs"];

struct Pending {
    scope: String,
    fingerprint: String,
    expires_at: Instant,
}

/// Almacén de confirmaciones pendientes (en memoria, por proceso).
pub(crate) struct ConfirmationStore {
    pending: Mutex<HashMap<String, Pending>>,
    ttl: Duration,
}

impl Default for ConfirmationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmationStore {
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(CONFIRM_TTL_SECS),
        }
    }

    /// Con TTL personalizado (tests).
    #[cfg(test)]
    fn with_ttl(ttl: Duration) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Devuelve los argumentos efectivos de la operación (sin meta-keys).
    pub(crate) fn effective_args(args: &Value) -> Value {
        match args {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    if !META_ARG_KEYS.contains(&k.as_str()) {
                        out.insert(k.clone(), v.clone());
                    }
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }

    /// Fingerprint canónico de `scope` + `args` (claves ordenadas, sin
    /// meta-keys). Dos llamadas producen el mismo fingerprint si y solo si
    /// la operación y sus parámetros efectivos son idénticos.
    fn fingerprint(scope: &str, args: &Value) -> String {
        let canonical = canonical_json(&Self::effective_args(args));
        format!("{scope}:{canonical}")
    }

    /// Registra una confirmación pendiente y devuelve su token.
    ///
    /// Rechaza (error de validación) si hay demasiadas pendientes sin
    /// consumir — protección ante un agente que genere tokens sin parar.
    pub(crate) fn create(&self, scope: &str, args: &Value) -> crate::domain::error::Result<String> {
        let mut pending = match self.pending.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let now = Instant::now();
        pending.retain(|_, p| p.expires_at > now);
        if pending.len() >= MAX_PENDING {
            return Err(crate::domain::error::EnolaError::ValidationError(
                "too many pending confirmations; retry later".to_string(),
            ));
        }
        let token = hex::encode(rand::random::<[u8; 16]>());
        pending.insert(
            token.clone(),
            Pending {
                scope: scope.to_string(),
                fingerprint: Self::fingerprint(scope, args),
                expires_at: now + self.ttl,
            },
        );
        Ok(token)
    }

    /// Consume un token: existe, no expiró, no se usó y su fingerprint casa
    /// con `scope` + `args`. Cualquier fallo invalida el intento (el token,
    /// si existía, se elimina para impedir reintentos de fuerza bruta sobre
    /// fingerprints — un token mal usado no sirve para nada más).
    pub(crate) fn consume(
        &self,
        token: &str,
        scope: &str,
        args: &Value,
    ) -> crate::domain::error::Result<()> {
        let mut pending = match self.pending.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let Some(p) = pending.remove(token) else {
            return Err(crate::domain::error::EnolaError::SecurityError(
                "unknown or already-consumed confirmation token".to_string(),
            ));
        };
        if p.expires_at <= Instant::now() {
            return Err(crate::domain::error::EnolaError::SecurityError(
                "confirmation token expired; request a new one".to_string(),
            ));
        }
        if p.scope != scope || p.fingerprint != Self::fingerprint(scope, args) {
            return Err(crate::domain::error::EnolaError::SecurityError(
                "confirmation token does not match this operation and parameters".to_string(),
            ));
        }
        Ok(())
    }

    /// Número de confirmaciones pendientes (diagnóstico/tests).
    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }
}

/// Serializa un `Value` con claves de objeto ordenadas recursivamente, de
/// modo que el fingerprint sea estable ante el orden de las claves.
fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_default(),
                        canonical_json(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", inner.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Resultado estructurado que devuelve el servidor cuando una operación
/// destructiva necesita confirmación de dos pasos.
pub(crate) fn confirmation_required_result(tool: &str, args: &Value, token: &str) -> Value {
    serde_json::json!({
        "confirmation_required": true,
        "confirm_token": token,
        "tool": tool,
        "params": ConfirmationStore::effective_args(args),
        "expires_in_secs": CONFIRM_TTL_SECS,
        "instructions": "This is a destructive, irreversible operation. To execute it, \
                         call the same tool again with identical arguments plus \
                         \"confirm_token\": \"<token>\". The token is single-use, expires \
                         and is bound to these exact parameters.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_then_consume_with_identical_args_works() {
        let store = ConfirmationStore::new();
        let args = json!({"name": "svc", "force": true});
        let token = store.create("tor_remove", &args).unwrap();
        assert_eq!(token.len(), 32);
        store.consume(&token, "tor_remove", &args).unwrap();
    }

    #[test]
    fn token_is_single_use() {
        let store = ConfirmationStore::new();
        let args = json!({"name": "svc"});
        let token = store.create("tor_remove", &args).unwrap();
        store.consume(&token, "tor_remove", &args).unwrap();
        let e = store.consume(&token, "tor_remove", &args).unwrap_err();
        assert!(e.to_string().contains("already-consumed") || e.to_string().contains("unknown"));
    }

    #[test]
    fn unknown_token_is_rejected() {
        let store = ConfirmationStore::new();
        let e = store
            .consume("deadbeef".repeat(4).as_str(), "tor_remove", &json!({}))
            .unwrap_err();
        assert!(e.to_string().contains("unknown"));
    }

    #[test]
    fn token_bound_to_exact_args() {
        let store = ConfirmationStore::new();
        let token = store.create("tor_remove", &json!({"name": "a"})).unwrap();
        // Otro nombre → mismatch
        assert!(store
            .consume(&token, "tor_remove", &json!({"name": "b"}))
            .is_err());
        // Otra tool → mismatch
        let t2 = store.create("tor_remove", &json!({"name": "a"})).unwrap();
        assert!(store
            .consume(&t2, "wp_hide", &json!({"name": "a"}))
            .is_err());
        // Arg extra → mismatch
        let t3 = store.create("tor_remove", &json!({"name": "a"})).unwrap();
        assert!(store
            .consume(&t3, "tor_remove", &json!({"name": "a", "extra": 1}))
            .is_err());
    }

    #[test]
    fn meta_keys_do_not_affect_fingerprint() {
        let store = ConfirmationStore::new();
        let base = json!({"name": "svc", "force": true});
        let token = store.create("tor_remove", &base).unwrap();
        // La segunda llamada añade meta-keys: no invalidan la confirmación.
        let with_meta =
            json!({"name": "svc", "force": true, "confirm_token": token, "try_run": false});
        store.consume(&token, "tor_remove", &with_meta).unwrap();
    }

    #[test]
    fn key_order_does_not_affect_fingerprint() {
        let store = ConfirmationStore::new();
        let token = store
            .create("t", &json!({"a": 1, "b": {"x": 1, "y": 2}}))
            .unwrap();
        store
            .consume(&token, "t", &json!({"b": {"y": 2, "x": 1}, "a": 1}))
            .unwrap();
    }

    #[test]
    fn expired_token_is_rejected() {
        let store = ConfirmationStore::with_ttl(Duration::from_millis(1));
        let args = json!({"name": "svc"});
        let token = store.create("tor_remove", &args).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        let e = store.consume(&token, "tor_remove", &args).unwrap_err();
        assert!(e.to_string().contains("expired"), "got: {e}");
    }

    #[test]
    fn pending_capacity_is_bounded() {
        let store = ConfirmationStore::new();
        for i in 0..MAX_PENDING {
            store.create("t", &json!({"i": i})).unwrap();
        }
        assert_eq!(store.pending_count(), MAX_PENDING);
        assert!(store.create("t", &json!({"i": "overflow"})).is_err());
    }

    #[test]
    fn confirmation_result_is_structured() {
        let r = confirmation_required_result("tor_remove", &json!({"name": "s"}), "tok");
        assert_eq!(r["confirmation_required"], true);
        assert_eq!(r["confirm_token"], "tok");
        assert_eq!(r["tool"], "tor_remove");
        assert_eq!(r["params"]["name"], "s");
    }
}
