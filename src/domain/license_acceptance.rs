/// LIC-002: License acceptance domain model.
///
/// Tracks whether the user has accepted the proprietary license.
/// Persisted in `~/.enola/license_accepted.json`.
/// If the license version changes (new hash), acceptance is required again.
use serde::{Deserialize, Serialize};

/// Current license version identifier.
/// Bump this when the LICENSE file changes significantly.
pub(crate) const LICENSE_VERSION: &str = "1.1";

/// SHA-256 hash of the LICENSE file, computed at build time by build.rs.
/// Used to detect if the license text changed without bumping LICENSE_VERSION.
pub(crate) const LICENSE_HASH: &str = env!("ENOLA_LICENSE_HASH");

/// Persisted record of the user's license acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LicenseAcceptance {
    /// Whether the license was accepted.
    pub accepted: bool,
    /// ISO-8601 timestamp of when the user accepted.
    pub timestamp: String,
    /// Version of the license that was accepted (e.g. "1.0").
    pub license_version: String,
    /// SHA-256 hash of the LICENSE file at the time of acceptance.
    pub license_hash: String,
}

impl LicenseAcceptance {
    /// Returns true if this acceptance is still valid for the current license.
    pub(crate) fn is_valid_for_current(&self) -> bool {
        self.accepted
            && self.license_version == LICENSE_VERSION
            && self.license_hash == LICENSE_HASH
    }
}

/// The abbreviated license text shown to the user on first run.
pub(crate) const LICENSE_SUMMARY_ES: &str = "\
\x1b[1;36m═══════════════════════════════════════════════════════════\x1b[0m
\x1b[1;37m  ENOLA CLI — Licencia de Software Propietario\x1b[0m
\x1b[1;37m  Copyright (c) 2026 Salvador Palma Rodriguez\x1b[0m
\x1b[1;36m═══════════════════════════════════════════════════════════\x1b[0m

  Este software es \x1b[1;33mPROPIETARIO\x1b[0m. No es open source.
  Queda prohibida su modificación, redistribución
  e ingeniería inversa sin autorización escrita.

  • Uso gratuito con licencia Free (registro obligatorio).
  • Las vulnerabilidades deben reportarse en 72 horas.
  • Jurisdicción: España / Unión Europea.

  Licencia completa: \x1b[4mhttps://enola-cli.com/legal\x1b[0m
  Contacto: salvadorpalmarodriguez@gmail.com

\x1b[1;36m═══════════════════════════════════════════════════════════\x1b[0m";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_acceptance() {
        let acceptance = LicenseAcceptance {
            accepted: true,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: LICENSE_VERSION.to_string(),
            license_hash: LICENSE_HASH.to_string(),
        };
        assert!(acceptance.is_valid_for_current());
    }

    #[test]
    fn test_invalid_if_not_accepted() {
        let acceptance = LicenseAcceptance {
            accepted: false,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: LICENSE_VERSION.to_string(),
            license_hash: LICENSE_HASH.to_string(),
        };
        assert!(!acceptance.is_valid_for_current());
    }

    #[test]
    fn test_invalid_if_version_changed() {
        let acceptance = LicenseAcceptance {
            accepted: true,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: "0.9".to_string(),
            license_hash: LICENSE_HASH.to_string(),
        };
        assert!(!acceptance.is_valid_for_current());
    }

    #[test]
    fn test_invalid_if_hash_changed() {
        let acceptance = LicenseAcceptance {
            accepted: true,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: LICENSE_VERSION.to_string(),
            license_hash: "different_hash".to_string(),
        };
        assert!(!acceptance.is_valid_for_current());
    }

    #[test]
    fn test_license_summary_not_empty() {
        assert!(!LICENSE_SUMMARY_ES.is_empty());
        assert!(LICENSE_SUMMARY_ES.contains("PROPIETARIO"));
        assert!(LICENSE_SUMMARY_ES.contains("Salvador Palma Rodriguez"));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let acceptance = LicenseAcceptance {
            accepted: true,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: LICENSE_VERSION.to_string(),
            license_hash: LICENSE_HASH.to_string(),
        };
        let json = serde_json::to_string(&acceptance).unwrap();
        let deserialized: LicenseAcceptance = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_valid_for_current());
        assert_eq!(deserialized.timestamp, "2026-04-05T12:00:00Z");
    }

    // ── Error-path tests ──

    #[test]
    fn test_deserialization_invalid_json() {
        let result: Result<LicenseAcceptance, _> = serde_json::from_str("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialization_missing_fields() {
        let result: Result<LicenseAcceptance, _> = serde_json::from_str(r#"{"accepted": true}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_when_all_fields_wrong() {
        let acceptance = LicenseAcceptance {
            accepted: false,
            timestamp: "".to_string(),
            license_version: "0.0".to_string(),
            license_hash: "wrong".to_string(),
        };
        assert!(!acceptance.is_valid_for_current());
    }

    #[test]
    fn test_serialization_with_not_accepted() {
        let acceptance = LicenseAcceptance {
            accepted: false,
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            license_version: LICENSE_VERSION.to_string(),
            license_hash: LICENSE_HASH.to_string(),
        };
        let json = serde_json::to_string(&acceptance).unwrap();
        let deserialized: LicenseAcceptance = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.is_valid_for_current());
        assert!(!deserialized.accepted);
    }

    #[test]
    fn test_license_summary_contains_key_info() {
        assert!(LICENSE_SUMMARY_ES.contains("72 horas"));
        assert!(LICENSE_SUMMARY_ES.contains("España"));
        assert!(LICENSE_SUMMARY_ES.contains("salvadorpalmarodriguez@gmail.com"));
    }
}
