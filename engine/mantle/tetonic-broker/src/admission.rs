//! Admission controller (M6-1).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{AttemptId, RunId, TaskId};
use tetonic_domain::{TaskState, TrustPlacementDecision};

use crate::budget::{
    BudgetReject, HierarchicalBudgetLedger, ReservationTarget, ResourceRequest, ResourceReservation,
};
use crate::priority::ComputePriority;
use crate::queue::{QueueAdmission, QueueManager};
use crate::types::{ComputeRequest, PlacementDecisionReference};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRejectionReason {
    ReservationConflict,
    RunCanceled,
    StaleTaskVersion,
    PlacementExpired,
    PlacementDenied,
    ResourceRequestTooLarge,
    RunConcurrencyLimit,
    WorkerConcurrencyLimit,
    MemoryBudgetExceeded,
    VramBudgetExceeded,
    ProcessLimitExceeded,
    TokenBudgetExceeded,
    SpeculationBudgetExceeded,
    QueueFull,
    DeadlineImpossible,
    SandboxUnavailable,
    NoEligibleTarget,
    AttemptNotActive,
    ArtifactsUnavailable,
    WorkspaceMismatch,
    DataClassChanged,
}

impl AdmissionRejectionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReservationConflict => "reservation_conflict",
            Self::RunCanceled => "run_canceled",
            Self::StaleTaskVersion => "stale_task_version",
            Self::PlacementExpired => "placement_expired",
            Self::PlacementDenied => "placement_denied",
            Self::ResourceRequestTooLarge => "resource_request_too_large",
            Self::RunConcurrencyLimit => "run_concurrency_limit",
            Self::WorkerConcurrencyLimit => "worker_concurrency_limit",
            Self::MemoryBudgetExceeded => "memory_budget_exceeded",
            Self::VramBudgetExceeded => "vram_budget_exceeded",
            Self::ProcessLimitExceeded => "process_limit_exceeded",
            Self::TokenBudgetExceeded => "token_budget_exceeded",
            Self::SpeculationBudgetExceeded => "speculation_budget_exceeded",
            Self::QueueFull => "queue_full",
            Self::DeadlineImpossible => "deadline_impossible",
            Self::SandboxUnavailable => "sandbox_unavailable",
            Self::NoEligibleTarget => "no_eligible_target",
            Self::AttemptNotActive => "attempt_not_active",
            Self::ArtifactsUnavailable => "artifacts_unavailable",
            Self::WorkspaceMismatch => "workspace_mismatch",
            Self::DataClassChanged => "data_class_changed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdmissionRejection {
    pub reason: AdmissionRejectionReason,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum AdmissionDecision {
    Admitted(ResourceReservation),
    Queued(QueueAdmission),
    Rejected(AdmissionRejection),
}

#[derive(Debug, Clone)]
pub struct AdmissionRequest {
    pub compute: ComputeRequest,
    pub run_canceled: bool,
    pub task_state: Option<TaskState>,
    pub expected_task_version: Option<u64>,
    pub attempt_active: bool,
    pub workspace_matches: bool,
    pub artifacts_available: bool,
    pub data_class_unchanged: bool,
    pub now: DateTime<Utc>,
}

#[async_trait]
pub trait AdmissionController: Send + Sync {
    async fn evaluate(&self, request: AdmissionRequest) -> AdmissionDecision;
}

pub struct HierarchicalAdmissionController {
    budgets: std::sync::Arc<HierarchicalBudgetLedger>,
    queue: std::sync::Arc<QueueManager>,
}

impl HierarchicalAdmissionController {
    pub fn new(
        budgets: std::sync::Arc<HierarchicalBudgetLedger>,
        queue: std::sync::Arc<QueueManager>,
    ) -> Self {
        Self { budgets, queue }
    }

    pub fn budgets(&self) -> &std::sync::Arc<HierarchicalBudgetLedger> {
        &self.budgets
    }

    pub fn queue(&self) -> &std::sync::Arc<QueueManager> {
        &self.queue
    }
}

#[async_trait]
impl AdmissionController for HierarchicalAdmissionController {
    async fn evaluate(&self, request: AdmissionRequest) -> AdmissionDecision {
        let outcome = evaluate_sync(&self.budgets, &self.queue, request);
        match &outcome {
            AdmissionDecision::Admitted(r) => {
                tracing::info!(
                    target: "lokai_admission",
                    attempt_id = %r.attempt_id,
                    reservation_id = %r.reservation_id,
                    "admitted"
                );
            }
            AdmissionDecision::Queued(q) => {
                tracing::info!(
                    target: "lokai_admission",
                    attempt_id = %q.attempt_id,
                    position = q.position,
                    queue_depth = self.queue.depth(),
                    "queued"
                );
                tetonic_telemetry::record_compute_stage(
                    tetonic_telemetry::span_names::QUEUE_WAIT,
                    None,
                    Some("queued"),
                    None,
                    None,
                    None,
                    None,
                    Some(self.queue.depth() as u64),
                    false,
                );
            }
            AdmissionDecision::Rejected(r) => {
                tracing::info!(
                    target: "lokai_admission",
                    reason = r.reason.as_str(),
                    message = %r.message,
                    "rejected"
                );
            }
        }
        outcome
    }
}

fn evaluate_sync(
    budgets: &HierarchicalBudgetLedger,
    queue: &QueueManager,
    request: AdmissionRequest,
) -> AdmissionDecision {
    let c = &request.compute;
    if request.run_canceled {
        return reject(AdmissionRejectionReason::RunCanceled, "run canceled");
    }
    if matches!(request.task_state, Some(TaskState::Canceled)) {
        return reject(AdmissionRejectionReason::RunCanceled, "task canceled");
    }
    if !request.attempt_active {
        return reject(
            AdmissionRejectionReason::AttemptNotActive,
            "attempt is not active",
        );
    }
    if let Some(expected) = request.expected_task_version {
        if expected != c.task_version {
            return reject(
                AdmissionRejectionReason::StaleTaskVersion,
                "task version stale",
            );
        }
    }
    if !request.workspace_matches {
        return reject(
            AdmissionRejectionReason::WorkspaceMismatch,
            "workspace version mismatch",
        );
    }
    if !request.artifacts_available {
        return reject(
            AdmissionRejectionReason::ArtifactsUnavailable,
            "required artifacts unavailable",
        );
    }
    if !request.data_class_unchanged {
        return reject(
            AdmissionRejectionReason::DataClassChanged,
            "data classification changed",
        );
    }
    if let Some(rej) = validate_placement(&c.placement_decision, request.now) {
        return rej;
    }
    if c.deadline.queue_deadline <= request.now {
        return reject(
            AdmissionRejectionReason::DeadlineImpossible,
            "queue deadline already passed",
        );
    }
    if c.resource_request.exceeds_hard_limits(&budgets.limits()) {
        return reject(
            AdmissionRejectionReason::ResourceRequestTooLarge,
            "resource request exceeds hard limits",
        );
    }

    let target = resolve_target(c);
    match budgets.reserve(
        &c.run_id,
        &c.task_id,
        &c.attempt_id,
        c.project_id.as_deref(),
        target,
        &c.resource_request,
        c.speculative,
        request.now,
    ) {
        Ok(reservation) => AdmissionDecision::Admitted(reservation),
        Err(err @ BudgetReject::RunConcurrencyLimit)
        | Err(err @ BudgetReject::WorkerConcurrencyLimit)
        | Err(err @ BudgetReject::MemoryBudgetExceeded)
        | Err(err @ BudgetReject::VramBudgetExceeded)
        | Err(err @ BudgetReject::ProcessLimitExceeded)
        | Err(err @ BudgetReject::TokenBudgetExceeded)
        | Err(err @ BudgetReject::SpeculationBudgetExceeded) => {
            let reason = budget_to_reason(err);
            match queue.enqueue(c, request.now) {
                Ok(qa) => AdmissionDecision::Queued(qa),
                Err(_) => reject(reason, "budgets exhausted and queue full"),
            }
        }
        Err(BudgetReject::ReservationConflict) => reject(
            AdmissionRejectionReason::ReservationConflict,
            "attempt already has a different reservation",
        ),
        Err(BudgetReject::ResourceRequestTooLarge) => reject(
            AdmissionRejectionReason::ResourceRequestTooLarge,
            "resource request too large",
        ),
        Err(BudgetReject::UnknownReservation) => reject(
            AdmissionRejectionReason::NoEligibleTarget,
            "reservation bookkeeping error",
        ),
        Err(BudgetReject::OverBudget) => reject(
            AdmissionRejectionReason::ResourceRequestTooLarge,
            "reservation over budget",
        ),
    }
}

fn budget_to_reason(err: BudgetReject) -> AdmissionRejectionReason {
    match err {
        BudgetReject::ReservationConflict => AdmissionRejectionReason::ReservationConflict,
        BudgetReject::ResourceRequestTooLarge => AdmissionRejectionReason::ResourceRequestTooLarge,
        BudgetReject::RunConcurrencyLimit => AdmissionRejectionReason::RunConcurrencyLimit,
        BudgetReject::WorkerConcurrencyLimit => AdmissionRejectionReason::WorkerConcurrencyLimit,
        BudgetReject::MemoryBudgetExceeded => AdmissionRejectionReason::MemoryBudgetExceeded,
        BudgetReject::VramBudgetExceeded => AdmissionRejectionReason::VramBudgetExceeded,
        BudgetReject::ProcessLimitExceeded => AdmissionRejectionReason::ProcessLimitExceeded,
        BudgetReject::TokenBudgetExceeded => AdmissionRejectionReason::TokenBudgetExceeded,
        BudgetReject::SpeculationBudgetExceeded => {
            AdmissionRejectionReason::SpeculationBudgetExceeded
        }
        BudgetReject::UnknownReservation => AdmissionRejectionReason::NoEligibleTarget,
        BudgetReject::OverBudget => AdmissionRejectionReason::ResourceRequestTooLarge,
    }
}

fn validate_placement(
    placement: &PlacementDecisionReference,
    now: DateTime<Utc>,
) -> Option<AdmissionDecision> {
    if placement.expires_at <= now {
        return Some(reject(
            AdmissionRejectionReason::PlacementExpired,
            "placement decision expired",
        ));
    }
    match &placement.decision {
        TrustPlacementDecision::Denied { .. } => Some(reject(
            AdmissionRejectionReason::PlacementDenied,
            "placement denied",
        )),
        TrustPlacementDecision::LocalOnly { .. }
        | TrustPlacementDecision::Eligible { .. }
        | TrustPlacementDecision::EligibleAfterRedaction { .. } => None,
    }
}

fn resolve_target(c: &ComputeRequest) -> ReservationTarget {
    if let Some(w) = &c.target_worker_id {
        return ReservationTarget::Worker {
            worker_id: w.clone(),
        };
    }
    let remote = match &c.placement_decision.decision {
        TrustPlacementDecision::Eligible { targets, .. }
        | TrustPlacementDecision::EligibleAfterRedaction { targets, .. } => targets.first(),
        _ => None,
    };
    match remote {
        Some(t) => ReservationTarget::Worker {
            worker_id: t.worker_id.clone(),
        },
        None => ReservationTarget::Local,
    }
}

fn reject(reason: AdmissionRejectionReason, message: impl Into<String>) -> AdmissionDecision {
    AdmissionDecision::Rejected(AdmissionRejection {
        reason,
        message: message.into(),
    })
}

/// Re-evaluate a queued item when capacity frees.
#[allow(clippy::too_many_arguments)]
pub fn try_admit_queued(
    budgets: &HierarchicalBudgetLedger,
    queue: &QueueManager,
    run_id: &RunId,
    task_id: &TaskId,
    attempt_id: &AttemptId,
    project_id: Option<&str>,
    target: ReservationTarget,
    request: &ResourceRequest,
    speculative: bool,
    priority: ComputePriority,
    now: DateTime<Utc>,
) -> AdmissionDecision {
    let _ = priority;
    match budgets.reserve(
        run_id,
        task_id,
        attempt_id,
        project_id,
        target,
        request,
        speculative,
        now,
    ) {
        Ok(r) => {
            queue.remove(attempt_id);
            AdmissionDecision::Admitted(r)
        }
        Err(e) => reject(budget_to_reason(e), "queued reservation denied"),
    }
}
