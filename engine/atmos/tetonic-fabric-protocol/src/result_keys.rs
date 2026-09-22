//! Worker result-signing key registry (M5-4).
//!
//! Prefer a dedicated result-signing key certified by the enrolled identity so
//! keys can rotate with narrower usage than the enrollment identity key.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{KeyId, WorkerId};

use crate::result_crypto::verifying_key_from_bytes;
use crate::{FabricError, FabricErrorCode};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultSigningKeyRecord {
    pub key_id: KeyId,
    pub worker_id: WorkerId,
    /// Raw Ed25519 verifying key (32 bytes).
    pub public_key: Vec<u8>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub revoked: bool,
    /// Optional certification by enrolled identity over canonical key binding.
    pub identity_certification: Option<Vec<u8>>,
    pub rotation_generation: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ResultSigningKeyRegistry {
    keys: Vec<ResultSigningKeyRecord>,
}

impl ResultSigningKeyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, record: ResultSigningKeyRecord) -> Result<(), FabricError> {
        verifying_key_from_bytes(&record.public_key)?;
        if record.key_id.0.trim().is_empty() {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: "empty key_id".into(),
                details: None,
            });
        }
        self.keys
            .retain(|k| !(k.worker_id == record.worker_id && k.key_id == record.key_id));
        self.keys.push(record);
        Ok(())
    }

    pub fn revoke(&mut self, worker_id: &WorkerId, key_id: &KeyId) -> bool {
        let mut found = false;
        for k in &mut self.keys {
            if &k.worker_id == worker_id && &k.key_id == key_id {
                k.revoked = true;
                found = true;
            }
        }
        found
    }

    pub fn revoke_worker(&mut self, worker_id: &WorkerId) {
        for k in &mut self.keys {
            if &k.worker_id == worker_id {
                k.revoked = true;
            }
        }
    }

    pub fn lookup(
        &self,
        worker_id: &WorkerId,
        key_id: &KeyId,
        now: DateTime<Utc>,
    ) -> Result<&ResultSigningKeyRecord, FabricError> {
        let rec = self
            .keys
            .iter()
            .find(|k| &k.worker_id == worker_id && &k.key_id == key_id)
            .ok_or_else(|| FabricError {
                code: FabricErrorCode::InvalidSignature,
                message: format!(
                    "unknown result signing key {} for worker {}",
                    key_id.0, worker_id.0
                ),
                details: None,
            })?;
        if rec.revoked {
            return Err(FabricError {
                code: FabricErrorCode::RevokedIdentity,
                message: format!("result signing key {} revoked", key_id.0),
                details: None,
            });
        }
        if now < rec.valid_from {
            return Err(FabricError {
                code: FabricErrorCode::InvalidSignature,
                message: "result signing key not yet valid".into(),
                details: None,
            });
        }
        if rec.valid_until.is_some_and(|until| now > until) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidSignature,
                message: "result signing key expired".into(),
                details: None,
            });
        }
        Ok(rec)
    }

    pub fn keys_for_worker(&self, worker_id: &WorkerId) -> Vec<&ResultSigningKeyRecord> {
        self.keys
            .iter()
            .filter(|k| &k.worker_id == worker_id)
            .collect()
    }
}
