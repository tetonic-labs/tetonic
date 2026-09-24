//! Fresh placement evaluation per failover / speculation hop (M6-2).

use chrono::Utc;
use tetonic_domain::{
    PlacementReason, ProjectPlacementPolicy, TrustPlacementDecision, WorkerTrust,
};
use tetonic_fabric_protocol::{CapabilityRegistry, ModelSelection};
use tetonic_inference::{evaluate_placement, placement_request_from_chat, ChatRequest};

use crate::scheduler::stamp::apply_execution_target;
use crate::scheduler::types::ExecutionTargetId;
use crate::types::ComputeRequest;

/// Result of hop placement revalidation before admit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopPlacementOutcome {
    /// Target stamped and eligible for admit/dispatch.
    Allowed,
    /// Skip this hop (placement deny / local-only for a remote target).
    Skip { reason: PlacementReason },
}

/// Retarget `compute_req` and stamp a real [`TrustPlacementDecision`] from
/// `evaluate_placement` (not a fabricated Eligible).
///
/// Local hops always stamp LocalOnly and are Allowed. Remote hops require
/// Eligible / EligibleAfterRedaction; otherwise Skip.
pub fn revalidate_hop_placement(
    compute_req: &mut ComputeRequest,
    chat: &ChatRequest,
    target: &ExecutionTargetId,
    caps: Option<&CapabilityRegistry>,
    worker_trust: WorkerTrust,
    project_policy: ProjectPlacementPolicy,
) -> HopPlacementOutcome {
    apply_execution_target(compute_req, target);

    match target {
        ExecutionTargetId::Local => {
            // apply_execution_target already stamped LocalOnly for local.
            HopPlacementOutcome::Allowed
        }
        ExecutionTargetId::Worker { worker_id } => {
            let now = Utc::now();
            let model = ModelSelection::from_request(&chat.model, chat.model_digest.as_deref());
            let placement_req = placement_request_from_chat(
                chat,
                worker_id.0.as_str(),
                compute_req.placement_decision.policy_epoch,
                project_policy,
            );
            let decision =
                evaluate_placement(&placement_req, worker_trust, caps, Some(&model), now);
            compute_req.placement_decision.decision = decision.clone();
            compute_req.placement_decision.issued_at = now;
            compute_req.placement_decision.expires_at = now + chrono::Duration::minutes(5);
            compute_req.target_worker_id = Some(worker_id.clone());

            if decision.allows_remote() {
                HopPlacementOutcome::Allowed
            } else {
                let reason = match decision {
                    TrustPlacementDecision::LocalOnly { reason }
                    | TrustPlacementDecision::Denied { reason } => reason,
                    TrustPlacementDecision::Eligible { .. }
                    | TrustPlacementDecision::EligibleAfterRedaction { .. } => {
                        PlacementReason::CapabilityUnavailable
                    }
                };
                HopPlacementOutcome::Skip { reason }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tetonic_domain::ids::{AttemptId, RunId, TaskId, WorkerId};
    use tetonic_domain::DataClass;
    use tetonic_fabric_protocol::{CapabilityRegistry, JobKind, WorkerCapabilities};
    use tetonic_inference::{ChatRequest, Message};

    use crate::budget::ResourceRequest;
    use crate::priority::ComputePriority;
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
            input_digest: tetonic_domain::ContentDigest::new("d"),
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
            verification_policy: tetonic_domain::VerificationPolicyReference {
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

    fn chat() -> ChatRequest {
        ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
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
        }
    }

    fn seed_caps(reg: &mut CapabilityRegistry, worker_id: &str) {
        let wid = WorkerId::new(worker_id);
        let now = Utc::now();
        let mut caps = WorkerCapabilities::legacy_infer_profile(
            wid.clone(),
            "boot",
            1,
            0,
            &["qwen:7b".into()],
            &["qwen:7b".into()],
            8192,
            4096,
            0,
            0,
            2,
        );
        caps.generated_at = now;
        caps.valid_until = now + chrono::Duration::hours(1);
        reg.upsert_validated(caps, &wid, 0, now).unwrap();
    }

    #[test]
    fn local_hop_always_allowed() {
        let mut req = base_req();
        let outcome = revalidate_hop_placement(
            &mut req,
            &chat(),
            &ExecutionTargetId::Local,
            None,
            WorkerTrust::LocalMachine,
            ProjectPlacementPolicy::default(),
        );
        assert_eq!(outcome, HopPlacementOutcome::Allowed);
        assert!(matches!(
            req.placement_decision.decision,
            TrustPlacementDecision::LocalOnly { .. }
        ));
    }

    #[test]
    fn remote_hop_stamps_real_eligible_when_caps_ok() {
        let mut reg = CapabilityRegistry::new();
        seed_caps(&mut reg, "w1");
        let mut req = base_req();
        let target = ExecutionTargetId::Worker {
            worker_id: WorkerId::new("w1"),
        };
        let outcome = revalidate_hop_placement(
            &mut req,
            &chat(),
            &target,
            Some(&reg),
            WorkerTrust::OwnerControlledEstate,
            ProjectPlacementPolicy::default(),
        );
        assert_eq!(outcome, HopPlacementOutcome::Allowed);
        assert!(matches!(
            req.placement_decision.decision,
            TrustPlacementDecision::Eligible { .. }
        ));
    }

    #[test]
    fn quarantined_remote_hop_skipped() {
        let mut reg = CapabilityRegistry::new();
        seed_caps(&mut reg, "w1");
        assert!(reg.force_quarantine("w1"));
        let mut req = base_req();
        let target = ExecutionTargetId::Worker {
            worker_id: WorkerId::new("w1"),
        };
        let outcome = revalidate_hop_placement(
            &mut req,
            &chat(),
            &target,
            Some(&reg),
            WorkerTrust::OwnerControlledEstate,
            ProjectPlacementPolicy::default(),
        );
        assert!(matches!(
            outcome,
            HopPlacementOutcome::Skip {
                reason: PlacementReason::WorkerQuarantined
            }
        ));
    }

    #[test]
    fn untrusted_remote_hop_skipped() {
        let mut reg = CapabilityRegistry::new();
        seed_caps(&mut reg, "w1");
        let mut req = base_req();
        let target = ExecutionTargetId::Worker {
            worker_id: WorkerId::new("w1"),
        };
        let outcome = revalidate_hop_placement(
            &mut req,
            &chat(),
            &target,
            Some(&reg),
            WorkerTrust::ExternalUntrusted,
            ProjectPlacementPolicy::default(),
        );
        assert!(matches!(outcome, HopPlacementOutcome::Skip { .. }));
    }
}
