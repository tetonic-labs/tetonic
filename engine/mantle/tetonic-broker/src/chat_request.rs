//! Map ChatRequest → ComputeRequest for broker Infer path.

use chrono::Utc;
use tetonic_domain::ids::{AttemptId, RunId, TaskId};
use tetonic_domain::{
    ContentDigest, DataClass, PlacementReason, TraceContext, TrustPlacementDecision,
    VerificationPolicyReference,
};
use tetonic_fabric_protocol::JobKind;
use tetonic_inference::ChatRequest;

use crate::job_profile::profile_for;
use crate::priority::ComputePriority;
use crate::types::{
    ComputeRequest, DeadlinePolicy, PlacementDecisionReference, RetryPolicyReference,
};

pub fn compute_request_from_chat(req: &ChatRequest) -> ComputeRequest {
    let meta = req.fabric.as_ref();
    let run_id = RunId::new(format!("run_{}", uuid::Uuid::new_v4()));
    let attempt_id = AttemptId::new(format!("att_{}", uuid::Uuid::new_v4()));
    let task_id = TaskId::new(format!("task_{}", attempt_id.0));
    let data_class = meta
        .map(|m| {
            m.context_data_class
                .map(|context| m.data_class.max(context))
                .unwrap_or(m.data_class)
        })
        .unwrap_or(DataClass::RepositorySource);
    let now = Utc::now();
    ComputeRequest {
        run_id: run_id.clone(),
        task_id: task_id.clone(),
        task_version: 1,
        attempt_id: attempt_id.clone(),
        job_kind: JobKind::Infer,
        input_artifacts: meta.map(|m| m.input_artifacts.clone()).unwrap_or_default(),
        input_digest: ContentDigest::new(format!("chat:{}", req.messages.len())),
        workspace_version: meta.and_then(|m| m.workspace_version.clone()),
        data_class,
        placement_decision: PlacementDecisionReference {
            decision_id: format!("plc_{}", uuid::Uuid::new_v4()),
            issued_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            policy_epoch: 0,
            // Local-first chat remains eligible without a remote placement decision.
            decision: TrustPlacementDecision::LocalOnly {
                reason: PlacementReason::CapabilityUnavailable,
            },
        },
        resource_request: profile_for(&JobKind::Infer).default_resources,
        deadline: DeadlinePolicy::default(),
        retry_policy: RetryPolicyReference::default(),
        verification_policy: VerificationPolicyReference {
            policy_id: "structural".into(),
        },
        priority: ComputePriority::Interactive,
        trace_context: TraceContext {
            trace_id: meta
                .map(|m| m.trace_context.trace_id.clone())
                .unwrap_or_default(),
            span_id: meta
                .map(|m| m.trace_context.span_id.clone())
                .unwrap_or_default(),
            run_id: Some(run_id),
            task_id: Some(task_id),
            attempt_id: Some(attempt_id),
            scheduler_decision_id: meta
                .and_then(|m| m.scheduler_decision_id.clone())
                .or_else(|| meta.and_then(|m| m.trace_context.scheduler_decision_id.clone())),
        },
        speculative: false,
        project_id: None,
        target_worker_id: None,
        fallback_order: vec![],
        scheduler_decision_id: meta
            .and_then(|m| m.scheduler_decision_id.clone())
            .or_else(|| meta.and_then(|m| m.trace_context.scheduler_decision_id.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_inference::{ChatRequest, FabricCallMeta, Message};

    #[test]
    fn secret_context_class_raises_the_compute_request() {
        let mut req = ChatRequest {
            messages: vec![Message::user("hello")],
            fabric: Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                context_data_class: Some(DataClass::Secret),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            compute_request_from_chat(&req).data_class,
            DataClass::Secret
        );
        req.fabric = Some(FabricCallMeta {
            data_class: DataClass::RepositorySource,
            ..Default::default()
        });
        assert_eq!(
            compute_request_from_chat(&req).data_class,
            DataClass::RepositorySource
        );
    }
}
