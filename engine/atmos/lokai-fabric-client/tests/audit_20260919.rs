use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use lokai_domain::ids::{AttemptId, JobId, KeyId, LeaseId, ResultId, RunId, TaskId, WorkerId};
use lokai_domain::result_integrity::WorkerBehaviorSignals;
use lokai_domain::workspace::ContentDigest;
use lokai_domain::{ResultDisposition, ResultVerificationRequirement};
use lokai_fabric_client::{accept_remote_result, RemoteResultAcceptRequest};
use lokai_fabric_protocol::*;
use rand::rngs::OsRng;

fn pair() -> (SigningKey, Vec<u8>, KeyId) {
    let sk = SigningKey::generate(&mut OsRng);
    let pk = sk.verifying_key().to_bytes().to_vec();
    (
        sk,
        pk.clone(),
        KeyId::new(format!("k_{}", hex::encode(&pk[..4]))),
    )
}

fn reg(keys: &mut ResultSigningKeyRegistry, worker: &str, kid: &KeyId, pk: Vec<u8>) {
    keys.register(ResultSigningKeyRecord {
        key_id: kid.clone(),
        worker_id: WorkerId::new(worker),
        public_key: pk,
        valid_from: Utc::now() - Duration::hours(1),
        valid_until: None,
        revoked: false,
        identity_certification: None,
        rotation_generation: 1,
    })
    .unwrap();
}

fn body(worker: &str, kid: &KeyId) -> SignedResultBody {
    SignedResultBody {
        protocol_version: ProtocolVersion(1),
        result_envelope_version: RESULT_ENVELOPE_VERSION,
        result_id: ResultId::new("res_adv"),
        worker_id: WorkerId::new(worker),
        worker_key_id: kid.clone(),
        coordinator_id: CoordinatorId("estate".into()),
        run_id: RunId::new("run"),
        job_id: JobId::new("job"),
        task_id: TaskId::new("task"),
        task_version: 1,
        attempt_id: AttemptId::new("att"),
        lease_id: LeaseId::new("lease"),
        lease_epoch: 1,
        idempotency_key: IdempotencyKey("idem".into()),
        job_kind: JobKind::Infer,
        input_digest: ContentDigest::new("sha256:in"),
        workspace_version: None,
        result_status: ResultStatus::Ok,
        result_digest: ContentDigest::new("pending"),
        artifacts: vec![],
        execution_summary: ExecutionSummary::default(),
        completed_at: Utc::now(),
        revocation_epoch: 0,
    }
}

fn expected() -> ExpectedResultBinding {
    ExpectedResultBinding {
        channel_worker_id: WorkerId::new("w1"),
        coordinator_id: "estate".into(),
        run_id: RunId::new("run"),
        job_id: JobId::new("job"),
        task_id: TaskId::new("task"),
        task_version: 1,
        attempt_id: AttemptId::new("att"),
        lease_id: LeaseId::new("lease"),
        lease_epoch: 1,
        input_digest: ContentDigest::new("sha256:in"),
        workspace_version: None,
        worker_revoked: false,
        attempt_canceled: false,
        attempt_superseded: false,
        another_attempt_won: false,
        run_active: true,
        task_active: true,
        known_revocation_epoch: 0,
        output_limits: OutputLimits {
            max_bytes: 1_000_000,
            max_artifacts: 4,
            max_artifact_size: 100_000,
        },
    }
}

fn signed(sk: &SigningKey, mut b: SignedResultBody, payload: serde_json::Value) -> ResultEnvelope {
    b.result_digest = digest_payload(&payload).unwrap();
    sign_result_envelope(b, sk, payload).unwrap()
}

// Rejected replays must remain rejected even when a receipt exists.
#[test]
fn accepted_result_id_cannot_bypass_current_validation() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let mut req = RemoteResultAcceptRequest {
        envelope: signed(
            &sk,
            body("w1", &kid),
            serde_json::json!({"message":"original"}),
        ),
        expected: expected(),
        verification: ResultVerificationRequirement::StructuralValidation,
        redundant: vec![],
        artifact_bytes: vec![],
        now: Utc::now(),
        run_snapshot: None,
        lease_proof: None,
        command_envelope: None,
    };
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    assert_eq!(
        accept_remote_result(&req, &keys, &mut dispositions, &mut signals).disposition,
        ResultDisposition::Accepted
    );
    req.expected.attempt_canceled = true;
    assert_eq!(
        accept_remote_result(&req, &keys, &mut dispositions, &mut signals).disposition,
        ResultDisposition::RejectedCanceled
    );
    req.expected.attempt_canceled = false;
    req.envelope.payload = serde_json::json!({"message":"tampered"});
    let direct = validate_result_envelope(
        &req.envelope,
        &req.expected,
        &keys,
        &default_message_limits(),
        req.now,
    )
    .unwrap();
    assert_ne!(direct.disposition, ResultDisposition::Accepted);
    assert_ne!(
        accept_remote_result(&req, &keys, &mut dispositions, &mut signals).disposition,
        ResultDisposition::Accepted
    );
}
