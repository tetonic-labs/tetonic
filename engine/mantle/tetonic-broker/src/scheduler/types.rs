//! Scheduler decision types (M6-2). No raw task content.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{AttemptId, RunId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchedulerDecisionId(pub String);

impl SchedulerDecisionId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// The scheduler's execution target is the domain seam type (M1). The broker no
/// longer defines its own copy.
pub use tetonic_domain::ExecutionTargetId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerReason {
    LocalFirstPolicy,
    RemoteFaster,
    LocalFaster,
    EqualWithUncertainty,
    MinSpeedupNotMet,
    TransferTooLarge,
    CircuitOpen,
    Quarantined,
    SecretLocalOnly,
    NoEligibleRemote,
    DeadlineForcedLocal,
    QueueThreshold,
    FeatureFlagLocalFirst,
}

impl SchedulerReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalFirstPolicy => "local_first_policy",
            Self::RemoteFaster => "remote_faster",
            Self::LocalFaster => "local_faster",
            Self::EqualWithUncertainty => "equal_with_uncertainty",
            Self::MinSpeedupNotMet => "min_speedup_not_met",
            Self::TransferTooLarge => "transfer_too_large",
            Self::CircuitOpen => "circuit_open",
            Self::Quarantined => "quarantined",
            Self::SecretLocalOnly => "secret_local_only",
            Self::NoEligibleRemote => "no_eligible_remote",
            Self::DeadlineForcedLocal => "deadline_forced_local",
            Self::QueueThreshold => "queue_threshold",
            Self::FeatureFlagLocalFirst => "feature_flag_local_first",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEstimate {
    pub target: ExecutionTargetId,
    pub estimated_finish_ms: u64,
    pub uncertainty_margin_ms: u64,
    pub queue_depth: u32,
    pub cold_start: bool,
    pub transfer_bytes: u64,
    pub score: u64,
}

fn default_pending() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerDecision {
    pub decision_id: SchedulerDecisionId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub candidates: Vec<CandidateEstimate>,
    pub selected_target: Option<ExecutionTargetId>,
    pub fallback_order: Vec<ExecutionTargetId>,
    pub uncertainty_margin_ms: u64,
    pub expected_speedup: Option<f32>,
    pub reason: SchedulerReason,
    pub model_version: String,
    pub decided_at: DateTime<Utc>,
    /// True until the attempt finishes or restart recovery clears it.
    #[serde(default = "default_pending")]
    pub pending: bool,
}
