//! M5-1 protocol conformance tests.

use chrono::{Duration, Utc};
use tetonic_domain::classify::DataClass;
use tetonic_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId, WorkerId};
use tetonic_domain::workspace::ContentDigest;
use tetonic_fabric_protocol::{
    decode_envelope_json, default_message_limits, negotiate_versions, parse_job_kind_str,
    sign_result_envelope, validate_channel_identity, validate_clock_skew, validate_heartbeat,
    validate_job_envelope, validate_mandatory_envelope_fields, validate_result_context,
    validate_result_context_audit, validate_revocation_epoch, validate_revoked,
    CancellationAcknowledged, CancellationRequest, CancellationState, CoordinatorId,
    ExecutionSummary, FabricEnvelope, FabricErrorCode, FabricMessageType, FabricTraceContext,
    Heartbeat, IdempotencyKey, IngressDecision, IngressState, JobDeadlines, JobEnvelope, JobKind,
    JobOffer, LeaseMessage, LifecycleContext, MessageId, MessageSizeLimits, ProtocolVersion,
    ResultEnvelope, ResultStatus, SignedResultBody, TerminalOutcome, VersionNegotiationRequest,
    WorkerCapabilityAdvertisement, IMMUTABLE_SECURITY_FEATURES, RESULT_ENVELOPE_VERSION,
};

fn sample_job(input_digest: &str) -> JobEnvelope {
    JobEnvelope {
        job_id: JobId::new("job_1"),
        run_id: RunId::new("run_1"),
        task_id: TaskId::new("task_1"),
        task_version: 1,
        attempt_id: AttemptId::new("att_1"),
        idempotency_key: IdempotencyKey("idem_1".into()),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        job_kind: JobKind::Infer,
        input_digest: ContentDigest::new(input_digest),
        workspace_version: None,
        input_artifacts: vec![],
        data_class: DataClass::RepositorySource,
        required_capabilities: Default::default(),
        resource_limits: Default::default(),
        deadlines: JobDeadlines::from_execution(Utc::now() + Duration::hours(1), 3600),
        output_limits: tetonic_fabric_protocol::OutputLimits {
            max_bytes: 1_000_000,
            max_artifacts: 8,
            max_artifact_size: 500_000,
        },
        verification_policy: Default::default(),
        payload: tetonic_fabric_protocol::VersionedJobPayload::V1Infer(serde_json::json!({})),
    }
}

fn sample_envelope(job: JobEnvelope) -> FabricEnvelope<JobOffer> {
    FabricEnvelope {
        protocol_version: ProtocolVersion(1),
        message_id: MessageId("msg_1".into()),
        message_type: FabricMessageType::JobOffer,
        coordinator_id: CoordinatorId("coord_1".into()),
        worker_id: WorkerId::new("worker_1"),
        sent_at: Utc::now(),
        revocation_epoch: 5,
        trace_context: FabricTraceContext {
            trace_id: "trace".into(),
            span_id: "span".into(),
            scheduler_decision_id: None,
        },
        payload: JobOffer { job },
    }
}

#[test]
fn job_artifact_references_require_digest_binding() {
    let mut job = sample_job("sha256:input");
    job.input_artifacts = vec![tetonic_fabric_protocol::ArtifactReference {
        artifact_id: "artifact_1".into(),
        digest: ContentDigest::new(""),
    }];
    let err = validate_job_envelope(&job).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InvalidEnvelope);
    assert!(err.message.contains("input_artifacts[].digest"));

    job.input_artifacts[0].digest = ContentDigest::new("sha256:artifact");
    validate_job_envelope(&job).unwrap();
}

#[test]
fn idempotency_key_conflict_across_delivery_keys() {
    let mut ingress = IngressState::default();
    let j1 = sample_job("sha256:abc");
    ingress.accept_delivery(&j1).unwrap();
    let mut j2 = sample_job("sha256:def");
    j2.attempt_id = AttemptId::new("att_2");
    j2.lease_id = LeaseId::new("lease_2");
    j2.idempotency_key = j1.idempotency_key.clone();
    let err = ingress.accept_delivery(&j2).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::DuplicateConflict);
}

#[test]
fn duplicate_in_flight_rejected() {
    let mut ingress = IngressState::default();
    let job = sample_job("sha256:inflight");
    ingress.accept_delivery(&job).unwrap();
    assert!(matches!(
        ingress.accept_delivery(&job).unwrap(),
        IngressDecision::DuplicateInFlight
    ));
}

#[test]
fn stale_lease_result_audit_only() {
    let ctx = LifecycleContext {
        job_id: JobId::new("job_1"),
        task_id: TaskId::new("task_1"),
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        worker_id: WorkerId::new("worker_1"),
        sequence_number: 1,
        protocol_version: ProtocolVersion(1),
        idempotency_key: IdempotencyKey("idem".into()),
    };
    let worker = WorkerId::new("worker_1");
    assert!(validate_result_context_audit(&ctx, &worker, 2).is_ok());
    let err = validate_result_context(&ctx, &worker, 2, false).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::StaleLeaseEpoch);
}

#[test]
fn heartbeat_bounds_enforced() {
    let ctx = LifecycleContext {
        job_id: JobId::new("job_1"),
        task_id: TaskId::new("task_1"),
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        worker_id: WorkerId::new("worker_1"),
        sequence_number: 1,
        protocol_version: ProtocolVersion(1),
        idempotency_key: IdempotencyKey("idem".into()),
    };
    let hb = Heartbeat {
        context: ctx,
        attempt_state: "x".repeat(512),
        heartbeat_sequence: 1,
        resource_usage_summary: None,
        progress_marker: None,
        current_output_size_bytes: 0,
        lease_renewal_request: false,
        worker_health_status: "ok".into(),
    };
    let err =
        validate_heartbeat(&hb, &WorkerId::new("worker_1"), &default_message_limits()).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InputTooLarge);
}

#[test]
fn result_envelope_roundtrip() {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use tetonic_domain::ids::{KeyId, ResultId};
    let sk = SigningKey::generate(&mut OsRng);
    let body = SignedResultBody {
        protocol_version: ProtocolVersion(1),
        result_envelope_version: RESULT_ENVELOPE_VERSION,
        result_id: ResultId::new("res_1"),
        worker_id: WorkerId::new("worker_1"),
        worker_key_id: KeyId::new("key_1"),
        coordinator_id: CoordinatorId("coord_1".into()),
        run_id: RunId::new("run_1"),
        job_id: JobId::new("job_1"),
        task_id: TaskId::new("task_1"),
        task_version: 1,
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        idempotency_key: IdempotencyKey("idem".into()),
        job_kind: JobKind::Infer,
        input_digest: ContentDigest::new("sha256:input"),
        workspace_version: None,
        result_status: ResultStatus::Ok,
        result_digest: ContentDigest::new("sha256:result"),
        artifacts: vec![],
        execution_summary: ExecutionSummary::default(),
        completed_at: Utc::now(),
        revocation_epoch: 3,
    };
    let env = sign_result_envelope(body, &sk, serde_json::json!({"ok": true})).unwrap();
    let json = serde_json::to_string(&env).unwrap();
    let back: ResultEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(back.body.revocation_epoch, 3);
    assert_eq!(back.signature.0.len(), 64);
}

#[test]
fn duplicate_job_delivery() {
    let mut ingress = IngressState::default();
    let job = sample_job("sha256:abc");
    assert!(matches!(
        ingress.accept_delivery(&job).unwrap(),
        IngressDecision::AcceptNew
    ));
    assert!(matches!(
        ingress.accept_delivery(&job).unwrap(),
        IngressDecision::DuplicateInFlight
    ));
    ingress
        .mark_terminal(
            &job,
            TerminalOutcome::Completed,
            Some(r#"{"ok":true}"#.into()),
        )
        .unwrap();
    assert!(matches!(
        ingress.accept_delivery(&job).unwrap(),
        IngressDecision::ReplayExisting(_)
    ));
}

#[test]
fn same_idempotency_key_different_input_digest() {
    let mut ingress = IngressState::default();
    let j1 = sample_job("sha256:abc");
    ingress.accept_delivery(&j1).unwrap();
    let mut j2 = sample_job("sha256:def");
    j2.idempotency_key = j1.idempotency_key.clone();
    let err = ingress.accept_delivery(&j2).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::DuplicateConflict);
}

#[test]
fn unsupported_protocol_version() {
    let mut env = sample_envelope(sample_job("x"));
    env.protocol_version = ProtocolVersion(99);
    let bytes = serde_json::to_vec(&env).unwrap();
    let err = decode_envelope_json::<JobOffer>(&bytes, &default_message_limits()).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::UnsupportedProtocolVersion);
}

#[test]
fn missing_mandatory_security_field() {
    let mut env = sample_envelope(sample_job("x"));
    env.trace_context.trace_id = String::new();
    let err = validate_mandatory_envelope_fields(&env).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InvalidEnvelope);
}

#[test]
fn oversized_payload() {
    let limits = MessageSizeLimits {
        max_envelope_bytes: 256,
        max_payload_bytes: 64,
        max_error_message_bytes: 128,
    };
    let env = sample_envelope(sample_job("x"));
    let bytes = serde_json::to_vec(&env).unwrap();
    let err = decode_envelope_json::<JobOffer>(&bytes, &limits).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InputTooLarge);
}

#[test]
fn stale_revocation_epoch() {
    let err = validate_revocation_epoch(3, 5).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::StaleRevocationEpoch);
}

#[test]
fn worker_identity_mismatch() {
    let channel = WorkerId::new("worker_a");
    let envelope = WorkerId::new("worker_b");
    let err = validate_channel_identity(&channel, &envelope).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::IdentityMismatch);
}

#[test]
fn lease_renewal_after_cancellation() {
    let mut cancel = CancellationState::default();
    let req = CancellationRequest {
        run_id: RunId::new("run_1"),
        task_id: TaskId::new("task_1"),
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        reason: "user cancel".into(),
        deadline: Utc::now(),
    };
    cancel
        .acknowledge(
            &req,
            CancellationAcknowledged {
                context: LifecycleContext {
                    job_id: JobId::new("job_1"),
                    task_id: TaskId::new("task_1"),
                    attempt_id: AttemptId::new("att_1"),
                    lease_id: LeaseId::new("lease_1"),
                    lease_epoch: 1,
                    worker_id: WorkerId::new("worker_1"),
                    sequence_number: 1,
                    protocol_version: ProtocolVersion(1),
                    idempotency_key: IdempotencyKey("idem".into()),
                },
            },
        )
        .unwrap();
    let err = cancel
        .can_renew_lease(&AttemptId::new("att_1"), 1)
        .unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InvalidLease);
}

#[test]
fn result_from_old_lease_epoch() {
    let ctx = LifecycleContext {
        job_id: JobId::new("job_1"),
        task_id: TaskId::new("task_1"),
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        worker_id: WorkerId::new("worker_1"),
        sequence_number: 1,
        protocol_version: ProtocolVersion(1),
        idempotency_key: IdempotencyKey("idem".into()),
    };
    let worker = WorkerId::new("worker_1");
    let err = validate_result_context(&ctx, &worker, 2, false).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::StaleLeaseEpoch);
}

#[test]
fn legacy_v1_chat_inference_capabilities() {
    let caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
    assert!(caps.legacy_v1_chat_only);
    assert!(!caps.supports_cancellation);
    assert_eq!(caps.job_kinds, vec![JobKind::Infer]);
}

#[test]
fn legacy_worker_cancellation_limitation() {
    let caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
    assert!(!caps.supports_cancellation);
}

#[test]
fn unknown_job_type() {
    let err = parse_job_kind_str("quantum_flux").unwrap_err();
    assert_eq!(err.code, FabricErrorCode::UnsupportedJobKind);
}

#[test]
fn clock_skew() {
    let now = Utc::now();
    let far = now + Duration::hours(2);
    let err = validate_clock_skew(far, now, 300).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InvalidEnvelope);
}

#[test]
fn coordinator_reconnect() {
    let mut ingress = IngressState::default();
    let job = sample_job("sha256:reconnect");
    ingress.accept_delivery(&job).unwrap();
    ingress
        .mark_terminal(
            &job,
            TerminalOutcome::Completed,
            Some(r#"{"ok":true}"#.into()),
        )
        .unwrap();
    let snapshot = ingress.records();
    let mut restored = IngressState::from_records(snapshot);
    assert_eq!(restored.len(), 1);
    assert!(matches!(
        restored.accept_delivery(&job).unwrap(),
        IngressDecision::ReplayExisting(_)
    ));
}

#[test]
fn worker_restart_with_persisted_ingress_record() {
    let mut ingress = IngressState::default();
    let job = sample_job("sha256:persist");
    ingress.accept_delivery(&job).unwrap();
    ingress
        .mark_terminal(
            &job,
            TerminalOutcome::Completed,
            Some(r#"{"ok":true}"#.into()),
        )
        .unwrap();
    let records = ingress.records();
    let mut after_restart = IngressState::from_records(records);
    assert!(matches!(
        after_restart.accept_delivery(&job).unwrap(),
        IngressDecision::ReplayExisting(_)
    ));
}

#[test]
fn negotiation_rejects_incompatible_version() {
    let req = VersionNegotiationRequest {
        min_supported_version: 99,
        max_supported_version: 100,
        required_features: IMMUTABLE_SECURITY_FEATURES
            .iter()
            .map(|s| s.to_string())
            .collect(),
        optional_features: vec![],
        software_version: "test".into(),
        message_size_limits: default_message_limits(),
    };
    let err = negotiate_versions(&req, 1, 1).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::UnsupportedProtocolVersion);
}

#[test]
fn negotiation_cannot_weaken_security_features() {
    let req = VersionNegotiationRequest {
        min_supported_version: 1,
        max_supported_version: 1,
        required_features: vec!["task_identity".into()],
        optional_features: vec![],
        software_version: "test".into(),
        message_size_limits: default_message_limits(),
    };
    let err = negotiate_versions(&req, 1, 1).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::InvalidEnvelope);
}

#[test]
fn revoked_worker_cannot_submit() {
    let err = validate_revoked(true).unwrap_err();
    assert_eq!(err.code, FabricErrorCode::RevokedIdentity);
}

#[test]
fn cancellation_messages_are_idempotent() {
    let mut cancel = CancellationState::default();
    let req = CancellationRequest {
        run_id: RunId::new("run_1"),
        task_id: TaskId::new("task_1"),
        attempt_id: AttemptId::new("att_1"),
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 1,
        reason: "stop".into(),
        deadline: Utc::now(),
    };
    let ack = CancellationAcknowledged {
        context: LifecycleContext {
            job_id: JobId::new("job_1"),
            task_id: TaskId::new("task_1"),
            attempt_id: AttemptId::new("att_1"),
            lease_id: LeaseId::new("lease_1"),
            lease_epoch: 1,
            worker_id: WorkerId::new("worker_1"),
            sequence_number: 1,
            protocol_version: ProtocolVersion(1),
            idempotency_key: IdempotencyKey("idem".into()),
        },
    };
    assert!(cancel.acknowledge(&req, ack.clone()).unwrap());
    assert!(!cancel.acknowledge(&req, ack).unwrap());
}

#[test]
fn lease_message_roundtrip() {
    let lease = LeaseMessage {
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 2,
        holder_identity: WorkerId::new("worker_1"),
        issued_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(5),
        heartbeat_interval_secs: 30,
        renewal_limit: Some(3),
    };
    let json = serde_json::to_string(&lease).unwrap();
    let back: LeaseMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.lease_epoch, 2);
}
