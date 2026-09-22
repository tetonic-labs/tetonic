use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId, WorkerId};

use crate::{IdempotencyKey, ProtocolVersion};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleContext {
    pub job_id: JobId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub worker_id: WorkerId,
    pub sequence_number: u64,
    pub protocol_version: ProtocolVersion,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub context: LifecycleContext,
    pub attempt_state: String,
    pub heartbeat_sequence: u64,
    pub resource_usage_summary: Option<serde_json::Value>,
    pub progress_marker: Option<String>,
    pub current_output_size_bytes: u64,
    pub lease_renewal_request: bool,
    pub worker_health_status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobOffer {
    pub job: crate::JobEnvelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRenewalRequest {
    pub context: LifecycleContext,
    pub requested_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRenewalResponse {
    pub context: LifecycleContext,
    pub granted_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultRejected {
    pub context: LifecycleContext,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDraining {
    pub worker_id: WorkerId,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerUnavailable {
    pub worker_id: WorkerId,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAccepted {
    pub context: LifecycleContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRejected {
    pub context: LifecycleContext,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobStarted {
    pub context: LifecycleContext,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub context: LifecycleContext,
    pub progress_marker: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCompleted {
    pub context: LifecycleContext,
    pub result_artifact_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFailed {
    pub context: LifecycleContext,
    pub error_code: String,
    pub error_message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancellationRequest {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub reason: String,
    pub deadline: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancellationAcknowledged {
    pub context: LifecycleContext,
}
