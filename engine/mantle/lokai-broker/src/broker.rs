//! ComputeBroker — application-facing compute entry (M6-1 / M6-2 / M6-3).
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use lokai_domain::ids::{AttemptId, ReservationId, WorkerId};
use lokai_domain::sinks::{
    AuthorizedProcessRequest, ManagedProcessResult, ProcessBroker, ProcessBrokerError,
};
use lokai_domain::{ProjectPlacementPolicy, SpeculationConfig, WorkerTrust};
use lokai_fabric_protocol::{CapabilityRegistry, JobKind};
use lokai_inference::{ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink};
use lokai_run::RunSupervisor;

use crate::adapters::InferenceTargetAdapter;
use crate::admission::{
    AdmissionController, AdmissionDecision, AdmissionRequest, HierarchicalAdmissionController,
};
use crate::budget::{HierarchicalBudgetLedger, ReservationState, ReservedResources};
use crate::chat_request::compute_request_from_chat;
use crate::dispatch::revalidate_before_dispatch;
use crate::job_profile::profile_for;
use crate::metrics::{AdmissionMetrics, SchedulerMetrics};
use crate::persist::ReservationStore;
use crate::scheduler::failover::PendingWorkerLossContinue;
use crate::scheduler::fallback::{labels_to_targets, next_fallback, parse_target_label};
use crate::scheduler::{
    apply_scheduler_decision, schedule_infer_chat, CircuitBreakerRegistry, ExecutionTargetId,
    InMemorySchedulerStore, PredictionCalibration, ScheduleInferChat, SchedulerConfig,
    SchedulerDecision, SchedulerDecisionStore, SchedulerReason,
};
use crate::types::{
    CancellationReason, ComputeBrokerError, ComputeHandle, ComputeRequest, ComputeStatus,
};

#[async_trait]
pub trait ComputeBroker: Send + Sync {
    async fn submit(&self, request: ComputeRequest) -> Result<ComputeHandle, ComputeBrokerError>;

    async fn cancel(
        &self,
        attempt_id: AttemptId,
        reason: CancellationReason,
    ) -> Result<(), ComputeBrokerError>;

    async fn status(&self, attempt_id: AttemptId) -> Result<ComputeStatus, ComputeBrokerError>;
}

struct AttemptRecord {
    status: ComputeStatus,
    reservation_id: Option<ReservationId>,
    request: ComputeRequest,
}

/// Production broker: admission → reserve → dispatch (never mutates task state).
pub struct DefaultComputeBroker {
    admission: Arc<dyn AdmissionController>,
    budgets: Arc<HierarchicalBudgetLedger>,
    persist: Arc<dyn ReservationStore>,
    inference: Option<Arc<InferenceTargetAdapter>>,
    supervisor: RwLock<Option<Arc<dyn RunSupervisor>>>,
    capability_registry: RwLock<Option<Arc<RwLock<CapabilityRegistry>>>>,
    attempts: Mutex<HashMap<String, AttemptRecord>>,
    admission_metrics: AdmissionMetrics,
    scheduler_metrics: SchedulerMetrics,
    scheduler_store: RwLock<Arc<dyn SchedulerDecisionStore>>,
    circuits: Arc<CircuitBreakerRegistry>,
    scheduler_config: SchedulerConfig,
    prediction_calibration: PredictionCalibration,
    speculation_config: RwLock<SpeculationConfig>,
    /// Attempt id → in-flight chat (for worker-loss continue).
    inflight_chats: Mutex<HashMap<String, PendingWorkerLossContinue>>,
    /// Attempt ids that need `continue_after_worker_loss` after `on_worker_lost`.
    pending_continues: Mutex<HashMap<String, PendingWorkerLossContinue>>,
}

impl DefaultComputeBroker {
    pub fn new(
        admission: Arc<HierarchicalAdmissionController>,
        persist: Arc<dyn ReservationStore>,
        inference: Option<Arc<InferenceTargetAdapter>>,
        supervisor: Option<Arc<dyn RunSupervisor>>,
    ) -> Self {
        let budgets = admission.budgets().clone();
        Self {
            admission: admission as Arc<dyn AdmissionController>,
            budgets,
            persist,
            inference,
            supervisor: RwLock::new(supervisor),
            capability_registry: RwLock::new(None),
            attempts: Mutex::new(HashMap::new()),
            admission_metrics: AdmissionMetrics::default(),
            scheduler_metrics: SchedulerMetrics::default(),
            scheduler_store: RwLock::new(Arc::new(InMemorySchedulerStore::new())),
            circuits: Arc::new(CircuitBreakerRegistry::default()),
            scheduler_config: SchedulerConfig::default(),
            prediction_calibration: PredictionCalibration::new(),
            speculation_config: RwLock::new(SpeculationConfig::default()),
            inflight_chats: Mutex::new(HashMap::new()),
            pending_continues: Mutex::new(HashMap::new()),
        }
    }

    pub fn speculation_config(&self) -> SpeculationConfig {
        self.speculation_config
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn set_speculation_config(&self, config: SpeculationConfig) {
        if let Ok(mut g) = self.speculation_config.write() {
            *g = config;
        }
    }

    pub(crate) fn inflight_chats(&self) -> &Mutex<HashMap<String, PendingWorkerLossContinue>> {
        &self.inflight_chats
    }

    pub(crate) fn pending_continues(&self) -> &Mutex<HashMap<String, PendingWorkerLossContinue>> {
        &self.pending_continues
    }

    pub(crate) fn persist_store(&self) -> &Arc<dyn ReservationStore> {
        &self.persist
    }

    pub(crate) fn gate_dispatch_pub(
        &self,
        request: &ComputeRequest,
    ) -> Result<(), ComputeBrokerError> {
        self.gate_dispatch(request)
    }

    pub(crate) fn finish_attempt_pub(&self, attempt_id: &AttemptId, ok: bool, err: Option<String>) {
        self.finish_attempt(attempt_id, ok, err);
    }

    pub(crate) fn record_prediction_feedback_pub(
        &self,
        decision: &SchedulerDecision,
        actual_ms: u64,
    ) {
        self.record_prediction_feedback(decision, actual_ms);
    }

    pub fn admission_metrics(&self) -> &AdmissionMetrics {
        &self.admission_metrics
    }

    pub fn scheduler_metrics(&self) -> &SchedulerMetrics {
        &self.scheduler_metrics
    }

    pub fn circuits(&self) -> &Arc<CircuitBreakerRegistry> {
        &self.circuits
    }

    pub fn scheduler_store(&self) -> Arc<dyn SchedulerDecisionStore> {
        self.scheduler_store
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| Arc::new(InMemorySchedulerStore::new()))
    }

    pub fn set_scheduler_store(&self, store: Arc<dyn SchedulerDecisionStore>) {
        if let Ok(mut g) = self.scheduler_store.write() {
            *g = store;
        }
    }

    pub fn prediction_calibration(&self) -> &PredictionCalibration {
        &self.prediction_calibration
    }

    /// Late-bind RunSupervisor after Application bootstrap (daemon init).
    pub fn set_supervisor(&self, supervisor: Arc<dyn RunSupervisor>) {
        if let Ok(mut g) = self.supervisor.write() {
            *g = Some(supervisor);
        }
    }

    pub(crate) fn run_supervisor(&self) -> Option<Arc<dyn RunSupervisor>> {
        self.supervisor.read().ok().and_then(|g| g.clone())
    }

    /// Share the fabric capability cache so dispatch can revalidate advertisements.
    pub fn set_capability_registry(&self, registry: Arc<RwLock<CapabilityRegistry>>) {
        if let Ok(mut g) = self.capability_registry.write() {
            *g = Some(registry);
        }
    }

    /// Snapshot capability registry for hop placement revalidation.
    pub(crate) fn capability_registry_arc(&self) -> Option<Arc<RwLock<CapabilityRegistry>>> {
        self.capability_registry.read().ok().and_then(|g| g.clone())
    }

    /// Trust for a hop target from pooled remotes (unresolved ≠ owner).
    pub(crate) fn trust_for_hop_target(&self, target: &ExecutionTargetId) -> Option<WorkerTrust> {
        match target {
            ExecutionTargetId::Local => Some(WorkerTrust::LocalMachine),
            ExecutionTargetId::Worker { worker_id } => self
                .inference
                .as_ref()
                .and_then(|a| a.pooled().worker_trust_for(worker_id.0.as_str())),
        }
    }

    pub(crate) fn project_policy_for_hops(&self) -> ProjectPlacementPolicy {
        self.inference
            .as_ref()
            .map(|a| a.pooled().project_placement_policy())
            .unwrap_or_default()
    }

    /// Rebuild ledger from durable store after coordinator restart (fail-closed).
    pub fn recover_reservations(&self) -> Result<usize, ComputeBrokerError> {
        let active = self
            .persist
            .list_active()
            .map_err(ComputeBrokerError::Persist)?;
        let n = self.budgets.reconcile_uncertain(Utc::now());
        for r in active {
            let _ = self
                .persist
                .mark_released(&r.reservation_id.0, ReservationState::Canceled);
        }
        Ok(n)
    }

    /// Clear pending scheduler decisions after restart (attempts are gone).
    pub fn recover_scheduler_decisions(&self) -> Result<usize, ComputeBrokerError> {
        let store = self.scheduler_store();
        let pending = store.list_pending().map_err(ComputeBrokerError::Persist)?;
        let n = pending.len();
        for mut d in pending {
            d.pending = false;
            let _ = store.upsert(&d);
        }
        Ok(n)
    }

    /// Infer chat path: schedule → admit → revalidate → PooledProvider via adapter.
    pub async fn chat_admitted(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let started = std::time::Instant::now();
        lokai_telemetry::record_compute_stage(
            lokai_telemetry::span_names::COMPUTE_SUBMIT,
            Some("infer"),
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        );
        let adapter = self.inference.as_ref().ok_or_else(|| {
            InferenceError::Provider("compute broker has no inference adapter".into())
        })?;
        let mut compute_req = compute_request_from_chat(&req);
        compute_req.placement_decision.policy_epoch = adapter
            .pooled()
            .policy_epoch()
            .load(std::sync::atomic::Ordering::Relaxed);
        let schedule_timer = lokai_telemetry::StageTimer::start_visible(
            lokai_telemetry::PerfStage::InferenceSchedule,
        );
        let discovery_timer = lokai_telemetry::StageTimer::start_visible(
            lokai_telemetry::PerfStage::InferenceDiscovery,
        );
        let snap = adapter.fabric_snapshot().await;
        discovery_timer.finish(true);
        let pooled = adapter.pooled();
        let mut worker_trust_by_node = HashMap::new();
        for node in &snap.nodes {
            if let Some(trust) = pooled.worker_trust_for(&node.id) {
                worker_trust_by_node.insert(node.id.clone(), trust);
            }
        }
        let decision = {
            let caps_guard = self.capability_registry.read().ok().and_then(|g| g.clone());
            let caps_read = caps_guard.as_ref().and_then(|r| r.read().ok());
            schedule_infer_chat(ScheduleInferChat {
                compute_req: &compute_req,
                snap: &snap,
                chat: &req,
                circuits: &self.circuits,
                config: &self.scheduler_config,
                caps: caps_read.as_deref(),
                calibration: Some(&self.prediction_calibration),
                worker_trust_by_node,
                policy_epoch: compute_req.placement_decision.policy_epoch,
                project_policy: pooled.project_placement_policy(),
            })
        };
        self.record_scheduler_metrics(&decision);
        let persist_timer = lokai_telemetry::StageTimer::start_visible(
            lokai_telemetry::PerfStage::InferenceSchedulePersist,
        );
        let persisted = self.scheduler_store().upsert_async(&decision).await;
        persist_timer.finish(persisted.is_ok());
        apply_scheduler_decision(&mut compute_req, &decision);
        lokai_telemetry::record_compute_stage(
            lokai_telemetry::span_names::SCHEDULER_DECIDE,
            Some("infer"),
            None,
            Some(decision.reason.as_str()),
            Some(target_type_label(decision.selected_target.as_ref())),
            Some(decision.decision_id.0.as_str()),
            None,
            Some(started.elapsed().as_millis() as u64),
            compute_req.speculative,
        );

        schedule_timer.finish(true);
        let _ = adapter;
        let mut timing = lokai_telemetry::CoordinatorObservedTiming::start(
            if compute_req.trace_context.trace_id.is_empty() {
                format!("trace_{}", uuid::Uuid::new_v4())
            } else {
                compute_req.trace_context.trace_id.clone()
            },
            Some(decision.decision_id.0.clone()),
        );
        self.chat_with_failover(req, compute_req, &decision, on_token, started, &mut timing)
            .await
    }

    fn record_prediction_feedback(&self, decision: &SchedulerDecision, actual_ms: u64) {
        let Some(selected) = decision.selected_target.as_ref() else {
            return;
        };
        let predicted = decision
            .candidates
            .iter()
            .find(|c| &c.target == selected)
            .map(|c| c.estimated_finish_ms)
            .unwrap_or(0);
        let abs_err = actual_ms.abs_diff(predicted);
        self.scheduler_metrics.record_prediction_error(abs_err);
        let cold = decision
            .candidates
            .iter()
            .find(|c| &c.target == selected)
            .is_some_and(|c| c.cold_start);
        self.prediction_calibration
            .record_attempt(selected, cold, abs_err);
    }

    fn record_scheduler_metrics(&self, decision: &SchedulerDecision) {
        self.scheduler_metrics.record_decision();
        lokai_telemetry::emit_safe_metric("scheduler_reason", decision.reason.as_str());
        match decision.reason {
            SchedulerReason::RemoteFaster => self.scheduler_metrics.record_offload(),
            SchedulerReason::CircuitOpen => self.scheduler_metrics.record_circuit_open(),
            _ => self.scheduler_metrics.record_local_preferred(),
        }
        if let Some(d) = decision
            .candidates
            .iter()
            .map(|c| u64::from(c.queue_depth))
            .max()
        {
            self.scheduler_metrics.record_queue_depth(d);
        }
    }

    /// Admit + revalidate + execute a sandboxed IndexShard/TestShard (or similar) process job.
    pub async fn dispatch_sandboxed(
        &self,
        request: ComputeRequest,
        process: AuthorizedProcessRequest,
        executor: Arc<dyn ProcessBroker>,
    ) -> Result<ManagedProcessResult, ComputeBrokerError> {
        let profile = profile_for(&request.job_kind);
        if !profile.requires_sandbox {
            return Err(ComputeBrokerError::Other(format!(
                "job kind {:?} does not require sandbox process path",
                request.job_kind
            )));
        }
        if !matches!(request.job_kind, JobKind::IndexShard | JobKind::TestShard) {
            return Err(ComputeBrokerError::Other(
                "dispatch_sandboxed only supports IndexShard and TestShard".into(),
            ));
        }

        let handle = self.submit(request.clone()).await?;
        match handle.status {
            ComputeStatus::Reserved
            | ComputeStatus::Dispatched
            | ComputeStatus::Running
            | ComputeStatus::Succeeded => {}
            ComputeStatus::Queued => {
                return Err(ComputeBrokerError::AdmissionRejected(
                    "queued; retry".into(),
                ));
            }
            other => {
                return Err(ComputeBrokerError::AdmissionRejected(format!("{other:?}")));
            }
        }

        if let Err(e) = self.gate_dispatch(&request) {
            self.finish_attempt(&request.attempt_id, false, Some(e.to_string()));
            return Err(e);
        }

        if let Some(res) = self.budgets.get_by_attempt(&request.attempt_id) {
            let _ = self
                .budgets
                .transition(&res.reservation_id, ReservationState::Dispatched);
            let _ = self
                .budgets
                .transition(&res.reservation_id, ReservationState::Running);
            if let Some(updated) = self.budgets.get(&res.reservation_id) {
                let _ = self.persist.upsert_async(&updated).await;
            }
        }

        match executor.execute(process).await {
            Ok(result) => {
                self.finish_attempt(&request.attempt_id, result.success, None);
                Ok(result)
            }
            Err(ProcessBrokerError::Capability(e)) => {
                let msg = e.to_string();
                self.finish_attempt(&request.attempt_id, false, Some(msg.clone()));
                Err(ComputeBrokerError::Dispatch(msg))
            }
            Err(ProcessBrokerError::ExecutionFailed(e)) => {
                self.finish_attempt(&request.attempt_id, false, Some(e.clone()));
                Err(ComputeBrokerError::Dispatch(e))
            }
        }
    }

    /// Admit and track an IndexShard reservation around in-process local indexing.
    pub async fn with_index_shard_reservation<F, T>(
        &self,
        request: ComputeRequest,
        work: F,
    ) -> Result<T, ComputeBrokerError>
    where
        F: FnOnce() -> Result<T, String>,
    {
        if !matches!(request.job_kind, JobKind::IndexShard) {
            return Err(ComputeBrokerError::Other(
                "with_index_shard_reservation requires JobKind::IndexShard".into(),
            ));
        }
        let attempt_id = request.attempt_id.clone();
        let handle = self.submit(request.clone()).await?;
        if !matches!(handle.status, ComputeStatus::Reserved) {
            return Err(ComputeBrokerError::AdmissionRejected(format!(
                "{:?}",
                handle.status
            )));
        }
        if let Err(e) = self.gate_dispatch(&request) {
            self.finish_attempt(&attempt_id, false, Some(e.to_string()));
            return Err(e);
        }
        if let Some(res) = self.budgets.get_by_attempt(&attempt_id) {
            let _ = self
                .budgets
                .transition(&res.reservation_id, ReservationState::Running);
        }
        match work() {
            Ok(v) => {
                self.finish_attempt(&attempt_id, true, None);
                Ok(v)
            }
            Err(e) => {
                self.finish_attempt(&attempt_id, false, Some(e.clone()));
                Err(ComputeBrokerError::Dispatch(e))
            }
        }
    }

    /// Worker disappeared / revoked: release capacity so it cannot leak.
    ///
    /// Returns attempt ids that should call [`Self::continue_after_worker_loss`]
    /// (non-secret Infer with remaining fallback targets). In-flight
    /// `chat_admitted` also fails the remote hop and runs broker failover.
    pub fn on_worker_lost(&self, worker_id: &str) -> Vec<AttemptId> {
        let wid = WorkerId::new(worker_id);
        let _released = self
            .budgets
            .release_for_worker(&wid, ReservationState::Canceled);

        if let Some(adapter) = &self.inference {
            let reg = adapter.pooled().job_registry();
            for (job_id, _, _, _) in reg.dispatched_for_worker(worker_id) {
                reg.cancel_job(&job_id);
            }
        }

        let mut continue_ids = Vec::new();
        let failed = parse_target_label(worker_id);

        if let Ok(mut g) = self.attempts.lock() {
            for rec in g.values_mut() {
                if !rec
                    .request
                    .target_worker_id
                    .as_ref()
                    .is_some_and(|w| w.0 == worker_id)
                {
                    continue;
                }
                if let Some(rid) = &rec.reservation_id {
                    let _ = self
                        .persist
                        .mark_released(&rid.0, ReservationState::Canceled);
                }
                rec.status = ComputeStatus::Canceled {
                    reason: CancellationReason::WorkerLost.as_str().into(),
                };

                let secret = matches!(rec.request.data_class, lokai_domain::DataClass::Secret);
                let order = if rec.request.fallback_order.is_empty() {
                    Vec::new()
                } else {
                    labels_to_targets(&rec.request.fallback_order)
                };
                let has_next = !secret
                    && matches!(rec.request.job_kind, JobKind::Infer)
                    && next_fallback(&order, &failed).is_some();

                if has_next {
                    let attempt_id = rec.request.attempt_id.clone();
                    let chat = self
                        .inflight_chats
                        .lock()
                        .ok()
                        .and_then(|g| g.get(&attempt_id.0).map(|p| p.chat.clone()));
                    if let Some(chat) = chat {
                        let pending = PendingWorkerLossContinue {
                            compute_req: rec.request.clone(),
                            chat,
                            failed: failed.clone(),
                            order,
                        };
                        if let Ok(mut p) = self.pending_continues.lock() {
                            p.insert(attempt_id.0.clone(), pending);
                        }
                        continue_ids.push(attempt_id);
                    }
                }
            }
        }
        continue_ids
    }

    /// Measured use exceeded estimate — OverBudget and release.
    pub fn report_actual_usage(
        &self,
        attempt_id: &AttemptId,
        actual: &ReservedResources,
    ) -> Result<(), ComputeBrokerError> {
        match self.budgets.enforce_actual_usage(attempt_id, actual) {
            Ok(()) => Ok(()),
            Err(crate::budget::BudgetReject::OverBudget) => {
                if let Ok(mut g) = self.attempts.lock() {
                    if let Some(rec) = g.get_mut(&attempt_id.0) {
                        if let Some(rid) = &rec.reservation_id {
                            let _ = self
                                .persist
                                .mark_released(&rid.0, ReservationState::OverBudget);
                        }
                        rec.status = ComputeStatus::Failed {
                            reason: "over_budget".into(),
                        };
                    }
                }
                Err(ComputeBrokerError::OverBudget(
                    "actual resource use exceeded reservation".into(),
                ))
            }
            Err(e) => Err(ComputeBrokerError::Other(format!("{e:?}"))),
        }
    }

    fn gate_dispatch(&self, request: &ComputeRequest) -> Result<(), ComputeBrokerError> {
        let caps_guard = self.capability_registry.read().ok();
        let caps_arc = caps_guard.as_ref().and_then(|g| g.as_ref());
        let reg_read = caps_arc.and_then(|r| r.read().ok());
        revalidate_before_dispatch(request, reg_read.as_deref(), Utc::now())
    }

    fn finish_attempt(&self, attempt_id: &AttemptId, ok: bool, err: Option<String>) {
        if let Some(res) = self.budgets.get_by_attempt(attempt_id) {
            let terminal = if ok {
                ReservationState::Released
            } else {
                ReservationState::Canceled
            };
            lokai_telemetry::record_compute_stage(
                lokai_telemetry::span_names::RESOURCE_RELEASE,
                None,
                if ok {
                    Some("succeeded")
                } else {
                    Some("failed")
                },
                None,
                None,
                None,
                Some(res.reservation_id.0.as_str()),
                None,
                false,
            );
            self.budgets.release(&res.reservation_id, terminal);
            let _ = self.persist.mark_released(&res.reservation_id.0, terminal);
        }
        if let Ok(mut g) = self.attempts.lock() {
            if let Some(rec) = g.get_mut(&attempt_id.0) {
                rec.status = if ok {
                    ComputeStatus::Succeeded
                } else {
                    ComputeStatus::Failed {
                        reason: err.unwrap_or_else(|| "dispatch failed".into()),
                    }
                };
            }
        }
    }

    pub fn cancel_run_jobs(&self, run_id: &lokai_domain::ids::RunId) {
        let _ = self
            .budgets
            .release_for_run(run_id, ReservationState::Canceled);
        let attempt_ids: Vec<_> = {
            let Ok(g) = self.attempts.lock() else {
                return;
            };
            g.values()
                .filter(|rec| rec.request.run_id == *run_id)
                .map(|rec| rec.request.attempt_id.clone())
                .collect()
        };
        for aid in attempt_ids {
            self.budgets
                .release_attempt(&aid, ReservationState::Canceled);
        }
    }

    /// Leftover session INDEX. Each Infer cancel uses stored run/attempt ids.
    pub fn cancel_session_jobs(&self, session_id: &str) {
        if let Some(adapter) = &self.inference {
            adapter.cancel_session_jobs(session_id);
        }
    }

    pub fn inference_adapter(&self) -> Option<Arc<InferenceTargetAdapter>> {
        self.inference.clone()
    }

    pub fn budgets(&self) -> &Arc<HierarchicalBudgetLedger> {
        &self.budgets
    }
}

pub(crate) fn target_type_label(target: Option<&ExecutionTargetId>) -> &'static str {
    match target {
        Some(ExecutionTargetId::Local) | None => "local",
        Some(ExecutionTargetId::Worker { .. }) => "remote",
    }
}

#[async_trait]
impl ComputeBroker for DefaultComputeBroker {
    async fn submit(&self, request: ComputeRequest) -> Result<ComputeHandle, ComputeBrokerError> {
        validate_request(&request)?;
        let job_kind = format!("{:?}", request.job_kind).to_ascii_lowercase();
        lokai_telemetry::record_compute_stage(
            lokai_telemetry::span_names::ADMISSION_EVALUATE,
            Some(job_kind.as_str()),
            None,
            None,
            None,
            request.scheduler_decision_id.as_deref(),
            None,
            None,
            request.speculative,
        );

        let Some(supervisor) = self.supervisor.read().ok().and_then(|g| g.clone()) else {
            return Err(ComputeBrokerError::SupervisorRequired);
        };
        let (run_canceled, task_state, expected_version, attempt_active) =
            match supervisor.snapshot(request.run_id.clone()).await {
                Ok(snap) => {
                    let run_canceled = snap.cancellation.run_canceled;
                    let task = snap.tasks.get(&request.task_id);
                    let task_state = task.map(|t| t.state.clone());
                    let expected_version = snap
                        .attempts
                        .get(&request.attempt_id)
                        .map(|a| a.task_version);
                    let attempt_active = snap
                        .attempts
                        .get(&request.attempt_id)
                        .map(|a| {
                            !matches!(
                                a.state,
                                lokai_domain::AttemptState::Canceled
                                    | lokai_domain::AttemptState::Failed
                                    | lokai_domain::AttemptState::Succeeded
                            )
                        })
                        .unwrap_or(true);
                    (run_canceled, task_state, expected_version, attempt_active)
                }
                Err(_) => (false, None, None, true),
            };

        let decision = self
            .admission
            .evaluate(AdmissionRequest {
                compute: request.clone(),
                run_canceled,
                task_state,
                expected_task_version: expected_version,
                attempt_active,
                workspace_matches: true,
                artifacts_available: true,
                data_class_unchanged: true,
                now: Utc::now(),
            })
            .await;

        match decision {
            AdmissionDecision::Admitted(res) => {
                self.admission_metrics.record_admitted();
                lokai_telemetry::record_compute_stage(
                    lokai_telemetry::span_names::RESOURCE_RESERVE,
                    Some(job_kind.as_str()),
                    Some("admitted"),
                    None,
                    None,
                    request.scheduler_decision_id.as_deref(),
                    Some(res.reservation_id.0.as_str()),
                    None,
                    request.speculative,
                );
                let _ = self.persist.upsert(&res);
                let status = ComputeStatus::Reserved;
                self.attempts.lock().unwrap().insert(
                    request.attempt_id.0.clone(),
                    AttemptRecord {
                        status: status.clone(),
                        reservation_id: Some(res.reservation_id.clone()),
                        request,
                    },
                );
                Ok(ComputeHandle {
                    attempt_id: res.attempt_id,
                    reservation_id: Some(res.reservation_id),
                    status,
                })
            }
            AdmissionDecision::Queued(q) => {
                self.admission_metrics.record_queued();
                lokai_telemetry::record_compute_stage(
                    lokai_telemetry::span_names::ADMISSION_EVALUATE,
                    Some(job_kind.as_str()),
                    Some("queued"),
                    None,
                    None,
                    request.scheduler_decision_id.as_deref(),
                    None,
                    None,
                    request.speculative,
                );
                let status = ComputeStatus::Queued;
                self.attempts.lock().unwrap().insert(
                    request.attempt_id.0.clone(),
                    AttemptRecord {
                        status: status.clone(),
                        reservation_id: None,
                        request,
                    },
                );
                Ok(ComputeHandle {
                    attempt_id: q.attempt_id,
                    reservation_id: None,
                    status,
                })
            }
            AdmissionDecision::Rejected(r) => {
                self.admission_metrics.record_rejected(&r.reason);
                lokai_telemetry::record_compute_stage(
                    lokai_telemetry::span_names::ADMISSION_EVALUATE,
                    Some(job_kind.as_str()),
                    Some("rejected"),
                    None,
                    None,
                    request.scheduler_decision_id.as_deref(),
                    None,
                    None,
                    request.speculative,
                );
                let status = ComputeStatus::Rejected {
                    reason: format!("{}: {}", r.reason.as_str(), r.message),
                };
                self.attempts.lock().unwrap().insert(
                    request.attempt_id.0.clone(),
                    AttemptRecord {
                        status: status.clone(),
                        reservation_id: None,
                        request,
                    },
                );
                Err(ComputeBrokerError::AdmissionRejected(format!(
                    "{}: {}",
                    r.reason.as_str(),
                    r.message
                )))
            }
        }
    }

    async fn cancel(
        &self,
        attempt_id: AttemptId,
        reason: CancellationReason,
    ) -> Result<(), ComputeBrokerError> {
        let pair = {
            let g = self.attempts.lock().unwrap();
            g.get(&attempt_id.0)
                .map(|r| (r.request.run_id.clone(), r.request.task_id.clone()))
        };

        // Persist cancellation intent through RunSupervisor — broker does not mutate task state itself.
        let supervisor = self.supervisor.read().ok().and_then(|g| g.clone());
        if let (Some(sup), Some((run_id, task_id))) = (supervisor, pair) {
            let _ = lokai_run::cancel_hop(sup.as_ref(), run_id, task_id).await;
        }

        let reason_str = reason.as_str().to_string();
        self.budgets
            .release_attempt(&attempt_id, ReservationState::Canceled);
        if let Ok(mut g) = self.attempts.lock() {
            if let Some(rec) = g.get_mut(&attempt_id.0) {
                if let Some(rid) = &rec.reservation_id {
                    let _ = self
                        .persist
                        .mark_released(&rid.0, ReservationState::Canceled);
                }
                rec.status = ComputeStatus::Canceled { reason: reason_str };
            }
        }
        Ok(())
    }

    async fn status(&self, attempt_id: AttemptId) -> Result<ComputeStatus, ComputeBrokerError> {
        self.attempts
            .lock()
            .unwrap()
            .get(&attempt_id.0)
            .map(|r| r.status.clone())
            .ok_or(ComputeBrokerError::UnknownAttempt)
    }
}

fn validate_request(request: &ComputeRequest) -> Result<(), ComputeBrokerError> {
    if matches!(
        request.placement_decision.decision,
        lokai_domain::TrustPlacementDecision::Denied { .. }
    ) {
        return Err(ComputeBrokerError::PlacementExpired);
    }
    if request.placement_decision.expires_at <= Utc::now() {
        return Err(ComputeBrokerError::PlacementExpired);
    }
    let profile = profile_for(&request.job_kind);
    if request.speculative && !profile.supports_speculation {
        return Err(ComputeBrokerError::Other(
            "job kind does not support speculation".into(),
        ));
    }
    Ok(())
}
