//! Coordinator-side lifecycle operations (M5-1).

use chrono::{DateTime, Utc};
use lokai_domain::{ProjectPlacementPolicy, WorkerTrust};
use lokai_fabric_protocol::{
    CancellationRequest, CapabilityRegistry, FabricError, FabricErrorCode, FabricTraceContext,
    Heartbeat, JobEnvelope, LeaseRenewalRequest, WorkerCapabilityAdvertisement,
};
use lokai_inference::evaluate_typed_job_placement;

use crate::protocol::map_protocol_err;
use crate::FabricClientError;

/// Observe a worker heartbeat envelope; rejects oversize/unbounded fields.
pub fn observe_heartbeat(
    hb: &Heartbeat,
    channel_worker_id: &lokai_domain::ids::WorkerId,
    limits: &lokai_fabric_protocol::MessageSizeLimits,
) -> Result<(), FabricClientError> {
    lokai_fabric_protocol::validate_heartbeat(hb, channel_worker_id, limits)
        .map_err(map_protocol_err)
}

/// Request lease renewal when the worker advertises lease support.
#[allow(clippy::too_many_arguments)]
pub fn request_lease_renewal(
    caps: &WorkerCapabilityAdvertisement,
    req: &LeaseRenewalRequest,
    job: &JobEnvelope,
    worker_trust: WorkerTrust,
    policy_epoch: u64,
    project_policy: ProjectPlacementPolicy,
    registry: &CapabilityRegistry,
    now: DateTime<Utc>,
) -> Result<(), FabricClientError> {
    if !caps.supports_leases || caps.legacy_v1_chat_only {
        return Err(FabricClientError::Protocol(FabricError {
            code: FabricErrorCode::CapabilityUnavailable,
            message: "worker does not support lease renewal".into(),
            details: None,
        }));
    }
    lokai_fabric_protocol::validate_lifecycle_context(&req.context, &req.context.worker_id)
        .map_err(map_protocol_err)?;
    if req.context.job_id != job.job_id
        || req.context.task_id != job.task_id
        || req.context.attempt_id != job.attempt_id
        || req.context.lease_id != job.lease_id
        || req.context.lease_epoch != job.lease_epoch
    {
        return Err(FabricClientError::Protocol(FabricError {
            code: FabricErrorCode::InvalidLease,
            message: "lease renewal does not match the placed job".into(),
            details: None,
        }));
    }
    let trace = FabricTraceContext::default();
    let decision = evaluate_typed_job_placement(
        job,
        &req.context.worker_id.to_string(),
        worker_trust,
        policy_epoch,
        project_policy,
        &trace,
        None,
        registry,
        now,
    );
    if !decision.allows_remote() {
        return Err(FabricClientError::Protocol(FabricError {
            code: FabricErrorCode::PolicyDenied,
            message: format!("lease renewal placement denied: {decision:?}"),
            details: None,
        }));
    }
    Ok(())
}

/// Deliver cancellation to a worker when supported.
pub fn deliver_cancellation(
    caps: &WorkerCapabilityAdvertisement,
    cancel: &CancellationRequest,
) -> Result<(), FabricClientError> {
    if !caps.supports_cancellation {
        return Err(FabricClientError::Protocol(FabricError {
            code: FabricErrorCode::CapabilityUnavailable,
            message: "worker does not support cancellation delivery".into(),
            details: None,
        }));
    }
    lokai_fabric_protocol::validate_cancellation(cancel).map_err(map_protocol_err)
}

/// Retry-safe idempotent dispatch marker for coordinator-side dedup bookkeeping.
pub fn retry_safe_delivery_allowed(
    caps: &WorkerCapabilityAdvertisement,
    attempt: u32,
    max_retries: u32,
) -> bool {
    attempt <= max_retries && (caps.supports_leases || caps.legacy_v1_chat_only)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::ids::{AttemptId, JobId, LeaseId, TaskId, WorkerId};
    use lokai_fabric_protocol::{IdempotencyKey, LifecycleContext, ProtocolVersion};

    #[test]
    fn legacy_cancellation_delivery_rejected() {
        let caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
        let cancel = CancellationRequest {
            run_id: lokai_domain::ids::RunId::new("run"),
            task_id: TaskId::new("task"),
            attempt_id: AttemptId::new("att"),
            lease_id: LeaseId::new("lease"),
            lease_epoch: 1,
            reason: "stop".into(),
            deadline: chrono::Utc::now(),
        };
        assert!(deliver_cancellation(&caps, &cancel).is_err());
    }

    #[test]
    fn lease_renewal_rejected_for_legacy_worker() {
        let caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
        let job = crate::build_infer_job_envelope(
            &crate::LegacyInferParams {
                job_id: "job".into(),
                attempt_id: "att".into(),
                data_class: lokai_domain::DataClass::RepositorySource,
                run_id: Some("run".into()),
                task_id: Some("task".into()),
            },
            &[lokai_inference::Message::user("hello")],
            "qwen:7b",
        )
        .unwrap();
        let ctx = LifecycleContext {
            job_id: JobId::new("job"),
            task_id: TaskId::new("task"),
            attempt_id: AttemptId::new("att"),
            lease_id: LeaseId::new("lease"),
            lease_epoch: 1,
            worker_id: WorkerId::new("worker"),
            sequence_number: 1,
            protocol_version: ProtocolVersion(1),
            idempotency_key: IdempotencyKey("idem".into()),
        };
        let req = LeaseRenewalRequest {
            context: ctx,
            requested_expires_at: chrono::Utc::now(),
        };
        assert!(request_lease_renewal(
            &caps,
            &req,
            &job,
            WorkerTrust::OwnerControlledEstate,
            0,
            ProjectPlacementPolicy::default(),
            &CapabilityRegistry::new(),
            Utc::now(),
        )
        .is_err());
    }

    #[test]
    fn lease_renewal_rechecks_trust_after_downgrade() {
        let mut caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
        caps.supports_leases = true;
        caps.legacy_v1_chat_only = false;
        let mut job = crate::build_infer_job_envelope(
            &crate::LegacyInferParams {
                job_id: "job".into(),
                attempt_id: "att".into(),
                data_class: lokai_domain::DataClass::SensitiveSource,
                run_id: Some("run".into()),
                task_id: Some("task".into()),
            },
            &[lokai_inference::Message::user("sensitive source")],
            "qwen:7b",
        )
        .unwrap();
        job.data_class = lokai_domain::DataClass::SensitiveSource;
        let worker_id = WorkerId::new("worker");
        let now = Utc::now();
        let mut worker_caps = lokai_fabric_protocol::WorkerCapabilities::legacy_infer_profile(
            worker_id.clone(),
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
        worker_caps.generated_at = now;
        worker_caps.valid_until = now + chrono::Duration::hours(1);
        let mut registry = CapabilityRegistry::new();
        registry
            .upsert_validated(worker_caps, &worker_id, 0, now)
            .unwrap();
        let req = LeaseRenewalRequest {
            context: LifecycleContext {
                job_id: JobId::new("job"),
                task_id: TaskId::new("task"),
                attempt_id: AttemptId::new("att"),
                lease_id: LeaseId::new("att"),
                lease_epoch: 1,
                worker_id,
                sequence_number: 1,
                protocol_version: ProtocolVersion(1),
                idempotency_key: IdempotencyKey("idem".into()),
            },
            requested_expires_at: now + chrono::Duration::minutes(1),
        };
        assert!(request_lease_renewal(
            &caps,
            &req,
            &job,
            WorkerTrust::ExternalUntrusted,
            0,
            ProjectPlacementPolicy::default(),
            &registry,
            now,
        )
        .is_err());
    }
}
