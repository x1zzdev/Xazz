//! Server-owned audit authentication. Keys/policy never come from log records or HTTP input.
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::audit_log::AuditRecord;

pub const ENV_KEYS: &str = "XAZZ_AUDIT_HMAC_KEYS";
const ENV_ACTIVE: &str = "XAZZ_AUDIT_HMAC_ACTIVE_KEY_ID";
const ENV_REQUIRED_FROM: &str = "XAZZ_AUDIT_HMAC_REQUIRED_FROM";

// Deliberately not Debug/Serialize: no diagnostic or API may print key material.
#[derive(Default)]
pub(crate) struct AuditAuth {
    keys: BTreeMap<String, Vec<u8>>,
    active: Option<String>,
    pub required_from: u64,
}

impl AuditAuth {
    pub fn from_settings(
        keys: Option<&str>,
        active: Option<&str>,
        required_from: Option<&str>,
    ) -> Result<Self, String> {
        let (keys, active) = match (keys, active, required_from) {
            (None, None, None) => return Ok(Self::default()),
            (Some(keys), Some(active), _) => (keys, active),
            _ => return Err("audit HMAC requires both KEYS and ACTIVE_KEY_ID".into()),
        };
        // Do not include serde/hex errors: they can contain submitted secrets.
        let encoded: BTreeMap<String, String> = serde_json::from_str(keys)
            .map_err(|_| "audit HMAC KEYS must be a JSON object of key IDs to hex keys")?;
        let mut decoded = BTreeMap::new();
        for (id, key) in encoded {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err("audit HMAC key IDs must be 1..64 ASCII letters/digits/-/_".into());
            }
            let bytes = hex::decode(key).map_err(|_| "audit HMAC key must be hex encoded")?;
            if bytes.len() < 32 {
                return Err("audit HMAC keys must contain at least 32 bytes".into());
            }
            decoded.insert(id, bytes);
        }
        if !decoded.contains_key(active) {
            return Err("audit HMAC active key ID is absent from KEYS".into());
        }
        let required_from = required_from
            .unwrap_or("0")
            .parse::<u64>()
            .map_err(|_| "audit HMAC REQUIRED_FROM must be a nonnegative integer")?;
        Ok(Self {
            keys: decoded,
            active: Some(active.into()),
            required_from,
        })
    }

    pub fn enabled(&self) -> bool {
        self.active.is_some()
    }

    pub fn sign(&self, record: &mut AuditRecord) {
        if let Some(id) = &self.active {
            record.hmac_key_id = Some(id.clone());
            record.hmac = Some(hex::encode(
                record_mac(record, &self.keys[id]).finalize().into_bytes(),
            ));
        }
    }

    pub fn verify(&self, record: &AuditRecord) -> bool {
        match (&record.hmac_key_id, &record.hmac) {
            (None, None) => !self.enabled() || record.index < self.required_from,
            (Some(id), Some(tag)) => {
                let Some(key) = self.keys.get(id) else {
                    return false;
                };
                let Ok(bytes) = hex::decode(tag) else {
                    return false;
                };
                // RustCrypto's constant-time comparison; never compare tag strings.
                record_mac(record, key).verify_slice(&bytes).is_ok()
            }
            _ => false,
        }
    }
}

fn record_mac(record: &AuditRecord, key: &[u8]) -> Hmac<Sha256> {
    // Versioned, unambiguous JSON tuple: binds field positions, null vs empty,
    // key ID and every semantic field (legacy NUL-delimited SHA stays unchanged).
    let payload = serde_json::to_vec(&(
        "xazz-audit-hmac-sha256-v1",
        &record.hmac_key_id,
        record.index,
        &record.timestamp,
        &record.hash,
        record.code_length,
        &record.outcome,
        &record.prompt_hash,
        &record.response_hash,
        &record.model_fingerprint,
        &record.prev_hash,
        &record.record_hash,
    ))
    .expect("audit tuple contains only JSON-serializable scalars");
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&payload);
    mac
}

pub(crate) fn configured() -> Result<&'static AuditAuth, String> {
    static AUTH: OnceLock<Result<AuditAuth, String>> = OnceLock::new();
    AUTH.get_or_init(|| {
        // Unit tests inject settings explicitly; never mutate/read operator secrets.
        #[cfg(test)]
        { Ok(AuditAuth::default()) }
        #[cfg(not(test))]
        {
            fn setting(name: &str) -> Result<Option<String>, String> {
                match std::env::var(name) {
                    Ok(value) => Ok(Some(value)),
                    Err(std::env::VarError::NotPresent) => Ok(None),
                    Err(_) => Err(format!("{name} must be valid Unicode")),
                }
            }
            let auth = AuditAuth::from_settings(
                setting(ENV_KEYS)?.as_deref(), setting(ENV_ACTIVE)?.as_deref(),
                setting(ENV_REQUIRED_FROM)?.as_deref(),
            )?;
            if !auth.enabled() {
                eprintln!("[xazz-server] WARNING: audit HMAC is not configured; logs have hash-chain integrity only, no record authentication");
            } else if auth.required_from > 0 {
                eprintln!("[xazz-server] WARNING: audit records before index {} are permitted legacy records without authentication", auth.required_from);
            }
            Ok(auth)
        }
    }).as_ref().map_err(Clone::clone)
}

/// Execution children must not inherit the server's audit signing authority.
pub(crate) fn remove_child_credentials(command: &mut std::process::Command) {
    for name in [ENV_KEYS, ENV_ACTIVE, ENV_REQUIRED_FROM] {
        command.env_remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4231_sha256_vector() {
        // https://www.rfc-editor.org/rfc/rfc4231.html#section-4.2
        let mut mac = Hmac::<Sha256>::new_from_slice(&[0x0b; 20]).unwrap();
        mac.update(b"Hi There");
        let expected =
            hex::decode("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
                .unwrap();
        mac.verify_slice(&expected).unwrap();
    }

    #[test]
    fn versioned_record_payload_matches_python_stdlib_hmac() {
        // Independent golden: Python json.dumps(ensure_ascii=False,
        // separators=(',', ':')) + hmac.new(bytes.fromhex('ab'*32), ..., sha256).
        // Pins UTF-8, tuple ordering, null handling, key ID and domain separation.
        let record = AuditRecord {
            index: 0,
            timestamp: "2026-10-22T00:00:00Z".into(),
            hash: "abc".into(),
            code_length: 4,
            outcome: Some("성공".into()),
            prompt_hash: None,
            response_hash: Some("r".into()),
            model_fingerprint: None,
            prev_hash: "GENESIS".into(),
            record_hash: "digest".into(),
            hmac_key_id: Some("old".into()),
            hmac: None,
        };
        let expected =
            hex::decode("e8fec96ef84c8203953e92a8795813659ee30a201f3784322da5a1153380cd10")
                .unwrap();
        record_mac(&record, &[0xab; 32])
            .verify_slice(&expected)
            .unwrap();
    }

    #[test]
    fn invalid_settings_fail_without_echoing_secrets() {
        for (keys, active, boundary) in [
            (Some("secret-input"), Some("k1"), None),
            (Some(r#"{"k1":"secret-input"}"#), Some("k1"), None),
            (Some(r#"{"k1":"abcd"}"#), Some("k1"), None),
            (None, Some("k1"), None),
            (None, None, Some("1")),
        ] {
            let err = AuditAuth::from_settings(keys, active, boundary)
                .err()
                .unwrap();
            assert!(!err.contains("secret-input"));
        }
        let keys = format!(r#"{{"k1":"{}"}}"#, "ab".repeat(32));
        assert!(AuditAuth::from_settings(Some(&keys), Some("missing"), None).is_err());
        assert!(AuditAuth::from_settings(Some(&keys), Some("k1"), Some("-1")).is_err());
        assert!(AuditAuth::from_settings(Some(&keys), Some("k1"), Some("2")).is_ok());
    }

    #[test]
    fn child_does_not_inherit_signing_configuration() {
        let mut command = std::process::Command::new("unused");
        command
            .env(ENV_KEYS, "test-secret")
            .env(ENV_ACTIVE, "k1")
            .env(ENV_REQUIRED_FROM, "0");
        remove_child_credentials(&mut command);
        for name in [ENV_KEYS, ENV_ACTIVE, ENV_REQUIRED_FROM] {
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == name && value.is_none())
            );
        }
    }
}
