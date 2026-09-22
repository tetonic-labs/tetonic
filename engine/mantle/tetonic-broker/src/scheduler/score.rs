//! Deterministic weighted scoring and offload rule (M6-2).

use chrono::Utc;
use tetonic_domain::ids::{AttemptId, RunId, TaskId};
use tetonic_domain::DataClass;

use crate::scheduler::circuit::CircuitBreakerRegistry;
use crate::scheduler::estimate::{estimate_finish_ms, CandidateInputs};
use crate::scheduler::types::{
    CandidateEstimate, ExecutionTargetId, SchedulerDecision, SchedulerDecisionId, SchedulerReason,
};

pub const SCHEDULER_MODEL_VERSION: &str = "weighted_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerMode {
    /// Deterministic local-first (feature-flag / safety fallback).
    LocalFirst,
    /// Completion-time inequality + min speedup.
    Weighted,
}

impl SchedulerMode {
    pub fn from_env() -> Self {
        match std::env::var("LOKAI_SCHEDULER")
            .unwrap_or_else(|_| "weighted".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "local_first" | "local-first" | "local" => Self::LocalFirst,
            _ => Self::Weighted,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub mode: SchedulerMode,
    pub min_remote_speedup: f32,
    pub min_remote_speedup_small: f32,
    pub small_task_transfer_bytes: u64,
    pub max_transfer_ratio: f32,
    pub max_queue_depth: u32,
    pub policy_penalty_ms: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            mode: SchedulerMode::from_env(),
            min_remote_speedup: 1.25,
            min_remote_speedup_small: 1.5,
            small_task_transfer_bytes: 64 * 1024,
            max_transfer_ratio: 0.35,
            max_queue_depth: 32,
            policy_penalty_ms: 50,
        }
    }
}

pub struct DecideInput<'a> {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub data_class: DataClass,
    pub local_only: bool,
    pub candidates: Vec<CandidateInputs>,
    pub circuits: Option<&'a CircuitBreakerRegistry>,
    pub config: &'a SchedulerConfig,
}

pub fn decide(input: DecideInput<'_>) -> SchedulerDecision {
    let decided_at = Utc::now();
    let decision_id = SchedulerDecisionId::new(format!("sched_{}", uuid::Uuid::new_v4()));

    if input.local_only || matches!(input.data_class, DataClass::Secret) {
        let reason = if matches!(input.data_class, DataClass::Secret) {
            SchedulerReason::SecretLocalOnly
        } else {
            SchedulerReason::NoEligibleRemote
        };
        return local_only_decision(decision_id, input, decided_at, reason);
    }

    if matches!(input.config.mode, SchedulerMode::LocalFirst) {
        return local_only_decision(
            decision_id,
            input,
            decided_at,
            SchedulerReason::FeatureFlagLocalFirst,
        );
    }

    let mut estimates: Vec<CandidateEstimate> = Vec::new();
    for c in &input.candidates {
        if c.queue_depth > input.config.max_queue_depth {
            continue;
        }
        if let ExecutionTargetId::Worker { worker_id } = &c.target {
            if let Some(reg) = input.circuits {
                if !reg.allows_dispatch(&worker_id.0) {
                    continue;
                }
            }
        }
        let est = estimate_finish_ms(c);
        estimates.push(CandidateEstimate {
            target: c.target.clone(),
            estimated_finish_ms: est.finish_ms,
            uncertainty_margin_ms: est.uncertainty_margin_ms,
            queue_depth: c.queue_depth,
            cold_start: c.cold_start,
            transfer_bytes: c.transfer_bytes,
            score: est.scored_ms,
        });
    }

    estimates.sort_by_key(|e| e.score);
    let local = estimates
        .iter()
        .find(|e| matches!(e.target, ExecutionTargetId::Local))
        .cloned();
    let best_remote = estimates
        .iter()
        .find(|e| matches!(e.target, ExecutionTargetId::Worker { .. }))
        .cloned();

    let (selected, reason, speedup, margin) = match (local, best_remote) {
        (Some(loc), Some(rem)) => {
            let small = rem.transfer_bytes <= input.config.small_task_transfer_bytes
                && loc.estimated_finish_ms < 2_000;
            let min_speedup = if small {
                input.config.min_remote_speedup_small
            } else {
                input.config.min_remote_speedup
            };
            let remote_with_margin = rem
                .estimated_finish_ms
                .saturating_add(rem.uncertainty_margin_ms)
                .saturating_add(input.config.policy_penalty_ms);
            let local_finish = loc.estimated_finish_ms.max(1);
            let speedup = local_finish as f32 / remote_with_margin.max(1) as f32;
            let transfer_too_large = rem.transfer_bytes > 0
                && (rem.transfer_bytes as f64)
                    > (input.config.max_transfer_ratio as f64) * (local_finish as f64) * 1024.0;
            if transfer_too_large {
                (
                    Some(loc.target.clone()),
                    SchedulerReason::TransferTooLarge,
                    None,
                    loc.uncertainty_margin_ms,
                )
            } else if remote_with_margin < local_finish && speedup >= min_speedup {
                (
                    Some(rem.target.clone()),
                    SchedulerReason::RemoteFaster,
                    Some(speedup),
                    rem.uncertainty_margin_ms,
                )
            } else if (rem.score as i64 - loc.score as i64).abs()
                <= loc.uncertainty_margin_ms.max(rem.uncertainty_margin_ms) as i64
            {
                (
                    Some(loc.target.clone()),
                    SchedulerReason::EqualWithUncertainty,
                    None,
                    loc.uncertainty_margin_ms.max(rem.uncertainty_margin_ms),
                )
            } else if speedup < min_speedup && remote_with_margin < local_finish {
                (
                    Some(loc.target.clone()),
                    SchedulerReason::MinSpeedupNotMet,
                    Some(speedup),
                    loc.uncertainty_margin_ms,
                )
            } else {
                (
                    Some(loc.target.clone()),
                    SchedulerReason::LocalFaster,
                    None,
                    loc.uncertainty_margin_ms,
                )
            }
        }
        (Some(loc), None) => (
            Some(loc.target.clone()),
            SchedulerReason::NoEligibleRemote,
            None,
            loc.uncertainty_margin_ms,
        ),
        (None, Some(rem)) => (
            Some(rem.target.clone()),
            SchedulerReason::RemoteFaster,
            None,
            rem.uncertainty_margin_ms,
        ),
        (None, None) => (
            Some(ExecutionTargetId::Local),
            SchedulerReason::NoEligibleRemote,
            None,
            0,
        ),
    };

    let mut fallback_order: Vec<ExecutionTargetId> =
        estimates.iter().map(|e| e.target.clone()).collect();
    if let Some(sel) = &selected {
        fallback_order.retain(|t| t != sel);
        let mut ordered = vec![sel.clone()];
        ordered.extend(fallback_order);
        fallback_order = ordered;
    }

    SchedulerDecision {
        decision_id,
        run_id: input.run_id,
        task_id: input.task_id,
        attempt_id: input.attempt_id,
        candidates: estimates,
        selected_target: selected,
        fallback_order,
        uncertainty_margin_ms: margin,
        expected_speedup: speedup,
        reason,
        model_version: SCHEDULER_MODEL_VERSION.into(),
        decided_at,
        pending: true,
    }
}

fn local_only_decision(
    decision_id: SchedulerDecisionId,
    input: DecideInput<'_>,
    decided_at: chrono::DateTime<Utc>,
    reason: SchedulerReason,
) -> SchedulerDecision {
    let local_est = input
        .candidates
        .iter()
        .find(|c| matches!(c.target, ExecutionTargetId::Local))
        .map(|c| {
            let e = estimate_finish_ms(c);
            CandidateEstimate {
                target: ExecutionTargetId::Local,
                estimated_finish_ms: e.finish_ms,
                uncertainty_margin_ms: e.uncertainty_margin_ms,
                queue_depth: c.queue_depth,
                cold_start: c.cold_start,
                transfer_bytes: 0,
                score: e.scored_ms,
            }
        });
    let margin = local_est
        .as_ref()
        .map(|e| e.uncertainty_margin_ms)
        .unwrap_or(0);
    let candidates = local_est.into_iter().collect::<Vec<_>>();
    SchedulerDecision {
        decision_id,
        run_id: input.run_id,
        task_id: input.task_id,
        attempt_id: input.attempt_id,
        candidates,
        selected_target: Some(ExecutionTargetId::Local),
        fallback_order: vec![ExecutionTargetId::Local],
        uncertainty_margin_ms: margin,
        expected_speedup: None,
        reason,
        model_version: SCHEDULER_MODEL_VERSION.into(),
        decided_at,
        pending: true,
    }
}
