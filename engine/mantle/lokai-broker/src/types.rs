//! Compute request / handle / status types (M6-1).

use chrono::{DateTime, Utc};
use lokai_domain::ids::{AttemptId, ReservationId, RunId, TaskId, WorkerId};
use lokai_domain::workspace::ContentDigest;
use lokai_domain::{
    ArtifactRef, DataClass, TraceContext, TrustPlacementDecision, VerificationPolicyReference,
    WorkspaceVersion,
};
use lokai_fabric_protocol::JobKind;
use serde::{Deserialize, Serialize};

use crate::budget::ResourceRequest;
use crate::priority::ComputePriority;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecisionReference {
    pub decision_id: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub policy_epoch: u64,
    pub decision: TrustPlacementDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlinePolicy {
    pub queue_deadline: DateTime<Utc>,
    pub execution_deadline: DateTime<Utc>,
}

impl Default for DeadlinePolicy {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            queue_deadline: now + chrono::Duration::minutes(5),
            execution_deadline: now + chrono::Duration::hours(1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicyReference {
    pub policy_id: String,
    pub max_attempts: u32,
}

impl Default for RetryPolicyReference {
    fn default() -> Self {
        Self {
            policy_id: "default".into(),
            max_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeRequest {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt_id: AttemptId,
    pub job_kind: JobKind,
    pub input_artifacts: Vec<ArtifactRef>,
    pub input_digest: ContentDigest,
    pub workspace_version: Option<WorkspaceVersion>,
    pub data_class: DataClass,
    pub placement_decision: PlacementDecisionReference,
    pub resource_request: ResourceRequest,
    pub deadline: DeadlinePolicy,
    pub retry_policy: RetryPolicyReference,
    pub verification_policy: VerificationPolicyReference,
    pub priority: ComputePriority,
    pub trace_context: TraceContext,
    /// When true, counts against speculation budgets in addition to ordinary ones.
    #[serde(default)]
    pub speculative: bool,
    /// Optional project scope for hierarchical budgets.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Preferred / assigned target after scheduling (may be local).
    #[serde(default)]
    pub target_worker_id: Option<WorkerId>,
    /// Ordered fallback targets (worker ids or `"local"`) from the scheduler.
    #[serde(default)]
    pub fallback_order: Vec<String>,
    /// Persisted scheduler decision id (M6-2 / M6-3 correlation).
    #[serde(default)]
    pub scheduler_decision_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeHandle {
    pub attempt_id: AttemptId,
    pub reservation_id: Option<ReservationId>,
    pub status: ComputeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeStatus {
    Queued,
    Reserved,
    Dispatched,
    Running,
    Succeeded,
    Failed { reason: String },
    Canceled { reason: String },
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancellationReason {
    UserCanceled,
    RunCanceled,
    TaskCanceled,
    Superseded,
    DeadlineExceeded,
    WorkerLost,
    AdmissionRevoked,
    Other(String),
}

impl CancellationReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::UserCanceled => "user_canceled",
            Self::RunCanceled => "run_canceled",
            Self::TaskCanceled => "task_canceled",
            Self::Superseded => "superseded",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::WorkerLost => "worker_lost",
            Self::AdmissionRevoked => "admission_revoked",
            Self::Other(s) => s.as_str(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ComputeBrokerError {
    #[error("admission rejected: {0}")]
    AdmissionRejected(String),
    #[error("stale task version")]
    StaleTaskVersion,
    #[error("attempt not active")]
    AttemptNotActive,
    #[error("placement decision absent or expired")]
    PlacementExpired,
    #[error("capability stale for worker {worker_id}: {detail}")]
    CapabilityStale { worker_id: String, detail: String },
    #[error("job kind {job_kind} runs on the local target; worker {worker_id} refused")]
    WorkerTargetRefused { job_kind: String, worker_id: String },
    #[error("reservation over budget: {0}")]
    OverBudget(String),
    #[error("workspace version mismatch")]
    WorkspaceMismatch,
    #[error("required artifacts unavailable")]
    ArtifactsUnavailable,
    #[error("data classification changed")]
    DataClassChanged,
    #[error("run or task canceled")]
    Canceled,
    #[error("RunSupervisor required: submit refused without supervisor")]
    SupervisorRequired,
    #[error("resource request exceeds hard policy limits")]
    ResourceTooLarge,
    #[error("unknown attempt")]
    UnknownAttempt,
    #[error("dispatch failed: {0}")]
    Dispatch(String),
    #[error("persistence failed: {0}")]
    Persist(String),
    #[error("{0}")]
    Other(String),
}
