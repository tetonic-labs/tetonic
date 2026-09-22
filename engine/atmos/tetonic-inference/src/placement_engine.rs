//! Placement policy engine — trust × capability eligibility (M5-3).

use chrono::{DateTime, Utc};
use tetonic_domain::ids::WorkerId;
use tetonic_domain::placement::{
    EligibleTarget, PlacementDecision, PlacementJobKind, PlacementReason, PlacementRequest,
    ProjectPlacementPolicy, SandboxRequirements, VerificationRequirement,
};
use tetonic_domain::{DataClass, WorkerTrust};
use tetonic_fabric_protocol::{
    model_inventory_matches, CapabilityRegistry, ControlSupport, FabricTraceContext, JobEnvelope,
    JobKind, ModelSelection, VersionedJobPayload, WorkerSchedulingState,
};
use tetonic_policy::{placement_to_dispatch_local_only, trust_permits_data_class};

use crate::worker_eligibility::{evaluate_worker_eligibility, WorkerEligibilityInput};
use crate::{aggregate_chat_classification, ChatRequest};

fn reason_to_decision(reason: PlacementReason) -> PlacementDecision {
    if placement_to_dispatch_local_only(reason.clone()) {
        PlacementDecision::LocalOnly { reason }
    } else {
        PlacementDecision::Denied { reason }
    }
}

/// Build a placement request from an outbound chat payload.
pub fn placement_request_from_chat(
    req: &ChatRequest,
    worker_id: &str,
    policy_epoch: u64,
    project_policy: ProjectPlacementPolicy,
) -> PlacementRequest {
    let data_class = aggregate_chat_classification(req)
        .map(|c| c.class)
        .or_else(|| req.fabric.as_ref().map(|f| f.data_class))
        .unwrap_or(DataClass::RepositorySource);
    PlacementRequest {
        run_id: req
            .fabric
            .as_ref()
            .and_then(|meta| meta.run_id.as_deref())
            .map(tetonic_domain::RunId::new),
        task_id: req
            .fabric
            .as_ref()
            .and_then(|meta| meta.task_id.as_deref())
            .map(tetonic_domain::TaskId::new),
        attempt_id: req
            .fabric
            .as_ref()
            .and_then(|meta| meta.attempt_id.as_deref())
            .map(tetonic_domain::AttemptId::new),
        job_kind: PlacementJobKind::Infer,
        data_class,
        input_artifacts: req
            .fabric
            .as_ref()
            .map(|meta| meta.input_artifacts.clone())
            .unwrap_or_default(),
        workspace_version: req
            .fabric
            .as_ref()
            .and_then(|meta| meta.workspace_version.clone()),
        required_capabilities: req
            .fabric
            .as_ref()
            .map(|meta| meta.required_capabilities.clone())
            .unwrap_or_default(),
        candidate_worker: Some(WorkerId::new(worker_id)),
        policy_epoch,
        required_sandbox: None,
        verification_policy: req
            .fabric
            .as_ref()
            .and_then(|meta| meta.verification_policy.as_ref())
            .map(|p| tetonic_domain::VerificationPolicyReference {
                policy_id: p.clone(),
            })
            .or_else(|| {
                project_policy.required_verification.as_ref().map(|p| {
                    tetonic_domain::VerificationPolicyReference {
                        policy_id: p.clone(),
                    }
                })
            }),
        project_policy,
        trace_context: req
            .fabric
            .as_ref()
            .map(|meta| meta.trace_context.clone())
            .unwrap_or_default(),
    }
}

/// Build a complete placement request for a typed fabric job. Unlike the legacy
/// chat adapter, this preserves run/task/attempt identity, artifact digests,
/// workspace version, capability requirements, verification policy, and trace.
pub fn placement_request_from_job(
    job: &JobEnvelope,
    worker_id: &str,
    policy_epoch: u64,
    project_policy: ProjectPlacementPolicy,
    trace: &FabricTraceContext,
    required_sandbox: Option<SandboxRequirements>,
) -> PlacementRequest {
    let job_kind = match job.job_kind {
        JobKind::Infer => PlacementJobKind::Infer,
        JobKind::Embed => PlacementJobKind::Embed,
        JobKind::AnalyzeCode
        | JobKind::IndexShard
        | JobKind::TestShard
        | JobKind::ReviewArtifact => PlacementJobKind::Compute,
    };
    let mut required_capabilities = job.required_capabilities.tools.clone();
    required_capabilities.extend(
        job.required_capabilities
            .environment
            .iter()
            .filter(|c| *c != NETWORK_DENY_ALL_CAPABILITY)
            .cloned(),
    );
    PlacementRequest {
        run_id: Some(job.run_id.clone()),
        task_id: Some(job.task_id.clone()),
        attempt_id: Some(job.attempt_id.clone()),
        job_kind,
        data_class: job.data_class,
        input_artifacts: job
            .input_artifacts
            .iter()
            .map(|artifact| tetonic_domain::ArtifactRef {
                artifact_id: artifact.artifact_id.clone(),
                digest: artifact.digest.0.clone(),
            })
            .collect(),
        workspace_version: job.workspace_version.clone(),
        required_capabilities,
        candidate_worker: Some(WorkerId::new(worker_id)),
        project_policy,
        policy_epoch,
        required_sandbox,
        verification_policy: job.verification_policy.require_verification.then(|| {
            tetonic_domain::VerificationPolicyReference {
                policy_id: job
                    .verification_policy
                    .policy_id
                    .clone()
                    .unwrap_or_else(|| "required".into()),
            }
        }),
        trace_context: tetonic_domain::TraceContext {
            trace_id: trace.trace_id.clone(),
            span_id: trace.span_id.clone(),
            run_id: Some(job.run_id.clone()),
            task_id: Some(job.task_id.clone()),
            attempt_id: Some(job.attempt_id.clone()),
            scheduler_decision_id: trace.scheduler_decision_id.clone(),
        },
    }
}

fn scheduling_reason(
    registry: &CapabilityRegistry,
    worker_id: &str,
    now: DateTime<Utc>,
) -> Option<PlacementReason> {
    match registry.scheduling_state(worker_id, now) {
        WorkerSchedulingState::NotCached => Some(PlacementReason::CapabilityUnavailable),
        WorkerSchedulingState::Quarantined => Some(PlacementReason::WorkerQuarantined),
        WorkerSchedulingState::Expired => Some(PlacementReason::CapabilityStale),
        WorkerSchedulingState::Draining => Some(PlacementReason::CapabilityUnavailable),
        WorkerSchedulingState::Degraded | WorkerSchedulingState::Eligible => None,
    }
}

/// Dispatch-time placement for typed embed/compute/artifact-bearing jobs.
///
/// This is intentionally separate from transport send: callers must invoke it
/// immediately before dispatch and may not cache an `Eligible` result.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_typed_job_placement(
    job: &JobEnvelope,
    worker_id: &str,
    worker_trust: WorkerTrust,
    policy_epoch: u64,
    project_policy: ProjectPlacementPolicy,
    trace: &FabricTraceContext,
    required_sandbox: Option<SandboxRequirements>,
    registry: &CapabilityRegistry,
    now: DateTime<Utc>,
) -> PlacementDecision {
    let required_sandbox = merge_sandbox_requirements(job, required_sandbox);
    let request = placement_request_from_job(
        job,
        worker_id,
        policy_epoch,
        project_policy,
        trace,
        required_sandbox,
    );
    let trust_decision = evaluate_placement(&request, worker_trust, None, None, now);
    if !trust_decision.allows_remote() {
        return trust_decision;
    }
    if let Some(reason) = scheduling_reason(registry, worker_id, now) {
        return reason_to_decision(reason);
    }
    let Some(caps) = registry.schedulable(worker_id, now, false) else {
        return reason_to_decision(PlacementReason::CapabilityStale);
    };
    if caps.revocation_epoch < policy_epoch {
        return reason_to_decision(PlacementReason::WorkerRevoked);
    }
    if job.job_kind == JobKind::Infer {
        let VersionedJobPayload::V1Infer(payload) = &job.payload else {
            return reason_to_decision(PlacementReason::CapabilityUnavailable);
        };
        let Some(model_name) = payload.get("model").and_then(|value| value.as_str()) else {
            return reason_to_decision(PlacementReason::CapabilityUnavailable);
        };
        let model = ModelSelection::from_request(
            model_name,
            payload.get("model_digest").and_then(|value| value.as_str()),
        );
        if !model_inventory_matches(&caps.model_inventory, &model) {
            return reason_to_decision(PlacementReason::CapabilityUnavailable);
        }
        let selected_model = caps.model_inventory.iter().find(|candidate| {
            candidate.local_name == model.local_name
                && model
                    .digest
                    .as_ref()
                    .is_none_or(|digest| candidate.model_digest.as_ref() == Some(digest))
        });
        let tools_requested = payload
            .get("tools")
            .and_then(|tools| tools.as_array())
            .is_some_and(|tools| !tools.is_empty());
        if tools_requested && selected_model.is_none_or(|model| !model.tool_call_support) {
            return reason_to_decision(PlacementReason::CapabilityUnavailable);
        }
    }
    let Some(job_capability) = caps
        .supported_job_types
        .iter()
        .find(|capability| capability.job_kind == job.job_kind)
    else {
        return reason_to_decision(PlacementReason::CapabilityUnavailable);
    };
    let input_bytes = serde_json::to_vec(&job.payload)
        .map(|payload| payload.len() as u64)
        .unwrap_or(u64::MAX);
    if input_bytes > job_capability.maximum_input_bytes
        || input_bytes > caps.limits.max_input_bytes
        || job.input_artifacts.len() as u32 > job_capability.maximum_artifacts
        || job.output_limits.max_bytes > job_capability.maximum_output_bytes
        || job.output_limits.max_bytes > caps.limits.max_output_bytes
        || job.output_limits.max_artifacts > job_capability.maximum_artifacts
    {
        return reason_to_decision(PlacementReason::InputTooLarge);
    }
    if let Some(required_memory) = job.resource_limits.max_memory_bytes {
        if caps
            .runtime_capacity
            .available_ram_bytes
            .or(caps.hardware.available_ram_bytes)
            .or(caps.hardware.total_ram_bytes)
            .is_none_or(|available| available < required_memory)
        {
            return reason_to_decision(PlacementReason::CapabilityUnavailable);
        }
        if caps.sandbox.memory_limits != ControlSupport::Enforced {
            return reason_to_decision(PlacementReason::SandboxInsufficient);
        }
    }
    if job.resource_limits.max_cpu_millis.is_some()
        && caps.sandbox.runtime_limits != ControlSupport::Enforced
    {
        return reason_to_decision(PlacementReason::SandboxInsufficient);
    }
    if request.required_capabilities.iter().any(|required| {
        !caps
            .supported_features
            .features
            .iter()
            .any(|feature| feature == required)
    }) {
        return reason_to_decision(PlacementReason::CapabilityUnavailable);
    }
    if let Some(sandbox) = &request.required_sandbox {
        if !sandbox_satisfied(sandbox, registry, worker_id, now) {
            return reason_to_decision(PlacementReason::SandboxInsufficient);
        }
    }
    trust_decision
}

fn sandbox_satisfied(
    required: &SandboxRequirements,
    registry: &CapabilityRegistry,
    worker_id: &str,
    now: DateTime<Utc>,
) -> bool {
    let Some(caps) = registry.schedulable(worker_id, now, false) else {
        return false;
    };
    if required.process_tree_enforced && caps.sandbox.process_tree != ControlSupport::Enforced {
        return false;
    }
    if required.network_denial_enforced && caps.sandbox.network_denial != ControlSupport::Enforced {
        return false;
    }
    true
}

/// Capability label that means the job needs OS-enforced network DenyAll.
pub const NETWORK_DENY_ALL_CAPABILITY: &str = "network_deny_all";

fn job_requests_network_deny_all(job: &JobEnvelope) -> bool {
    job.required_capabilities
        .environment
        .iter()
        .chain(job.required_capabilities.tools.iter())
        .any(|c| c == NETWORK_DENY_ALL_CAPABILITY)
}

fn merge_sandbox_requirements(
    job: &JobEnvelope,
    explicit: Option<SandboxRequirements>,
) -> Option<SandboxRequirements> {
    let wants_net = job_requests_network_deny_all(job);
    if explicit.is_none() && !wants_net {
        return None;
    }
    let mut req = explicit.unwrap_or_default();
    if wants_net {
        req.network_denial_enforced = true;
    }
    Some(req)
}

/// Evaluate placement for a candidate worker. Does not dispatch work.
pub fn evaluate_placement(
    request: &PlacementRequest,
    worker_trust: WorkerTrust,
    registry: Option<&CapabilityRegistry>,
    model: Option<&ModelSelection>,
    now: DateTime<Utc>,
) -> PlacementDecision {
    let required_verification = request
        .verification_policy
        .as_ref()
        .map(|policy| VerificationRequirement::Required {
            policy_id: Some(policy.policy_id.clone()),
        })
        .unwrap_or(VerificationRequirement::None);
    if request.data_class == DataClass::Secret {
        return PlacementDecision::LocalOnly {
            reason: PlacementReason::SecretLocalOnly,
        };
    }

    if let Err(reason) =
        trust_permits_data_class(worker_trust, request.data_class, &request.project_policy)
    {
        return reason_to_decision(reason);
    }

    let Some(worker_id) = request.candidate_worker.as_ref() else {
        return PlacementDecision::Eligible {
            targets: vec![],
            required_verification,
        };
    };

    if let (Some(reg), Some(model)) = (registry, model) {
        if let Some(sandbox) = &request.required_sandbox {
            if !sandbox_satisfied(sandbox, reg, worker_id.0.as_str(), now) {
                return reason_to_decision(PlacementReason::SandboxInsufficient);
            }
        }
        let input = WorkerEligibilityInput {
            worker_id: worker_id.0.as_str(),
            trust: worker_trust,
            data_class: request.data_class,
            project_policy: &request.project_policy,
            model,
            coordinator_policy_epoch: request.policy_epoch,
        };
        if let Err(reason) = evaluate_worker_eligibility(reg, &input, now) {
            return reason_to_decision(reason);
        }
    }

    PlacementDecision::Eligible {
        targets: vec![EligibleTarget {
            worker_id: worker_id.clone(),
            trust: worker_trust,
        }],
        required_verification,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use tetonic_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId, WorkerId};
    use tetonic_domain::workspace::ContentDigest;
    use tetonic_fabric_protocol::{
        IdempotencyKey, JobDeadlines, OutputLimits, VersionedJobPayload, WorkerCapabilities,
    };

    use super::*;
    use tetonic_fabric_protocol::CapabilityRegistry;

    fn request(class: DataClass, worker: Option<&str>, epoch: u64) -> PlacementRequest {
        PlacementRequest {
            run_id: None,
            task_id: None,
            attempt_id: None,
            job_kind: tetonic_domain::PlacementJobKind::Infer,
            data_class: class,
            input_artifacts: Vec::new(),
            workspace_version: None,
            required_capabilities: Vec::new(),
            candidate_worker: worker.map(WorkerId::new),
            project_policy: ProjectPlacementPolicy::default(),
            policy_epoch: epoch,
            required_sandbox: None,
            verification_policy: None,
            trace_context: tetonic_domain::TraceContext::default(),
        }
    }

    fn seed(reg: &mut CapabilityRegistry, worker_id: &str, revocation_epoch: u64) {
        let wid = WorkerId::new(worker_id);
        let now = Utc::now();
        let mut caps = WorkerCapabilities::legacy_infer_profile(
            wid.clone(),
            "boot",
            1,
            revocation_epoch,
            &["qwen:7b".into()],
            &["qwen:7b".into()],
            8192,
            4096,
            0,
            0,
            2,
        );
        caps.generated_at = now;
        caps.valid_until = now + Duration::hours(1);
        reg.upsert_validated(caps, &wid, 0, now).unwrap();
    }

    #[test]
    fn secret_always_local_only() {
        let d = evaluate_placement(
            &request(DataClass::Secret, Some("w1"), 0),
            WorkerTrust::OwnerControlledEstate,
            None,
            None,
            Utc::now(),
        );
        assert!(matches!(
            d,
            PlacementDecision::LocalOnly {
                reason: PlacementReason::SecretLocalOnly
            }
        ));
    }

    #[test]
    fn sensitive_to_owner_estate_eligible() {
        let d = evaluate_placement(
            &request(DataClass::SensitiveSource, Some("w1"), 0),
            WorkerTrust::OwnerControlledEstate,
            None,
            None,
            Utc::now(),
        );
        assert!(d.allows_remote());
    }

    #[test]
    fn sensitive_to_external_denied() {
        let d = evaluate_placement(
            &request(DataClass::SensitiveSource, Some("w1"), 0),
            WorkerTrust::ExternalUntrusted,
            None,
            None,
            Utc::now(),
        );
        assert!(matches!(
            d,
            PlacementDecision::LocalOnly {
                reason: PlacementReason::WorkerTrustInsufficient
            }
        ));
    }

    #[test]
    fn worker_capability_trust_evaluated_independently() {
        let mut reg = CapabilityRegistry::new();
        seed(&mut reg, "w1", 0);
        let model = ModelSelection::from_request("qwen:7b", None);
        // Trust permits, capability matches.
        let d = evaluate_placement(
            &request(DataClass::RepositorySource, Some("w1"), 0),
            WorkerTrust::OwnerControlledEstate,
            Some(&reg),
            Some(&model),
            Utc::now(),
        );
        assert!(d.allows_remote());
        // Same capability doc cannot override insufficient trust.
        let d = evaluate_placement(
            &request(DataClass::SensitiveSource, Some("w1"), 0),
            WorkerTrust::ExternalUntrusted,
            Some(&reg),
            Some(&model),
            Utc::now(),
        );
        assert!(!d.allows_remote());
    }

    #[test]
    fn stale_policy_epoch_blocks_eligibility() {
        let mut reg = CapabilityRegistry::new();
        seed(&mut reg, "w1", 2);
        let model = ModelSelection::from_request("qwen:7b", None);
        let d = evaluate_placement(
            &request(DataClass::RepositorySource, Some("w1"), 5),
            WorkerTrust::OwnerControlledEstate,
            Some(&reg),
            Some(&model),
            Utc::now(),
        );
        assert!(matches!(
            d,
            PlacementDecision::Denied {
                reason: PlacementReason::WorkerRevoked
            }
        ));
    }

    #[test]
    fn project_policy_cannot_override_secret() {
        let project = ProjectPlacementPolicy {
            allow_sensitive_to_owner_estate: true,
            ..ProjectPlacementPolicy::default()
        };
        let req = PlacementRequest {
            run_id: None,
            task_id: None,
            attempt_id: None,
            job_kind: tetonic_domain::PlacementJobKind::Infer,
            data_class: DataClass::Secret,
            input_artifacts: Vec::new(),
            workspace_version: None,
            required_capabilities: Vec::new(),
            candidate_worker: Some(WorkerId::new("w1")),
            project_policy: project,
            policy_epoch: 0,
            required_sandbox: None,
            verification_policy: None,
            trace_context: tetonic_domain::TraceContext::default(),
        };
        let d = evaluate_placement(&req, WorkerTrust::ExternalUntrusted, None, None, Utc::now());
        assert!(matches!(
            d,
            PlacementDecision::LocalOnly {
                reason: PlacementReason::SecretLocalOnly
            }
        ));
    }

    #[test]
    fn required_sandbox_blocks_when_not_enforced() {
        use tetonic_domain::placement::SandboxRequirements;

        let mut reg = CapabilityRegistry::new();
        seed(&mut reg, "w1", 0);
        let model = ModelSelection::from_request("qwen:7b", None);
        let req = PlacementRequest {
            run_id: None,
            task_id: None,
            attempt_id: None,
            job_kind: tetonic_domain::PlacementJobKind::Compute,
            data_class: DataClass::RepositorySource,
            input_artifacts: Vec::new(),
            workspace_version: None,
            required_capabilities: Vec::new(),
            candidate_worker: Some(WorkerId::new("w1")),
            project_policy: ProjectPlacementPolicy::default(),
            policy_epoch: 0,
            required_sandbox: Some(SandboxRequirements {
                process_tree_enforced: true,
                network_denial_enforced: false,
            }),
            verification_policy: None,
            trace_context: tetonic_domain::TraceContext::default(),
        };
        let d = evaluate_placement(
            &req,
            WorkerTrust::OwnerControlledEstate,
            Some(&reg),
            Some(&model),
            Utc::now(),
        );
        assert!(matches!(
            d,
            PlacementDecision::Denied {
                reason: PlacementReason::SandboxInsufficient
            }
        ));
    }

    #[test]
    fn required_network_denial_skips_when_unsupported() {
        use tetonic_domain::placement::SandboxRequirements;
        use tetonic_fabric_protocol::{SandboxCapabilities, WorkerCapabilities};

        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("w1");
        let now = Utc::now();
        let mut caps = WorkerCapabilities::typed_infer_profile(
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
            1,
        );
        caps.generated_at = now;
        caps.valid_until = now + chrono::Duration::hours(1);
        caps.sandbox = SandboxCapabilities {
            network_denial: ControlSupport::Unsupported,
            ..SandboxCapabilities::default()
        };
        reg.upsert_validated(caps, &wid, 0, now).unwrap();

        let mut job = typed_job(JobKind::Infer, DataClass::RepositorySource);
        job.input_artifacts.clear();
        job.payload = VersionedJobPayload::V1Infer(serde_json::json!({
            "model": "qwen:7b",
            "messages": []
        }));
        job.output_limits.max_artifacts = 0;
        job.required_capabilities.environment = vec!["network_deny_all".into()];

        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            Some(SandboxRequirements {
                process_tree_enforced: false,
                network_denial_enforced: true,
            }),
            &reg,
            now,
        );
        assert!(
            matches!(
                decision,
                PlacementDecision::Denied {
                    reason: PlacementReason::SandboxInsufficient
                }
            ),
            "expected sandbox insufficient, got {decision:?}"
        );
    }

    fn typed_job(kind: JobKind, class: DataClass) -> JobEnvelope {
        JobEnvelope {
            job_id: JobId::new("job_1"),
            run_id: RunId::new("run_1"),
            task_id: TaskId::new("task_1"),
            task_version: 1,
            attempt_id: AttemptId::new("attempt_1"),
            idempotency_key: IdempotencyKey("idem_1".into()),
            lease_id: LeaseId::new("lease_1"),
            lease_epoch: 1,
            job_kind: kind,
            input_digest: ContentDigest::new("sha256:input"),
            workspace_version: None,
            input_artifacts: vec![tetonic_fabric_protocol::ArtifactReference {
                artifact_id: "artifact_1".into(),
                digest: ContentDigest::new("sha256:artifact"),
            }],
            data_class: class,
            required_capabilities: Default::default(),
            resource_limits: Default::default(),
            deadlines: JobDeadlines::from_execution(Utc::now() + Duration::hours(1), 3600),
            output_limits: OutputLimits {
                max_bytes: 1024,
                max_artifacts: 1,
                max_artifact_size: 1024,
            },
            verification_policy: Default::default(),
            payload: VersionedJobPayload::V1Embed(serde_json::json!({"input": "text"})),
        }
    }

    #[test]
    fn typed_job_request_preserves_identity_and_artifact_digest() {
        let job = typed_job(JobKind::Embed, DataClass::RepositorySource);
        let trace = FabricTraceContext {
            trace_id: "trace_1".into(),
            span_id: "span_1".into(),
            scheduler_decision_id: None,
        };
        let request = placement_request_from_job(
            &job,
            "w1",
            7,
            ProjectPlacementPolicy::default(),
            &trace,
            None,
        );
        assert_eq!(request.run_id, Some(RunId::new("run_1")));
        assert_eq!(request.task_id, Some(TaskId::new("task_1")));
        assert_eq!(request.attempt_id, Some(AttemptId::new("attempt_1")));
        assert_eq!(request.input_artifacts[0].digest, "sha256:artifact");
        assert_eq!(request.trace_context.trace_id, "trace_1");
    }

    #[test]
    fn secret_typed_job_is_local_only_before_capability_checks() {
        let job = typed_job(JobKind::Embed, DataClass::Secret);
        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            None,
            &CapabilityRegistry::new(),
            Utc::now(),
        );
        assert!(matches!(
            decision,
            PlacementDecision::LocalOnly {
                reason: PlacementReason::SecretLocalOnly
            }
        ));
    }

    #[test]
    fn typed_job_requires_advertised_job_kind() {
        let mut registry = CapabilityRegistry::new();
        seed(&mut registry, "w1", 0); // Legacy profile advertises Infer only.
        let job = typed_job(JobKind::Embed, DataClass::RepositorySource);
        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            None,
            &registry,
            Utc::now(),
        );
        assert!(matches!(
            decision,
            PlacementDecision::Denied {
                reason: PlacementReason::CapabilityUnavailable
            }
        ));
    }

    #[test]
    fn typed_job_preserves_required_result_verification() {
        let mut registry = CapabilityRegistry::new();
        seed(&mut registry, "w1", 0);
        let mut job = typed_job(JobKind::Infer, DataClass::RepositorySource);
        job.input_artifacts.clear();
        job.payload = VersionedJobPayload::V1Infer(serde_json::json!({
            "model": "qwen:7b",
            "messages": []
        }));
        job.output_limits.max_artifacts = 0;
        job.verification_policy.require_verification = true;
        job.verification_policy.policy_id = Some("local-review".into());
        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            None,
            &registry,
            Utc::now(),
        );
        assert!(matches!(
            decision,
            PlacementDecision::Eligible {
                required_verification: VerificationRequirement::Required {
                    policy_id: Some(ref policy)
                },
                ..
            } if policy == "local-review"
        ));
    }

    #[test]
    fn typed_infer_requires_advertised_model_digest() {
        let mut registry = CapabilityRegistry::new();
        seed(&mut registry, "w1", 0);
        let mut job = typed_job(JobKind::Infer, DataClass::RepositorySource);
        job.input_artifacts.clear();
        job.output_limits.max_artifacts = 0;
        job.payload = VersionedJobPayload::V1Infer(serde_json::json!({
            "model": "qwen:7b",
            "model_digest": "sha256:not-advertised",
            "messages": []
        }));
        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            None,
            &registry,
            Utc::now(),
        );
        assert!(matches!(
            decision,
            PlacementDecision::Denied {
                reason: PlacementReason::CapabilityUnavailable
            }
        ));
    }

    #[test]
    fn typed_infer_requires_model_tool_support_when_tools_are_sent() {
        let mut registry = CapabilityRegistry::new();
        seed(&mut registry, "w1", 0); // Legacy profile advertises no tool-call support.
        let mut job = typed_job(JobKind::Infer, DataClass::RepositorySource);
        job.input_artifacts.clear();
        job.output_limits.max_artifacts = 0;
        job.payload = VersionedJobPayload::V1Infer(serde_json::json!({
            "model": "qwen:7b",
            "messages": [],
            "tools": [{"name": "read_file"}]
        }));
        let decision = evaluate_typed_job_placement(
            &job,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &FabricTraceContext {
                trace_id: "trace".into(),
                span_id: "span".into(),
                scheduler_decision_id: None,
            },
            None,
            &registry,
            Utc::now(),
        );
        assert!(matches!(
            decision,
            PlacementDecision::Denied {
                reason: PlacementReason::CapabilityUnavailable
            }
        ));
    }
}
