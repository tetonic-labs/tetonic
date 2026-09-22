//! Apply scheduler decisions onto ComputeRequest / ChatRequest (M6-2).

use chrono::Utc;
use lokai_domain::ids::WorkerId;
use lokai_domain::{
    EligibleTarget, PlacementReason, TrustPlacementDecision, VerificationRequirement, WorkerTrust,
};
use lokai_inference::{ChatRequest, LOCAL_NODE_ID};

use crate::scheduler::types::{ExecutionTargetId, SchedulerDecision};
use crate::types::ComputeRequest;

pub fn apply_scheduler_decision(compute_req: &mut ComputeRequest, decision: &SchedulerDecision) {
    let id = decision.decision_id.0.clone();
    compute_req.scheduler_decision_id = Some(id.clone());
    compute_req.trace_context.scheduler_decision_id = Some(id);
    compute_req.fallback_order = decision
        .fallback_order
        .iter()
        .map(|t| t.as_label())
        .collect();
    compute_req.target_worker_id = match &decision.selected_target {
        Some(ExecutionTargetId::Local) => Some(WorkerId::new(LOCAL_NODE_ID)),
        Some(ExecutionTargetId::Worker { worker_id }) => Some(worker_id.clone()),
        None => None,
    };
    compute_req.placement_decision.decision =
        placement_for_selection(decision.selected_target.as_ref(), compute_req.data_class);
    let now = Utc::now();
    compute_req.placement_decision.issued_at = now;
    compute_req.placement_decision.expires_at = now + chrono::Duration::minutes(5);
}

fn placement_for_selection(
    selected: Option<&ExecutionTargetId>,
    data_class: lokai_domain::DataClass,
) -> TrustPlacementDecision {
    match selected {
        Some(ExecutionTargetId::Worker { worker_id }) => TrustPlacementDecision::Eligible {
            targets: vec![EligibleTarget {
                worker_id: worker_id.clone(),
                trust: WorkerTrust::OwnerControlledEstate,
            }],
            required_verification: VerificationRequirement::None,
        },
        Some(ExecutionTargetId::Local) | None => TrustPlacementDecision::LocalOnly {
            reason: if matches!(data_class, lokai_domain::DataClass::Secret) {
                PlacementReason::SecretLocalOnly
            } else {
                PlacementReason::CapabilityUnavailable
            },
        },
    }
}

pub fn stamp_fabric_preferred(req: &mut ChatRequest, decision: &SchedulerDecision) {
    let preferred = decision.selected_target.as_ref().map(|t| t.as_label());
    let meta = req.fabric.get_or_insert_with(Default::default);
    meta.preferred_target = preferred;
    meta.fallback_order = decision
        .fallback_order
        .iter()
        .map(|t| t.as_label())
        .collect();
    let id = decision.decision_id.0.clone();
    meta.scheduler_decision_id = Some(id.clone());
    meta.trace_context.scheduler_decision_id = Some(id);
}

/// Retarget an admitted compute request onto the next fallback hop.
pub fn apply_execution_target(compute_req: &mut ComputeRequest, target: &ExecutionTargetId) {
    compute_req.target_worker_id = match target {
        ExecutionTargetId::Local => Some(WorkerId::new(LOCAL_NODE_ID)),
        ExecutionTargetId::Worker { worker_id } => Some(worker_id.clone()),
    };
    compute_req.placement_decision.decision =
        placement_for_selection(Some(target), compute_req.data_class);
    let now = Utc::now();
    compute_req.placement_decision.issued_at = now;
    compute_req.placement_decision.expires_at = now + chrono::Duration::minutes(5);
}

/// Stamp a single preferred hop for broker-driven re-admit (pooled must not walk the full list).
pub fn stamp_fabric_single_target(
    req: &mut ChatRequest,
    target: &ExecutionTargetId,
    decision_id: Option<&str>,
) {
    let meta = req.fabric.get_or_insert_with(Default::default);
    let label = target.as_label();
    meta.preferred_target = Some(label.clone());
    meta.fallback_order = vec![label];
    if let Some(id) = decision_id {
        meta.scheduler_decision_id = Some(id.to_string());
        meta.trace_context.scheduler_decision_id = Some(id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::ids::{AttemptId, RunId, TaskId};
    use lokai_domain::DataClass;
    use lokai_fabric_protocol::JobKind;

    use crate::budget::ResourceRequest;
    use crate::priority::ComputePriority;
    use crate::scheduler::types::{SchedulerDecisionId, SchedulerReason};
    use crate::types::{DeadlinePolicy, PlacementDecisionReference, RetryPolicyReference};

    fn base_req() -> ComputeRequest {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: lokai_domain::ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "p".into(),
                issued_at: now,
                expires_at: now + chrono::Duration::minutes(5),
                policy_epoch: 0,
                decision: TrustPlacementDecision::LocalOnly {
                    reason: PlacementReason::CapabilityUnavailable,
                },
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: lokai_domain::VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: Default::default(),
            speculative: false,
            project_id: None,
            target_worker_id: None,
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    }

    #[test]
    fn remote_selection_sets_eligible_placement() {
        let mut req = base_req();
        let decision = SchedulerDecision {
            decision_id: SchedulerDecisionId::new("sched_1"),
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            attempt_id: AttemptId::new("a"),
            candidates: vec![],
            selected_target: Some(ExecutionTargetId::Worker {
                worker_id: WorkerId::new("w1"),
            }),
            fallback_order: vec![],
            uncertainty_margin_ms: 10,
            expected_speedup: Some(1.5),
            reason: SchedulerReason::RemoteFaster,
            model_version: "weighted_v1".into(),
            decided_at: Utc::now(),
            pending: true,
        };
        apply_scheduler_decision(&mut req, &decision);
        assert_eq!(req.scheduler_decision_id.as_deref(), Some("sched_1"));
        assert_eq!(
            req.trace_context.scheduler_decision_id.as_deref(),
            Some("sched_1")
        );
        assert_eq!(
            req.target_worker_id.as_ref().map(|w| w.0.as_str()),
            Some("w1")
        );
        assert!(matches!(
            req.placement_decision.decision,
            TrustPlacementDecision::Eligible { .. }
        ));
    }

    #[test]
    fn stamp_fabric_copies_fallback_order() {
        use lokai_domain::ids::{AttemptId, RunId, TaskId};
        use lokai_inference::{ChatRequest, Message};

        let mut chat = ChatRequest {
            model: "m".into(),
            model_digest: None,
            messages: vec![Message::user("hi")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: None,
            response_format: None,
            outbound_scan: Default::default(),
        };
        let decision = SchedulerDecision {
            decision_id: SchedulerDecisionId::new("sched_fb"),
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            attempt_id: AttemptId::new("a"),
            candidates: vec![],
            selected_target: Some(ExecutionTargetId::Worker {
                worker_id: WorkerId::new("w1"),
            }),
            fallback_order: vec![
                ExecutionTargetId::Worker {
                    worker_id: WorkerId::new("w1"),
                },
                ExecutionTargetId::Local,
            ],
            uncertainty_margin_ms: 10,
            expected_speedup: Some(1.5),
            reason: SchedulerReason::RemoteFaster,
            model_version: "weighted_v1".into(),
            decided_at: Utc::now(),
            pending: true,
        };
        stamp_fabric_preferred(&mut chat, &decision);
        let meta = chat.fabric.as_ref().unwrap();
        assert_eq!(meta.preferred_target.as_deref(), Some("w1"));
        assert_eq!(
            meta.fallback_order,
            vec!["w1".to_string(), "local".to_string()]
        );
        assert_eq!(meta.scheduler_decision_id.as_deref(), Some("sched_fb"));
    }
}
