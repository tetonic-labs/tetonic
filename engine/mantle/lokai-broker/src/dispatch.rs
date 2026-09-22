//! Pre-dispatch revalidation gates (M6-1).

use chrono::{DateTime, Utc};
use lokai_domain::ids::WorkerId;
use lokai_domain::TrustPlacementDecision;
use lokai_fabric_protocol::{CapabilityRegistry, JobKind, WorkerSchedulingState};
use lokai_inference::LOCAL_NODE_ID;

use crate::types::{ComputeBrokerError, ComputeRequest};

fn is_local_worker_id(id: &str) -> bool {
    id == LOCAL_NODE_ID || id.eq_ignore_ascii_case("local")
}

/// Job kinds that execute as a sandboxed process under ProcessAuthority.
fn is_process_class(kind: &JobKind) -> bool {
    matches!(kind, JobKind::IndexShard | JobKind::TestShard)
}

/// V3 implements WorkerTarget for **Infer only** — `job_ingress.rs` answers every other
/// kind with `UnsupportedJobKind`. Process-class kinds are refused here so they are not
/// silently downgraded to local execution (INV-EXEC-002). Non-process kinds not in the
/// cached `supported_job_types` advertisement are refused at dispatch (C-4 / ads ≤ enforce).
fn refuse_process_on_worker_target(
    request: &ComputeRequest,
    worker_id: &WorkerId,
) -> Result<(), ComputeBrokerError> {
    if is_process_class(&request.job_kind) {
        return Err(ComputeBrokerError::WorkerTargetRefused {
            job_kind: format!("{:?}", request.job_kind),
            worker_id: worker_id.0.clone(),
        });
    }
    Ok(())
}

/// Re-check placement expiry and, when a remote worker is targeted, capability freshness.
///
/// Local Infer (`node_local` / `LocalOnly`) is not advertised in the remote
/// capability registry. A down enrolled worker must not fail-close that path —
/// scheduler already selected local; this gate must let loopback Ollama run.
pub fn revalidate_before_dispatch(
    request: &ComputeRequest,
    caps: Option<&CapabilityRegistry>,
    now: DateTime<Utc>,
) -> Result<(), ComputeBrokerError> {
    if request.placement_decision.expires_at <= now {
        return Err(ComputeBrokerError::PlacementExpired);
    }
    if matches!(
        request.placement_decision.decision,
        TrustPlacementDecision::Denied { .. }
    ) {
        return Err(ComputeBrokerError::PlacementExpired);
    }

    // INV-EXEC-002 is decided on the target as requested, before `target_worker_id` launders
    // local-sentinel names into "no target at all". Otherwise a worker enrolled as
    // `node_local` would be erased here and walk straight past the refusal. The local process
    // path never names a target — it sends `target_worker_id: None` with `LocalOnly` — so a
    // named target always means "not the LocalTarget".
    if let Some(named) = requested_target(request) {
        refuse_process_on_worker_target(request, &named)?;
    }

    let Some(worker_id) = target_worker_id(request) else {
        return Ok(());
    };

    let Some(registry) = caps else {
        // Remote target without a capability cache cannot be revalidated — fail closed.
        return Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "no capability registry for remote dispatch".into(),
        });
    };

    match registry.scheduling_state(&worker_id.0, now) {
        WorkerSchedulingState::Eligible => {
            let Some(caps) = registry.get(&worker_id.0) else {
                return Err(ComputeBrokerError::CapabilityStale {
                    worker_id: worker_id.0.clone(),
                    detail: "worker capabilities not cached".into(),
                });
            };
            if !caps.supports_job_kind(&request.job_kind) {
                return Err(ComputeBrokerError::WorkerTargetRefused {
                    job_kind: format!("{:?}", request.job_kind),
                    worker_id: worker_id.0.clone(),
                });
            }
            Ok(())
        }
        WorkerSchedulingState::Expired => Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "capability advertisement expired".into(),
        }),
        WorkerSchedulingState::Degraded => Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "dynamic capacity expired or worker degraded".into(),
        }),
        WorkerSchedulingState::Quarantined => Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "worker quarantined".into(),
        }),
        WorkerSchedulingState::Draining => Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "worker draining".into(),
        }),
        WorkerSchedulingState::NotCached => Err(ComputeBrokerError::CapabilityStale {
            worker_id: worker_id.0.clone(),
            detail: "worker capabilities not cached".into(),
        }),
    }
}

/// The target exactly as the request names it, with no local-sentinel filtering.
///
/// `target_worker_id` below deliberately maps local-sentinel names to `None` so that local
/// Infer is not fail-closed against the remote capability registry. That erasure must not
/// also decide capability-matrix questions, so refusals read the raw target from here.
fn requested_target(request: &ComputeRequest) -> Option<WorkerId> {
    if let Some(id) = &request.target_worker_id {
        return Some(id.clone());
    }
    match &request.placement_decision.decision {
        TrustPlacementDecision::Eligible { targets, .. }
        | TrustPlacementDecision::EligibleAfterRedaction { targets, .. } => {
            targets.first().map(|t| t.worker_id.clone())
        }
        TrustPlacementDecision::LocalOnly { .. } | TrustPlacementDecision::Denied { .. } => None,
    }
}

fn target_worker_id(request: &ComputeRequest) -> Option<WorkerId> {
    if let Some(id) = &request.target_worker_id {
        if is_local_worker_id(&id.0) {
            return None;
        }
        return Some(id.clone());
    }
    match &request.placement_decision.decision {
        TrustPlacementDecision::Eligible { targets, .. }
        | TrustPlacementDecision::EligibleAfterRedaction { targets, .. } => targets
            .first()
            .map(|t| t.worker_id.clone())
            .filter(|id| !is_local_worker_id(&id.0)),
        TrustPlacementDecision::LocalOnly { .. } | TrustPlacementDecision::Denied { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use lokai_domain::ids::{AttemptId, RunId, TaskId};
    use lokai_domain::{
        ContentDigest, DataClass, PlacementReason, TraceContext, VerificationPolicyReference,
    };
    use lokai_fabric_protocol::{JobKind, WorkerCapabilities};

    use crate::budget::ResourceRequest;
    use crate::priority::ComputePriority;
    use crate::types::{DeadlinePolicy, PlacementDecisionReference, RetryPolicyReference};

    fn base_req(decision: TrustPlacementDecision) -> ComputeRequest {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "p".into(),
                issued_at: now,
                expires_at: now + Duration::minutes(5),
                policy_epoch: 1,
                decision,
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: TraceContext::default(),
            speculative: false,
            project_id: None,
            target_worker_id: Some(WorkerId::new("w1")),
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    }

    #[test]
    fn expired_placement_rejected() {
        let mut req = base_req(TrustPlacementDecision::LocalOnly {
            reason: PlacementReason::CapabilityUnavailable,
        });
        req.target_worker_id = None;
        req.placement_decision.expires_at = Utc::now() - Duration::seconds(1);
        assert!(matches!(
            revalidate_before_dispatch(&req, None, Utc::now()),
            Err(ComputeBrokerError::PlacementExpired)
        ));
    }

    #[test]
    fn expired_capability_rejects_remote() {
        let mut req = base_req(TrustPlacementDecision::Eligible {
            targets: vec![lokai_domain::EligibleTarget {
                worker_id: WorkerId::new("w1"),
                trust: lokai_domain::WorkerTrust::OwnerControlledEstate,
            }],
            required_verification: lokai_domain::VerificationRequirement::None,
        });
        let mut registry = CapabilityRegistry::new();
        let now = Utc::now();
        let wid = WorkerId::new("w1");
        let caps = WorkerCapabilities::legacy_infer_profile(
            wid.clone(),
            "boot",
            1,
            0,
            &["m".into()],
            &["m".into()],
            8192,
            4096,
            0,
            0,
            2,
        );
        registry
            .upsert_validated(caps, &wid, 0, now)
            .expect("insert");
        let later = now + Duration::days(2);
        req.placement_decision.expires_at = later + Duration::minutes(5);
        let result = revalidate_before_dispatch(&req, Some(&registry), later);
        assert!(
            matches!(result, Err(ComputeBrokerError::CapabilityStale { .. })),
            "result={result:?}"
        );
    }

    #[test]
    fn local_fallback_skips_remote_capability_cache() {
        let mut req = base_req(TrustPlacementDecision::LocalOnly {
            reason: PlacementReason::CapabilityUnavailable,
        });
        req.target_worker_id = Some(WorkerId::new(LOCAL_NODE_ID));
        let registry = CapabilityRegistry::new();
        revalidate_before_dispatch(&req, Some(&registry), Utc::now())
            .expect("local Infer must not require a cached remote capability advertisement");
        revalidate_before_dispatch(&req, None, Utc::now())
            .expect("local Infer must not fail closed without a capability registry");
    }

    #[test]
    fn uncached_remote_still_fail_closed() {
        let mut req = base_req(TrustPlacementDecision::Eligible {
            targets: vec![lokai_domain::EligibleTarget {
                worker_id: WorkerId::new("gpu-box"),
                trust: lokai_domain::WorkerTrust::OwnerControlledEstate,
            }],
            required_verification: lokai_domain::VerificationRequirement::None,
        });
        req.target_worker_id = Some(WorkerId::new("gpu-box"));
        let registry = CapabilityRegistry::new();
        let result = revalidate_before_dispatch(&req, Some(&registry), Utc::now());
        match result {
            Err(ComputeBrokerError::CapabilityStale { worker_id, .. }) => {
                assert_eq!(worker_id, "gpu-box");
            }
            other => panic!("expected CapabilityStale for gpu-box, got {other:?}"),
        }
    }

    #[test]
    fn named_worker_embed_refused_when_ads_are_infer_only() {
        let mut req = base_req(TrustPlacementDecision::Eligible {
            targets: vec![lokai_domain::EligibleTarget {
                worker_id: WorkerId::new("w1"),
                trust: lokai_domain::WorkerTrust::OwnerControlledEstate,
            }],
            required_verification: lokai_domain::VerificationRequirement::None,
        });
        req.job_kind = JobKind::Embed;
        req.target_worker_id = Some(WorkerId::new("w1"));
        let mut registry = CapabilityRegistry::new();
        let now = Utc::now();
        let wid = WorkerId::new("w1");
        let caps = WorkerCapabilities::legacy_infer_profile(
            wid.clone(),
            "boot",
            1,
            0,
            &["m".into()],
            &["m".into()],
            8192,
            4096,
            0,
            0,
            2,
        );
        registry
            .upsert_validated(caps, &wid, 0, now)
            .expect("insert");
        let result = revalidate_before_dispatch(&req, Some(&registry), now);
        match result {
            Err(ComputeBrokerError::WorkerTargetRefused {
                job_kind,
                worker_id,
            }) => {
                assert_eq!(job_kind, "Embed");
                assert_eq!(worker_id, "w1");
            }
            other => panic!("expected WorkerTargetRefused for Embed, got {other:?}"),
        }
    }
}
