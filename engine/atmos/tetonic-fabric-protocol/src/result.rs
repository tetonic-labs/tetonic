//! Signed result envelopes (M5-4).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{
    AttemptId, JobId, KeyId, LeaseId, ResultId, RunId, TaskId, WorkerId, WorkspaceVersion,
};
use tetonic_domain::workspace::ContentDigest;

use crate::{CoordinatorId, IdempotencyKey, JobKind, ProtocolVersion};

pub const RESULT_ENVELOPE_VERSION: u32 = 1;

/// Opaque Ed25519 signature bytes (64).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Signature(pub Vec<u8>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Ok,
    Failed,
    Canceled,
    Preempted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedArtifactReference {
    pub artifact_id: String,
    pub digest: ContentDigest,
    pub size_bytes: u64,
    pub kind: String,
    /// Declared relative paths for patch/archive artifacts (validated in quarantine).
    #[serde(default)]
    pub declared_paths: Vec<String>,
}

/// Worker-local monotonic segment durations (M6-3). Never wall-clock stamps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkerLocalDurations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execute_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_load_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serialize_out_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionSummary {
    pub duration_ms: Option<u64>,
    pub model: Option<String>,
    pub exit_status: Option<i32>,
    pub truncated: bool,
    pub notes: Option<String>,
    /// Optional worker-local monotonic segments (M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<WorkerLocalDurations>,
}

/// Fields covered by the result signature (canonical JSON, keys sorted).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedResultBody {
    pub protocol_version: ProtocolVersion,
    pub result_envelope_version: u32,
    pub result_id: ResultId,
    pub worker_id: WorkerId,
    pub worker_key_id: KeyId,
    pub coordinator_id: CoordinatorId,
    pub run_id: RunId,
    pub job_id: JobId,
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub idempotency_key: IdempotencyKey,
    pub job_kind: JobKind,
    pub input_digest: ContentDigest,
    pub workspace_version: Option<WorkspaceVersion>,
    pub result_status: ResultStatus,
    pub result_digest: ContentDigest,
    pub artifacts: Vec<SignedArtifactReference>,
    pub execution_summary: ExecutionSummary,
    pub completed_at: DateTime<Utc>,
    pub revocation_epoch: u64,
}

/// Wire result envelope: signed body + signature. Signature proves provenance and
/// tamper detection only — never correctness of execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultEnvelope {
    #[serde(flatten)]
    pub body: SignedResultBody,
    pub signature: Signature,
    /// Untrusted payload bytes/JSON; never executed or applied directly.
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl ResultEnvelope {
    pub fn worker_id(&self) -> &WorkerId {
        &self.body.worker_id
    }

    pub fn result_id(&self) -> &ResultId {
        &self.body.result_id
    }

    pub fn signed_body(&self) -> &SignedResultBody {
        &self.body
    }
}
