//! Build scheduler candidates from fabric snapshot + chat (M6-2).

use std::collections::HashMap;

use chrono::Utc;
use tetonic_domain::ids::WorkerId;
use tetonic_domain::{DataClass, ProjectPlacementPolicy, TrustPlacementDecision, WorkerTrust};
use tetonic_fabric_protocol::{CapabilityRegistry, ModelSelection, WorkerSchedulingState};
use tetonic_inference::{
    evaluate_placement, placement_request_from_chat, ChatRequest, FabricSnapshot, LOCAL_NODE_ID,
};

use crate::scheduler::calibration::PredictionCalibration;
use crate::scheduler::circuit::CircuitBreakerRegistry;
use crate::scheduler::score::{decide, DecideInput, SchedulerConfig};
use crate::scheduler::types::{ExecutionTargetId, SchedulerDecision, SchedulerReason};
use crate::scheduler::{CandidateInputs, UncertaintyModel};
use crate::types::ComputeRequest;

/// Inputs for Infer chat scheduling (eligibility before ranking).
pub struct ScheduleInferChat<'a> {
    pub compute_req: &'a ComputeRequest,
    pub snap: &'a FabricSnapshot,
    pub chat: &'a ChatRequest,
    pub circuits: &'a CircuitBreakerRegistry,
    pub config: &'a SchedulerConfig,
    pub caps: Option<&'a CapabilityRegistry>,
    pub calibration: Option<&'a PredictionCalibration>,
    pub worker_trust_by_node: HashMap<String, WorkerTrust>,
    pub policy_epoch: u64,
    pub project_policy: ProjectPlacementPolicy,
}

pub fn schedule_infer_chat(input: ScheduleInferChat<'_>) -> SchedulerDecision {
    let ScheduleInferChat {
        compute_req,
        snap,
        chat,
        circuits,
        config,
        caps,
        calibration,
        worker_trust_by_node,
        policy_epoch,
        project_policy,
    } = input;

    let input_bytes = chat.messages.iter().map(|m| m.content.len()).sum::<usize>() as u64;
    let force_local_secret = matches!(compute_req.data_class, DataClass::Secret);
    let now = Utc::now();
    let model = ModelSelection::from_request(&chat.model, chat.model_digest.as_deref());

    let local_key = ExecutionTargetId::Local.as_label();
    let local_uncertainty = calibration
        .map(|c| c.uncertainty_for(&local_key, false))
        .unwrap_or(UncertaintyModel {
            samples: 20,
            rolling_mae_ms: 40,
            ..UncertaintyModel::default()
        });

    let mut candidates = vec![CandidateInputs {
        target: ExecutionTargetId::Local,
        admission_delay_ms: 5,
        queue_delay_ms: 10,
        connection_setup_ms: 0,
        input_transfer_ms: 0,
        cold_start_ms: 0,
        execution_ms: compute_req
            .resource_request
            .estimated_duration
            .expected_ms
            .max(50),
        result_transfer_ms: 0,
        verification_ms: 5,
        queue_depth: 0,
        transfer_bytes: 0,
        cold_start: false,
        uncertainty: local_uncertainty,
    }];

    let mut excluded_quarantine = false;

    if !force_local_secret {
        for node in snap
            .nodes
            .iter()
            .filter(|n| n.id != LOCAL_NODE_ID && n.healthy)
        {
            if let Some(reg) = caps {
                match reg.scheduling_state(&node.id, now) {
                    WorkerSchedulingState::Quarantined | WorkerSchedulingState::Draining => {
                        excluded_quarantine = true;
                        continue;
                    }
                    WorkerSchedulingState::NotCached
                    | WorkerSchedulingState::Expired
                    | WorkerSchedulingState::Degraded
                    | WorkerSchedulingState::Eligible => {}
                }
                let Some(worker_trust) = worker_trust_by_node.get(&node.id).copied() else {
                    continue;
                };
                let placement_req = placement_request_from_chat(
                    chat,
                    &node.id,
                    policy_epoch,
                    project_policy.clone(),
                );
                let placement =
                    evaluate_placement(&placement_req, worker_trust, Some(reg), Some(&model), now);
                if !matches!(
                    placement,
                    TrustPlacementDecision::Eligible { .. }
                        | TrustPlacementDecision::EligibleAfterRedaction { .. }
                ) {
                    continue;
                }
            } else {
                let Some(worker_trust) = worker_trust_by_node.get(&node.id).copied() else {
                    continue;
                };
                let placement_req = placement_request_from_chat(
                    chat,
                    &node.id,
                    policy_epoch,
                    project_policy.clone(),
                );
                let placement = evaluate_placement(&placement_req, worker_trust, None, None, now);
                if !placement.allows_remote() {
                    continue;
                }
            }

            if !circuits.allows_dispatch(&node.id) {
                continue;
            }
            let queue_depth = node.queue_depth;
            let cold = !node.resident_models.iter().any(|m| m == &chat.model);
            let transfer_ms = if input_bytes == 0 {
                5
            } else {
                (input_bytes / 10_000).max(5)
            };
            let exec = compute_req
                .resource_request
                .estimated_duration
                .expected_ms
                .saturating_mul(80)
                / 100;
            let uncertainty = calibration
                .map(|c| c.uncertainty_for(&node.id, cold))
                .unwrap_or(UncertaintyModel {
                    samples: if cold { 0 } else { 8 },
                    rolling_mae_ms: if cold { 0 } else { 80 },
                    cold_floor_ms: 300,
                    cold_model_extra_ms: 500,
                });
            candidates.push(CandidateInputs {
                target: ExecutionTargetId::Worker {
                    worker_id: WorkerId::new(node.id.clone()),
                },
                admission_delay_ms: 10,
                queue_delay_ms: u64::from(queue_depth) * 25,
                connection_setup_ms: 20,
                input_transfer_ms: transfer_ms,
                cold_start_ms: if cold { 800 } else { 0 },
                execution_ms: exec.max(30),
                result_transfer_ms: transfer_ms / 2,
                verification_ms: 15,
                queue_depth,
                transfer_bytes: input_bytes,
                cold_start: cold,
                uncertainty,
            });
        }
    }

    let mut decision = decide(DecideInput {
        run_id: compute_req.run_id.clone(),
        task_id: compute_req.task_id.clone(),
        attempt_id: compute_req.attempt_id.clone(),
        data_class: compute_req.data_class,
        local_only: force_local_secret,
        candidates,
        circuits: Some(circuits),
        config,
    });

    if excluded_quarantine
        && matches!(
            decision.reason,
            SchedulerReason::NoEligibleRemote | SchedulerReason::LocalFaster
        )
        && !decision
            .candidates
            .iter()
            .any(|c| matches!(c.target, ExecutionTargetId::Worker { .. }))
    {
        decision.reason = SchedulerReason::Quarantined;
    }

    decision
}
