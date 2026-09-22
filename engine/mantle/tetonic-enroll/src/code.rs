use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::crypto::{KeyPair, PublicKeyBytes};

pub const ENROLL_CODE_PREFIX: &str = "lokai-enroll-v1:";

pub const DEFAULT_ENROLL_PORT: u16 = 9470;
pub const DEFAULT_FABRIC_PORT: u16 = 9471;
pub const DEFAULT_ENROLL_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Error)]
pub enum CodeError {
    #[error("bad prefix")]
    Prefix,
    #[error("base64: {0}")]
    B64(#[from] base64::DecodeError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("expired")]
    Expired,
    #[error("unsupported version {0}")]
    Version(u32),
    #[error("invalid enrollment field: {0}")]
    InvalidField(String),
}

/// Payload embedded in the one-time enrollment code the user copies to the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentCode {
    pub v: u32,
    /// Base64(raw 32-byte secret). Shown once in the code string.
    pub secret_b64: String,
    pub worker_pubkey: PublicKeyBytes,
    pub audit_pubkey: PublicKeyBytes,
    pub host: String,
    pub enroll_port: u16,
    pub fabric_port: u16,
    pub expires_at: DateTime<Utc>,
    pub worker_id: String,
    /// Pinned TLS cert for HTTPS enrollment (AR2-1). Coordinator verifies this exact DER.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enroll_tls_cert_b64: Option<String>,
}

impl EnrollmentCode {
    pub fn new(
        worker_keys: &KeyPair,
        audit_keys: &KeyPair,
        host: impl Into<String>,
        enroll_port: u16,
        fabric_port: u16,
        ttl: Duration,
        enroll_tls_cert: Option<&[u8]>,
    ) -> Self {
        let mut secret = [0u8; 32];
        OsRng.fill_bytes(&mut secret);
        let secret_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret);
        let enroll_tls_cert_b64 = enroll_tls_cert
            .map(|cert| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cert));
        Self {
            v: 1,
            secret_b64,
            worker_pubkey: worker_keys.public(),
            audit_pubkey: audit_keys.public(),
            host: host.into(),
            enroll_port,
            fabric_port,
            expires_at: Utc::now()
                + chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::minutes(15)),
            worker_id: worker_keys.worker_id(),
            enroll_tls_cert_b64,
        }
    }

    pub fn secret_bytes(&self) -> Result<Vec<u8>, CodeError> {
        let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&self.secret_b64)?;
        if secret.len() != 32 {
            return Err(CodeError::InvalidField("secret must be 32 bytes".into()));
        }
        Ok(secret)
    }

    pub fn secret_hash(&self) -> Result<[u8; 32], CodeError> {
        Ok(Sha256::digest(&self.secret_bytes()?).into())
    }

    pub fn verify_not_expired(&self) -> Result<(), CodeError> {
        if Utc::now() > self.expires_at {
            return Err(CodeError::Expired);
        }
        Ok(())
    }

    pub fn display_string(&self) -> Result<String, CodeError> {
        Ok(format!(
            "{ENROLL_CODE_PREFIX}{}",
            encode_enrollment_code(self)?
        ))
    }
}

pub fn encode_enrollment_code(code: &EnrollmentCode) -> Result<String, CodeError> {
    let json = serde_json::to_vec(code)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
}

pub fn decode_enrollment_code(s: &str) -> Result<EnrollmentCode, CodeError> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let payload = compact
        .strip_prefix(ENROLL_CODE_PREFIX)
        .ok_or(CodeError::Prefix)?;
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)?;
    let code: EnrollmentCode = serde_json::from_slice(&json)?;
    if code.v != 1 {
        return Err(CodeError::Version(code.v));
    }
    validate_code_fields(&code)?;
    code.verify_not_expired()?;
    Ok(code)
}

fn validate_code_fields(code: &EnrollmentCode) -> Result<(), CodeError> {
    code.worker_pubkey
        .to_verifying()
        .map_err(|e| CodeError::InvalidField(format!("worker_pubkey: {e}")))?;
    code.audit_pubkey
        .to_verifying()
        .map_err(|e| CodeError::InvalidField(format!("audit_pubkey: {e}")))?;
    if code.host.trim().is_empty() {
        return Err(CodeError::InvalidField("host must not be empty".into()));
    }
    Ok(())
}

pub fn new_worker_id(fingerprint: &str) -> String {
    format!("worker_{fingerprint}")
}

/// Hostname or IP advertised in the enrollment code for coordinator reachability.
///
/// **Binaries only** — reads `LOKAI_ADVERTISE_HOST`; library callers should pass
/// an explicit host into [`EnrollmentCode::new`].
pub fn advertise_host() -> String {
    std::env::var("LOKAI_ADVERTISE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_tolerates_embedded_whitespace() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let code = EnrollmentCode::new(&w, &a, "127.0.0.1", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        let mut s = code.display_string().unwrap();
        s.insert(40, '\n');
        let back = decode_enrollment_code(&s).unwrap();
        assert_eq!(code.worker_id, back.worker_id);
    }

    #[test]
    fn encode_decode_round_trip() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let code = EnrollmentCode::new(&w, &a, "10.0.0.5", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        let encoded = encode_enrollment_code(&code).unwrap();
        let back = decode_enrollment_code(&format!("{ENROLL_CODE_PREFIX}{encoded}")).unwrap();
        assert_eq!(code, back);
    }

    #[test]
    fn expired_code_is_rejected() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let mut code =
            EnrollmentCode::new(&w, &a, "127.0.0.1", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        code.expires_at = Utc::now() - chrono::Duration::seconds(1);
        let s = format!(
            "{ENROLL_CODE_PREFIX}{}",
            encode_enrollment_code(&code).unwrap()
        );
        assert!(matches!(
            decode_enrollment_code(&s),
            Err(CodeError::Expired)
        ));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let mut code =
            EnrollmentCode::new(&w, &a, "127.0.0.1", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        code.v = 2;
        let s = format!(
            "{ENROLL_CODE_PREFIX}{}",
            encode_enrollment_code(&code).unwrap()
        );
        assert!(matches!(
            decode_enrollment_code(&s),
            Err(CodeError::Version(2))
        ));
    }

    #[test]
    fn invalid_worker_pubkey_is_rejected() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let mut code =
            EnrollmentCode::new(&w, &a, "127.0.0.1", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        code.worker_pubkey = PublicKeyBytes(vec![0u8; 16]);
        let s = format!(
            "{ENROLL_CODE_PREFIX}{}",
            encode_enrollment_code(&code).unwrap()
        );
        assert!(matches!(
            decode_enrollment_code(&s),
            Err(CodeError::InvalidField(_))
        ));
    }
}
