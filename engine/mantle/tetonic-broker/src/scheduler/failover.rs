//! Broker-driven Infer failover, worker-loss continue, and speculation race (M6-2).

use std::sync::{Arc, Mutex};

use tetonic_domain::ids::AttemptId;
use tetonic_domain::DataClass;
use tetonic_fabric_protocol::JobKind;
use tetonic_inference::{
    ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink, LOCAL_NODE_ID,
};

use crate::broker::{ComputeBroker, DefaultComputeBroker};
use crate::budget::ReservationState;
use crate::scheduler::attempt_lease::HopAttemptMode;
use crate::scheduler::fallback::{
    classify_inference_error, fallback_action, next_fallback, FallbackAction, FallbackFailureClass,
};
use crate::scheduler::hop_placement::{revalidate_hop_placement, HopPlacementOutcome};
use crate::scheduler::speculate::{speculation_allowed, speculative_race_sessions};
use crate::scheduler::stamp::stamp_fabric_single_target;
use crate::scheduler::types::{ExecutionTargetId, SchedulerDecision};
use crate::types::{CancellationReason, ComputeRequest, ComputeStatus};
use tetonic_telemetry::{
    critical_path_ms, format_critical_path_report, record_stage_segments, CoordinatorObservedTiming,
};

/// In-flight Infer that can continue after worker loss.
#[derive(Debug, Clone)]
pub struct PendingWorkerLossContinue {
    pub compute_req: ComputeRequest,
    pub chat: ChatRequest,
    pub failed: ExecutionTargetId,
    pub order: Vec<ExecutionTargetId>,
}

impl DefaultComputeBroker {
    /// Execute Infer with per-target re-admit failover (and optional speculation race).
    pub(crate) async fn chat_with_failover(
        &self,
        mut req: ChatRequest,
        mut compute_req: ComputeRequest,
        decision: &SchedulerDecision,
        on_token: &mut TokenSink<'_>,
        started: std::time::Instant,
        timing: &mut CoordinatorObservedTiming,
    ) -> Result<ChatResponse, InferenceError> {
        let is_secret = matches!(compute_req.data_class, DataClass::Secret);
        let mut targets: Vec<ExecutionTargetId> = if decision.fallback_order.is_empty() {
            decision.selected_target.clone().into_iter().collect()
        } else {
            decision.fallback_order.clone()
        };
        if is_secret {
            targets = vec![ExecutionTargetId::Local];
        }
        if targets.is_empty() {
            targets.push(ExecutionTargetId::Local);
        }

        // Speculative race when config allows and ≥2 targets. Prefer launching
        // when `should_speculate_for_tail` is true; still race whenever
        // speculation is explicitly allowed (estate opt-in / tests).
        if !is_secret && targets.len() >= 2 {
            match speculation_allowed(&JobKind::Infer, &self.speculation_config()) {
                Ok(()) => {
                    if let Some(resp) = self
                        .race_speculative_infer(
                            req.clone(),
                            compute_req.clone(),
                            decision,
                            targets[0].clone(),
                            targets[1].clone(),
                            on_token,
                            started,
                            timing,
                        )
                        .await?
                    {
                        self.finalize_decision(decision, started, timing, None);
                        return Ok(resp);
                    }
                }
                Err(_) => {
                    self.scheduler_metrics().record_speculation_denied();
                }
            }
        }

        let mut last_err: Option<InferenceError> = None;
        let mut prev_failed: Option<ExecutionTargetId> = None;
        let mut previous_attempt: Option<AttemptId> = Some(compute_req.attempt_id.clone());

        for (i, target) in targets.iter().enumerate() {
            if i > 0 {
                let err = last_err
                    .as_ref()
                    .expect("fallback hop requires prior error");
                let class = classify_inference_error(err);
                if matches!(fallback_action(class), FallbackAction::FailExplicit)
                    || matches!(class, FallbackFailureClass::PermanentPolicy)
                {
                    break;
                }
                let _ = prev_failed
                    .as_ref()
                    .and_then(|f| next_fallback(&targets, f));
                self.scheduler_metrics().record_fallback();
                compute_req.speculative = false;
            }

            let mode = if i == 0 {
                HopAttemptMode::EnsureExisting
            } else {
                HopAttemptMode::Failover {
                    previous: previous_attempt
                        .clone()
                        .unwrap_or_else(|| compute_req.attempt_id.clone()),
                }
            };
            if let Err(e) = self
                .bind_hop_attempt(&mut compute_req, &mut req, target, mode)
                .await
            {
                last_err = Some(e);
                prev_failed = Some(target.clone());
                if is_secret {
                    break;
                }
                continue;
            }
            previous_attempt = Some(compute_req.attempt_id.clone());

            {
                let caps_arc = self.capability_registry_arc();
                let caps_guard = caps_arc.as_ref().and_then(|r| r.read().ok());
                let Some(trust) = self.trust_for_hop_target(target) else {
                    prev_failed = Some(target.clone());
                    last_err = Some(InferenceError::Provider(
                        "failover hop ineligible: worker_trust_missing".into(),
                    ));
                    if is_secret {
                        break;
                    }
                    continue;
                };
                match revalidate_hop_placement(
                    &mut compute_req,
                    &req,
                    target,
                    caps_guard.as_deref(),
                    trust,
                    self.project_policy_for_hops(),
                ) {
                    HopPlacementOutcome::Allowed => {}
                    HopPlacementOutcome::Skip { reason } => {
                        prev_failed = Some(target.clone());
                        last_err = Some(InferenceError::Provider(format!(
                            "failover hop ineligible: {}",
                            reason.code()
                        )));
                        if is_secret {
                            break;
                        }
                        continue;
                    }
                }
            }
            stamp_fabric_single_target(&mut req, target, Some(decision.decision_id.0.as_str()));

            self.track_inflight_chat(&compute_req.attempt_id, &req, &compute_req, &targets);

            match self
                .admit_gate_chat_once(
                    &mut compute_req,
                    req.clone(),
                    decision,
                    on_token,
                    started,
                    timing,
                )
                .await
            {
                Ok(resp) => {
                    self.clear_inflight_chat(&compute_req.attempt_id);
                    self.record_remote_circuit(&compute_req, true);
                    self.finalize_decision(decision, started, timing, None);
                    self.finish_attempt_pub(&compute_req.attempt_id, true, None);
                    return Ok(resp);
                }
                Err(e) => {
                    self.clear_inflight_chat(&compute_req.attempt_id);
                    self.record_remote_circuit(&compute_req, false);
                    self.finish_attempt_pub(&compute_req.attempt_id, false, Some(e.to_string()));
                    prev_failed = Some(target.clone());
                    last_err = Some(e);
                    if is_secret {
                        break;
                    }
                }
            }
        }

        self.finalize_decision(decision, started, timing, None);
        Err(last_err.unwrap_or_else(|| {
            InferenceError::Provider("infer failover exhausted with no error".into())
        }))
    }

    /// Continue a non-secret Infer after worker loss when fallback targets remain.
    pub async fn continue_after_worker_loss(
        &self,
        attempt_id: &AttemptId,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let pending = {
            let mut g = self
                .pending_continues()
                .lock()
                .map_err(|_| InferenceError::Provider("pending continue lock".into()))?;
            g.remove(&attempt_id.0)
        };
        let Some(pending) = pending else {
            return Err(InferenceError::Provider(
                "no pending worker-loss continue for attempt".into(),
            ));
        };
        if matches!(pending.compute_req.data_class, DataClass::Secret) {
            return Err(InferenceError::Provider(
                "secret jobs never failover to remote after worker loss".into(),
            ));
        }
        let Some(next) = next_fallback(&pending.order, &pending.failed) else {
            return Err(InferenceError::Provider(
                "no eligible fallback after worker loss".into(),
            ));
        };

        self.scheduler_metrics().record_fallback();
        let mut compute_req = pending.compute_req;
        let mut req = pending.chat;
        {
            let previous = compute_req.attempt_id.clone();
            self.bind_hop_attempt(
                &mut compute_req,
                &mut req,
                &next,
                HopAttemptMode::Failover { previous },
            )
            .await?;
        }
        let caps_arc = self.capability_registry_arc();
        let caps_guard = caps_arc.as_ref().and_then(|r| r.read().ok());
        let Some(trust) = self.trust_for_hop_target(&next) else {
            return Err(InferenceError::Provider(
                "failover hop ineligible after worker loss: worker_trust_missing".into(),
            ));
        };
        match revalidate_hop_placement(
            &mut compute_req,
            &req,
            &next,
            caps_guard.as_deref(),
            trust,
            self.project_policy_for_hops(),
        ) {
            HopPlacementOutcome::Allowed => {}
            HopPlacementOutcome::Skip { reason } => {
                return Err(InferenceError::Provider(format!(
                    "failover hop ineligible after worker loss: {}",
                    reason.code()
                )));
            }
        }
        let decision_id = compute_req.scheduler_decision_id.clone();
        stamp_fabric_single_target(&mut req, &next, decision_id.as_deref());

        // Remaining order after the hop we are about to try.
        let mut remaining = pending.order.clone();
        if let Some(pos) = remaining.iter().position(|t| t == &pending.failed) {
            remaining = remaining.split_off(pos + 1);
        }

        // Build a synthetic decision for telemetry/finalize.
        let decision = SchedulerDecision {
            decision_id: crate::scheduler::types::SchedulerDecisionId::new(
                decision_id.unwrap_or_else(|| format!("sched_{}", uuid::Uuid::new_v4())),
            ),
            run_id: compute_req.run_id.clone(),
            task_id: compute_req.task_id.clone(),
            attempt_id: compute_req.attempt_id.clone(),
            candidates: vec![],
            selected_target: Some(next.clone()),
            fallback_order: remaining,
            uncertainty_margin_ms: 0,
            expected_speedup: None,
            reason: crate::scheduler::types::SchedulerReason::NoEligibleRemote,
            model_version: crate::scheduler::score::SCHEDULER_MODEL_VERSION.into(),
            decided_at: chrono::Utc::now(),
            pending: true,
        };

        let started = std::time::Instant::now();
        let trace_id = if compute_req.trace_context.trace_id.is_empty() {
            format!("trace_{}", uuid::Uuid::new_v4())
        } else {
            compute_req.trace_context.trace_id.clone()
        };
        let mut timing =
            CoordinatorObservedTiming::start(trace_id, compute_req.scheduler_decision_id.clone());
        self.chat_with_failover(req, compute_req, &decision, on_token, started, &mut timing)
            .await
    }

    async fn admit_gate_chat_once(
        &self,
        compute_req: &mut ComputeRequest,
        req: ChatRequest,
        decision: &SchedulerDecision,
        on_token: &mut TokenSink<'_>,
        started: std::time::Instant,
        timing: &mut CoordinatorObservedTiming,
    ) -> Result<ChatResponse, InferenceError> {
        let adapter = self.inference_adapter().ok_or_else(|| {
            InferenceError::Provider("compute broker has no inference adapter".into())
        })?;

        let handle = self
            .submit(compute_req.clone())
            .await
            .map_err(|e| InferenceError::Provider(e.to_string()))?;
        match handle.status {
            ComputeStatus::Queued => {
                return Err(InferenceError::Provider(
                    "compute admission queued; retry".into(),
                ));
            }
            ComputeStatus::Rejected { reason } => {
                return Err(InferenceError::Provider(format!(
                    "compute admission rejected: {reason}"
                )));
            }
            ComputeStatus::Canceled { reason } => {
                return Err(InferenceError::Provider(format!(
                    "compute canceled: {reason}"
                )));
            }
            ComputeStatus::Failed { reason } => {
                return Err(InferenceError::Provider(reason));
            }
            ComputeStatus::Reserved
            | ComputeStatus::Dispatched
            | ComputeStatus::Running
            | ComputeStatus::Succeeded => {
                if timing.admitted_at.is_none() {
                    timing.mark_admitted();
                }
            }
        }

        self.gate_dispatch_pub(compute_req)
            .map_err(|e| InferenceError::Provider(e.to_string()))?;

        if let Some(res) = self.budgets().get_by_attempt(&compute_req.attempt_id) {
            tetonic_telemetry::record_compute_stage(
                tetonic_telemetry::span_names::RESOURCE_RESERVE,
                Some("infer"),
                Some("admitted"),
                None,
                Some(crate::broker::target_type_label(
                    decision.selected_target.as_ref(),
                )),
                Some(decision.decision_id.0.as_str()),
                Some(res.reservation_id.0.as_str()),
                Some(started.elapsed().as_millis() as u64),
                compute_req.speculative,
            );
            let _ = self
                .budgets()
                .transition(&res.reservation_id, ReservationState::Dispatched);
            if let Some(updated) = self.budgets().get(&res.reservation_id) {
                let _ = self.persist_store().upsert_async(&updated).await;
            }
            let _ = self
                .budgets()
                .transition(&res.reservation_id, ReservationState::Running);
        }

        timing.mark_dispatched();
        let transfer_start = std::time::Instant::now();
        let result = adapter.chat(req, on_token).await;
        let transfer_ms = transfer_start.elapsed().as_millis() as u64;
        if let Ok(ref resp) = result {
            if let Some(t) = resp.provenance.transfer_ms {
                timing.transfer.recv_ms = Some(t);
            } else {
                timing.transfer.recv_ms = Some(transfer_ms);
            }
            timing.verification_observed_ms = resp.provenance.verification_ms;
            if resp.provenance.worker_execute_ms.is_some()
                || resp.provenance.worker_queue_ms.is_some()
            {
                timing.attach_worker(tetonic_telemetry::WorkerLocalDurations {
                    queue_ms: resp.provenance.worker_queue_ms,
                    execute_ms: resp.provenance.worker_execute_ms,
                    ..Default::default()
                });
            }
            timing.mark_result_received();
            timing.mark_accepted();
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn race_speculative_infer(
        &self,
        req: ChatRequest,
        base_compute: ComputeRequest,
        decision: &SchedulerDecision,
        primary: ExecutionTargetId,
        speculative_target: ExecutionTargetId,
        on_token: &mut TokenSink<'_>,
        _started: std::time::Instant,
        timing: &mut CoordinatorObservedTiming,
    ) -> Result<Option<ChatResponse>, InferenceError> {
        let cfg = self.speculation_config();
        if cfg.max_simultaneous_attempts < 2 {
            return Ok(None);
        }

        let mut primary_compute = base_compute.clone();
        primary_compute.speculative = false;
        {
            let caps_arc = self.capability_registry_arc();
            let caps_guard = caps_arc.as_ref().and_then(|r| r.read().ok());
            let Some(trust) = self.trust_for_hop_target(&primary) else {
                return Ok(None);
            };
            if matches!(
                revalidate_hop_placement(
                    &mut primary_compute,
                    &req,
                    &primary,
                    caps_guard.as_deref(),
                    trust,
                    self.project_policy_for_hops(),
                ),
                HopPlacementOutcome::Skip { .. }
            ) {
                return Ok(None);
            }
        }
        let mut primary_req = req.clone();
        stamp_fabric_single_target(
            &mut primary_req,
            &primary,
            Some(decision.decision_id.0.as_str()),
        );
        if self
            .bind_hop_attempt(
                &mut primary_compute,
                &mut primary_req,
                &primary,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .is_err()
        {
            return Ok(None);
        }
        let base_session = primary_req
            .fabric
            .as_ref()
            .and_then(|f| f.session_id.clone())
            .unwrap_or_else(|| format!("sess_{}", uuid::Uuid::new_v4()));
        let (primary_session, spec_session) = speculative_race_sessions(&base_session);
        if let Some(meta) = primary_req.fabric.as_mut() {
            meta.session_id = Some(primary_session.clone());
        }

        let mut spec_compute = base_compute;
        spec_compute.speculative = true;
        let mut spec_req = req;
        stamp_fabric_single_target(
            &mut spec_req,
            &speculative_target,
            Some(decision.decision_id.0.as_str()),
        );
        if self
            .bind_hop_attempt(
                &mut spec_compute,
                &mut spec_req,
                &speculative_target,
                HopAttemptMode::Additional,
            )
            .await
            .is_err()
        {
            // Cannot mint an unleased speculative id — skip the race.
            return Ok(None);
        }
        {
            let caps_arc = self.capability_registry_arc();
            let caps_guard = caps_arc.as_ref().and_then(|r| r.read().ok());
            let Some(trust) = self.trust_for_hop_target(&speculative_target) else {
                return Ok(None);
            };
            if matches!(
                revalidate_hop_placement(
                    &mut spec_compute,
                    &spec_req,
                    &speculative_target,
                    caps_guard.as_deref(),
                    trust,
                    self.project_policy_for_hops(),
                ),
                HopPlacementOutcome::Skip { .. }
            ) {
                return Ok(None);
            }
        }
        if let Some(meta) = spec_req.fabric.as_mut() {
            meta.session_id = Some(spec_session.clone());
            meta.attempt_id = Some(spec_compute.attempt_id.0.clone());
        }
        if timing.admitted_at.is_none() {
            timing.mark_admitted();
        }
        timing.mark_dispatched();

        let primary_handle = self.submit(primary_compute.clone()).await;
        let spec_handle = self.submit(spec_compute.clone()).await;
        let admitted = |h: &Result<crate::types::ComputeHandle, _>| {
            h.as_ref().ok().is_some_and(|h| {
                matches!(
                    h.status,
                    ComputeStatus::Reserved
                        | ComputeStatus::Dispatched
                        | ComputeStatus::Running
                        | ComputeStatus::Succeeded
                )
            })
        };
        if !admitted(&primary_handle) || !admitted(&spec_handle) {
            if admitted(&primary_handle) {
                self.finish_attempt_pub(
                    &primary_compute.attempt_id,
                    false,
                    Some("speculation aborted".into()),
                );
            }
            if admitted(&spec_handle) {
                self.finish_attempt_pub(
                    &spec_compute.attempt_id,
                    false,
                    Some("speculation aborted".into()),
                );
            }
            return Ok(None);
        }
        if self.gate_dispatch_pub(&primary_compute).is_err()
            || self.gate_dispatch_pub(&spec_compute).is_err()
        {
            self.finish_attempt_pub(
                &primary_compute.attempt_id,
                false,
                Some("speculation gate failed".into()),
            );
            self.finish_attempt_pub(
                &spec_compute.attempt_id,
                false,
                Some("speculation gate failed".into()),
            );
            return Ok(None);
        }

        let adapter = self.inference_adapter().ok_or_else(|| {
            InferenceError::Provider("compute broker has no inference adapter".into())
        })?;
        let registry = adapter.pooled().job_registry();

        let primary_tokens: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let primary_tokens_c = primary_tokens.clone();
        let mut primary_sink = move |t: &str| {
            if let Ok(mut g) = primary_tokens_c.lock() {
                g.push(t.to_string());
            }
        };
        let mut spec_sink = |_t: &str| {};

        let p_adapter = adapter.clone();
        let s_adapter = adapter.clone();
        let p_req = primary_req.clone();
        let s_req = spec_req.clone();

        let primary_fut = p_adapter.chat(p_req, &mut primary_sink);
        let spec_fut = s_adapter.chat(s_req, &mut spec_sink);
        tokio::pin!(primary_fut);
        tokio::pin!(spec_fut);

        let race_started = std::time::Instant::now();
        self.scheduler_metrics().record_speculation_launched();
        let (winner_primary, result) = tokio::select! {
            r = &mut primary_fut => {
                if r.is_ok() { (true, r) } else { (false, spec_fut.await) }
            },
            r = &mut spec_fut => {
                if r.is_ok() { (false, r) } else { (true, primary_fut.await) }
            },
        };
        let winner_ms = race_started.elapsed().as_millis() as u64;

        if winner_primary {
            registry.cancel_session(&spec_session);
            let _ = self
                .cancel(
                    spec_compute.attempt_id.clone(),
                    CancellationReason::Superseded,
                )
                .await;
            // Loser cost ≈ work in flight until cancel (same wall as winner race).
            self.scheduler_metrics()
                .record_speculation_race(true, winner_ms, winner_ms);
            match result {
                Ok(resp) => {
                    if let Ok(g) = primary_tokens.lock() {
                        for t in g.iter() {
                            (on_token)(t);
                        }
                    }
                    self.finish_attempt_pub(&primary_compute.attempt_id, true, None);
                    self.finish_attempt_pub(
                        &spec_compute.attempt_id,
                        false,
                        Some("speculative loser canceled".into()),
                    );
                    self.record_remote_circuit(&primary_compute, true);
                    timing.mark_result_received();
                    timing.mark_verified();
                    timing.mark_accepted();
                    Ok(Some(resp))
                }
                Err(_) => {
                    self.finish_attempt_pub(
                        &primary_compute.attempt_id,
                        false,
                        Some("primary race failed".into()),
                    );
                    self.finish_attempt_pub(
                        &spec_compute.attempt_id,
                        false,
                        Some("race aborted".into()),
                    );
                    Ok(None)
                }
            }
        } else {
            registry.cancel_session(&primary_session);
            let _ = self
                .cancel(
                    primary_compute.attempt_id.clone(),
                    CancellationReason::Superseded,
                )
                .await;
            self.scheduler_metrics()
                .record_speculation_race(false, winner_ms, winner_ms);
            match result {
                Ok(resp) => {
                    (on_token)(&resp.message.content);
                    self.finish_attempt_pub(&spec_compute.attempt_id, true, None);
                    self.finish_attempt_pub(
                        &primary_compute.attempt_id,
                        false,
                        Some("primary loser canceled".into()),
                    );
                    self.record_remote_circuit(&spec_compute, true);
                    timing.mark_result_received();
                    timing.mark_verified();
                    timing.mark_accepted();
                    Ok(Some(resp))
                }
                Err(_) => {
                    self.finish_attempt_pub(
                        &spec_compute.attempt_id,
                        false,
                        Some("speculative failed".into()),
                    );
                    self.finish_attempt_pub(
                        &primary_compute.attempt_id,
                        false,
                        Some("race aborted".into()),
                    );
                    Ok(None)
                }
            }
        }
    }

    fn record_remote_circuit(&self, compute_req: &ComputeRequest, ok: bool) {
        if let Some(wid) = compute_req
            .target_worker_id
            .as_ref()
            .map(|w| w.0.as_str())
            .filter(|id| *id != LOCAL_NODE_ID && !id.eq_ignore_ascii_case("local"))
        {
            if ok {
                self.circuits().record_success(wid);
            } else {
                self.circuits().record_failure(wid);
                if matches!(
                    self.circuits().state(wid),
                    crate::scheduler::CircuitState::Open
                ) {
                    self.scheduler_metrics().record_circuit_open();
                }
            }
        }
    }

    fn finalize_decision(
        &self,
        decision: &SchedulerDecision,
        started: std::time::Instant,
        timing: &mut CoordinatorObservedTiming,
        worker_local: Option<&tetonic_telemetry::WorkerLocalDurations>,
    ) {
        self.record_prediction_feedback_pub(decision, started.elapsed().as_millis() as u64);
        let mut finished = decision.clone();
        finished.pending = false;
        let _ = self.scheduler_store().upsert(&finished);
        if timing.accepted_at.is_none() && timing.result_received_at.is_some() {
            timing.mark_accepted();
        }
        let report = critical_path_ms(timing, worker_local.or(timing.worker.as_ref()));
        record_stage_segments(
            &report.segments,
            Some(decision.decision_id.0.as_str()),
            false,
            report.missing_worker_spans,
        );
        tracing::debug!(
            target: "lokai_compute_trace",
            report = %format_critical_path_report(&report),
            "critical path"
        );
    }

    fn track_inflight_chat(
        &self,
        attempt_id: &AttemptId,
        chat: &ChatRequest,
        compute_req: &ComputeRequest,
        order: &[ExecutionTargetId],
    ) {
        if let Ok(mut g) = self.inflight_chats().lock() {
            g.insert(
                attempt_id.0.clone(),
                PendingWorkerLossContinue {
                    compute_req: compute_req.clone(),
                    chat: chat.clone(),
                    failed: compute_req
                        .target_worker_id
                        .as_ref()
                        .map(|w| {
                            if w.0 == LOCAL_NODE_ID || w.0.eq_ignore_ascii_case("local") {
                                ExecutionTargetId::Local
                            } else {
                                ExecutionTargetId::Worker {
                                    worker_id: w.clone(),
                                }
                            }
                        })
                        .unwrap_or(ExecutionTargetId::Local),
                    order: order.to_vec(),
                },
            );
        }
    }

    fn clear_inflight_chat(&self, attempt_id: &AttemptId) {
        if let Ok(mut g) = self.inflight_chats().lock() {
            g.remove(&attempt_id.0);
        }
        if let Ok(mut g) = self.pending_continues().lock() {
            g.remove(&attempt_id.0);
        }
    }
}
