//! Persist scheduler decisions without task content (M6-2).

use std::collections::HashMap;
use std::sync::Mutex;

use lokai_domain::ids::{AttemptId, RunId, TaskId, WorkerId};
use lokai_memory::SharedStore;

use crate::scheduler::types::{
    CandidateEstimate, ExecutionTargetId, SchedulerDecision, SchedulerDecisionId, SchedulerReason,
};

#[async_trait::async_trait]
pub trait SchedulerDecisionStore: Send + Sync {
    fn upsert(&self, decision: &SchedulerDecision) -> Result<(), String>;
    fn get(&self, id: &str) -> Result<Option<SchedulerDecision>, String>;
    fn list_pending(&self) -> Result<Vec<SchedulerDecision>, String>;

    async fn upsert_async(&self, decision: &SchedulerDecision) -> Result<(), String> {
        self.upsert(decision)
    }
    async fn get_async(&self, id: &str) -> Result<Option<SchedulerDecision>, String> {
        self.get(id)
    }
    async fn list_pending_async(&self) -> Result<Vec<SchedulerDecision>, String> {
        self.list_pending()
    }
}

#[derive(Default)]
pub struct InMemorySchedulerStore {
    inner: Mutex<HashMap<String, SchedulerDecision>>,
}

#[async_trait::async_trait]
impl SchedulerDecisionStore for InMemorySchedulerStore {
    fn upsert(&self, decision: &SchedulerDecision) -> Result<(), String> {
        self.inner
            .lock()
            .map_err(|e| e.to_string())?
            .insert(decision.decision_id.0.clone(), decision.clone());
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<SchedulerDecision>, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|e| e.to_string())?
            .get(id)
            .cloned())
    }

    fn list_pending(&self) -> Result<Vec<SchedulerDecision>, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .filter(|d| d.pending)
            .cloned()
            .collect())
    }
}

impl InMemorySchedulerStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, id: &SchedulerDecisionId) -> bool {
        self.inner
            .lock()
            .ok()
            .is_some_and(|g| g.contains_key(&id.0))
    }
}

/// Durable scheduler decisions in lokai.db (M6-2).
pub struct MemorySchedulerStore {
    store: SharedStore,
}

impl MemorySchedulerStore {
    pub fn new(store: SharedStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl SchedulerDecisionStore for MemorySchedulerStore {
    fn upsert(&self, decision: &SchedulerDecision) -> Result<(), String> {
        self.store
            .write_sync({
                let decision = decision.clone();
                move |db| {
                    db.upsert_scheduler_decision_row(
                        &decision.decision_id.0,
                        &decision.run_id.0,
                        &decision.task_id.0,
                        &decision.attempt_id.0,
                        &encode_target_opt(decision.selected_target.as_ref()),
                        &serde_json::to_string(&decision.fallback_order)
                            .unwrap_or_else(|_| "[]".into()),
                        decision.reason.as_str(),
                        &decision.model_version,
                        decision.expected_speedup.map(f64::from),
                        decision.uncertainty_margin_ms as i64,
                        &serde_json::to_string(&decision.candidates)
                            .unwrap_or_else(|_| "[]".into()),
                        &decision.decided_at.to_rfc3339(),
                        decision.pending,
                    )
                    .map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn upsert_async(&self, decision: &SchedulerDecision) -> Result<(), String> {
        self.store
            .write({
                let decision = decision.clone();
                move |db| {
                    db.upsert_scheduler_decision_row(
                        &decision.decision_id.0,
                        &decision.run_id.0,
                        &decision.task_id.0,
                        &decision.attempt_id.0,
                        &encode_target_opt(decision.selected_target.as_ref()),
                        &serde_json::to_string(&decision.fallback_order)
                            .unwrap_or_else(|_| "[]".into()),
                        decision.reason.as_str(),
                        &decision.model_version,
                        decision.expected_speedup.map(f64::from),
                        decision.uncertainty_margin_ms as i64,
                        &serde_json::to_string(&decision.candidates)
                            .unwrap_or_else(|_| "[]".into()),
                        &decision.decided_at.to_rfc3339(),
                        decision.pending,
                    )
                    .map_err(|e| e.to_string())
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn get(&self, id: &str) -> Result<Option<SchedulerDecision>, String> {
        self.store
            .read_sync({
                let id = id.to_string();
                move |db| {
                    Ok(db
                        .get_scheduler_decision_row(&id)
                        .map_err(|e| e.to_string())?
                        .map(decision_from_row))
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn get_async(&self, id: &str) -> Result<Option<SchedulerDecision>, String> {
        self.store
            .read({
                let id = id.to_string();
                move |db| {
                    Ok(db
                        .get_scheduler_decision_row(&id)
                        .map_err(|e| e.to_string())?
                        .map(decision_from_row))
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn list_pending(&self) -> Result<Vec<SchedulerDecision>, String> {
        self.store
            .read_sync(move |db| {
                Ok(db
                    .list_pending_scheduler_decision_rows()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(decision_from_row)
                    .collect())
            })
            .map_err(|e| e.to_string())?
    }

    async fn list_pending_async(&self) -> Result<Vec<SchedulerDecision>, String> {
        self.store
            .read(move |db| {
                Ok(db
                    .list_pending_scheduler_decision_rows()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(decision_from_row)
                    .collect())
            })
            .await
            .map_err(|e| e.to_string())?
    }
}

fn encode_target_opt(t: Option<&ExecutionTargetId>) -> String {
    match t {
        Some(ExecutionTargetId::Local) => "local".into(),
        Some(ExecutionTargetId::Worker { worker_id }) => format!("worker:{}", worker_id.0),
        None => String::new(),
    }
}

fn decode_target(raw: &str) -> Option<ExecutionTargetId> {
    if raw.is_empty() {
        None
    } else if raw == "local" {
        Some(ExecutionTargetId::Local)
    } else if let Some(id) = raw.strip_prefix("worker:") {
        Some(ExecutionTargetId::Worker {
            worker_id: WorkerId::new(id),
        })
    } else {
        Some(ExecutionTargetId::Worker {
            worker_id: WorkerId::new(raw),
        })
    }
}

fn decode_reason(raw: &str) -> SchedulerReason {
    match raw {
        "local_first_policy" => SchedulerReason::LocalFirstPolicy,
        "remote_faster" => SchedulerReason::RemoteFaster,
        "local_faster" => SchedulerReason::LocalFaster,
        "equal_with_uncertainty" => SchedulerReason::EqualWithUncertainty,
        "min_speedup_not_met" => SchedulerReason::MinSpeedupNotMet,
        "transfer_too_large" => SchedulerReason::TransferTooLarge,
        "circuit_open" => SchedulerReason::CircuitOpen,
        "quarantined" => SchedulerReason::Quarantined,
        "secret_local_only" => SchedulerReason::SecretLocalOnly,
        "no_eligible_remote" => SchedulerReason::NoEligibleRemote,
        "deadline_forced_local" => SchedulerReason::DeadlineForcedLocal,
        "queue_threshold" => SchedulerReason::QueueThreshold,
        "feature_flag_local_first" => SchedulerReason::FeatureFlagLocalFirst,
        _ => SchedulerReason::NoEligibleRemote,
    }
}

fn decision_from_row(r: lokai_memory::SchedulerDecisionRow) -> SchedulerDecision {
    let fallback_order: Vec<ExecutionTargetId> =
        serde_json::from_str(&r.fallback_order_json).unwrap_or_default();
    let candidates: Vec<CandidateEstimate> =
        serde_json::from_str(&r.candidates_json).unwrap_or_default();
    SchedulerDecision {
        decision_id: SchedulerDecisionId::new(r.decision_id),
        run_id: RunId::new(r.run_id),
        task_id: TaskId::new(r.task_id),
        attempt_id: AttemptId::new(r.attempt_id),
        candidates,
        selected_target: decode_target(&r.selected_target),
        fallback_order,
        uncertainty_margin_ms: r.uncertainty_margin_ms as u64,
        expected_speedup: r.expected_speedup.map(|v| v as f32),
        reason: decode_reason(&r.reason),
        model_version: r.model_version,
        decided_at: chrono::DateTime::parse_from_rfc3339(&r.decided_at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        pending: r.pending,
    }
}
