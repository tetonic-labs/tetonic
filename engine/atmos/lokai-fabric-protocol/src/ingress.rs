//! Worker ingress idempotency state (M5-1) — pure logic; persistence is caller-owned.

use std::collections::HashMap;

use lokai_domain::ids::{AttemptId, LeaseId};
use serde::{Deserialize, Serialize};

use crate::{FabricError, FabricErrorCode, IdempotencyKey, JobEnvelope, JobKind};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeliveryKey {
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub idempotency_key: IdempotencyKey,
}

impl DeliveryKey {
    pub fn from_job(job: &JobEnvelope) -> Self {
        Self {
            attempt_id: job.attempt_id.clone(),
            lease_id: job.lease_id.clone(),
            lease_epoch: job.lease_epoch,
            idempotency_key: job.idempotency_key.clone(),
        }
    }

    pub fn digest(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.attempt_id, self.lease_id, self.lease_epoch, self.idempotency_key.0
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalOutcome {
    InProgress,
    Completed,
    Failed(String),
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressRecord {
    pub delivery_key: DeliveryKey,
    pub input_digest: String,
    pub job_kind: JobKind,
    pub terminal: Option<TerminalOutcome>,
    /// Serialized HTTP JSON body to replay on duplicate delivery (legacy `/v1/chat`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_response_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngressDecision {
    AcceptNew,
    ReplayExisting(IngressRecord),
    DuplicateInFlight,
}

#[derive(Clone, Debug, Default)]
pub struct IngressState {
    records: HashMap<String, IngressRecord>,
    idempotency_digests: HashMap<String, String>,
}

impl IngressState {
    pub fn from_records(records: Vec<IngressRecord>) -> Self {
        let mut map = HashMap::new();
        let mut idempotency_digests = HashMap::new();
        for rec in records {
            idempotency_digests.insert(
                rec.delivery_key.idempotency_key.0.clone(),
                rec.input_digest.clone(),
            );
            map.insert(rec.delivery_key.digest(), rec);
        }
        Self {
            records: map,
            idempotency_digests,
        }
    }

    pub fn records(&self) -> Vec<IngressRecord> {
        self.records.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn accept_delivery(&mut self, job: &JobEnvelope) -> Result<IngressDecision, FabricError> {
        if let Some(existing_digest) = self.idempotency_digests.get(&job.idempotency_key.0) {
            if existing_digest != &job.input_digest.0 {
                return Err(FabricError {
                    code: FabricErrorCode::DuplicateConflict,
                    message: "idempotency key reused with different input digest".into(),
                    details: None,
                });
            }
        }
        let key = DeliveryKey::from_job(job);
        let digest = key.digest();
        if let Some(existing) = self.records.get(&digest) {
            if existing.input_digest != job.input_digest.0 {
                return Err(FabricError {
                    code: FabricErrorCode::DuplicateConflict,
                    message: "idempotency key reused with different input digest".into(),
                    details: None,
                });
            }
            match &existing.terminal {
                None | Some(TerminalOutcome::InProgress) => {
                    return Ok(IngressDecision::DuplicateInFlight);
                }
                Some(TerminalOutcome::Completed)
                | Some(TerminalOutcome::Failed(_))
                | Some(TerminalOutcome::Rejected(_)) => {
                    return Ok(IngressDecision::ReplayExisting(existing.clone()));
                }
            }
        }
        self.idempotency_digests
            .insert(job.idempotency_key.0.clone(), job.input_digest.0.clone());
        self.records.insert(
            digest,
            IngressRecord {
                delivery_key: key,
                input_digest: job.input_digest.0.clone(),
                job_kind: job.job_kind.clone(),
                terminal: Some(TerminalOutcome::InProgress),
                cached_response_json: None,
            },
        );
        Ok(IngressDecision::AcceptNew)
    }

    pub fn mark_terminal(
        &mut self,
        job: &JobEnvelope,
        outcome: TerminalOutcome,
        cached_response_json: Option<String>,
    ) -> Result<(), FabricError> {
        let digest = DeliveryKey::from_job(job).digest();
        let rec = self.records.get_mut(&digest).ok_or_else(|| FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "ingress record missing for terminal update".into(),
            details: None,
        })?;
        rec.terminal = Some(outcome);
        if cached_response_json.is_some() {
            rec.cached_response_json = cached_response_json;
        }
        Ok(())
    }
}
