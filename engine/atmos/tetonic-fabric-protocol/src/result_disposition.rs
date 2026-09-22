//! Durable, idempotent result disposition records (M5-4).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::ResultId;
use tetonic_domain::ResultDisposition;

use crate::result::ResultEnvelope;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultDispositionRecord {
    pub result_id: ResultId,
    pub idempotency_key: String,
    pub disposition: ResultDisposition,
    pub reason: String,
    pub recorded_at: DateTime<Utc>,
    /// When true, retained for audit only — must not mutate task state.
    pub audit_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DispositionStore {
    by_result: HashMap<String, ResultDispositionRecord>,
    by_idempotency: HashMap<String, ResultDispositionRecord>,
}

impl DispositionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_by_result(&self, result_id: &ResultId) -> Option<&ResultDispositionRecord> {
        self.by_result.get(&result_id.0)
    }

    pub fn get_by_idempotency(&self, key: &str) -> Option<&ResultDispositionRecord> {
        self.by_idempotency.get(key)
    }

    /// Record disposition. Duplicate delivery of the same envelope returns the
    /// previously recorded disposition unchanged.
    pub fn record(
        &mut self,
        envelope: &ResultEnvelope,
        disposition: ResultDisposition,
        reason: impl Into<String>,
        audit_only: bool,
        now: DateTime<Utc>,
    ) -> ResultDispositionRecord {
        let key = envelope.body.idempotency_key.0.clone();
        if let Some(existing) = self.by_result.get(&envelope.body.result_id.0) {
            return existing.clone();
        }
        if let Some(existing) = self.by_idempotency.get(&key) {
            return existing.clone();
        }
        let rec = ResultDispositionRecord {
            result_id: envelope.body.result_id.clone(),
            idempotency_key: key.clone(),
            disposition,
            reason: reason.into(),
            recorded_at: now,
            audit_only,
        };
        self.by_result
            .insert(envelope.body.result_id.0.clone(), rec.clone());
        self.by_idempotency.insert(key, rec.clone());
        rec
    }
}
