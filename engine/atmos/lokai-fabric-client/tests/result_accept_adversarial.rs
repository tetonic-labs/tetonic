//! M5-4 acceptance-path adversarial tests (quarantine, patch, independence, side effects).

#[path = "common/patch_pipeline.rs"]
mod patch_pipeline;

use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use lokai_domain::ids::{AttemptId, JobId, KeyId, LeaseId, ResultId, RunId, TaskId, WorkerId};
use lokai_domain::result_integrity::WorkerBehaviorSignals;
use lokai_domain::workspace::{
    CommitHash, ContentDigest, PatchApproval, RepositoryId, WorkspaceVersion,
    WorkspaceVersionScheme,
};
use lokai_domain::{ResultDisposition, ResultVerificationRequirement, TransactionId};
use lokai_fabric_client::{
    accept_remote_result, apply_verification_policy, deny_remote_side_effects, independence_holds,
    validate_patch_acceptance, IndependenceEvidence, RedundantCandidate, RemoteResultAcceptRequest,
};
use lokai_fabric_protocol::*;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

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

#[test]
fn path_traversal_in_patch_rejected() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let mut b = body("w1", &kid);
    b.artifacts.push(SignedArtifactReference {
        artifact_id: "patch1".into(),
        digest: ContentDigest::new(format!("sha256:{}", hex::encode(Sha256::digest(b"diff")))),
        size_bytes: 4,
        kind: "patch".into(),
        declared_paths: vec!["../escape.txt".into()],
    });
    let env = signed(&sk, b, serde_json::json!({"diff":"x"}));
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let out = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env,
            expected: expected(),
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![("patch1".into(), b"diff".to_vec())],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(out.disposition, ResultDisposition::RejectedPathUnsafe);
}

#[test]
fn executable_payload_rejected() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let bytes = b"MZ\x90\x00executable".to_vec();
    let digest = ContentDigest::new(format!("sha256:{}", hex::encode(Sha256::digest(&bytes))));
    let mut b = body("w1", &kid);
    b.artifacts.push(SignedArtifactReference {
        artifact_id: "bin".into(),
        digest: digest.clone(),
        size_bytes: bytes.len() as u64,
        kind: "file".into(),
        declared_paths: vec!["tool.exe".into()],
    });
    let env = signed(&sk, b, serde_json::json!({}));
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let out = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env,
            expected: expected(),
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![("bin".into(), bytes)],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(
        out.disposition,
        ResultDisposition::RejectedExecutablePayload
    );
}

#[test]
fn archive_bomb_rejected() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let bytes = b"PK\x03\x04compressed-data".to_vec();
    let mut b = body("w1", &kid);
    b.artifacts.push(SignedArtifactReference {
        artifact_id: "arch".into(),
        digest: ContentDigest::new(format!("sha256:{}", hex::encode(Sha256::digest(&bytes)))),
        size_bytes: bytes.len() as u64,
        kind: "file".into(),
        declared_paths: vec!["a.txt".into()],
    });
    let env = signed(
        &sk,
        b,
        serde_json::json!({
            "archive_entries": 1,
            "archive_uncompressed_bytes": 1
        }),
    );
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let out = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env,
            expected: expected(),
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![("arch".into(), bytes)],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(out.disposition, ResultDisposition::RejectedVerification);
}

#[test]
fn fabricated_test_report_fails_local_verification() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let mut b = body("w1", &kid);
    b.execution_summary.exit_status = Some(1);
    let env = signed(&sk, b, serde_json::json!({"test_report": {"passed": true}}));
    let outcome =
        apply_verification_policy(&ResultVerificationRequirement::LocalVerification, &env, &[]);
    assert_eq!(outcome.disposition, ResultDisposition::RejectedVerification);
    let _ = keys;
}

#[test]
fn redundant_same_host_rejected() {
    let a = IndependenceEvidence {
        worker_id: WorkerId::new("w1"),
        host_id: "host-a".into(),
        owner_domain: "d1".into(),
        model_instance: Some("m1".into()),
        execution_nonce: "n1".into(),
        shared_cache: false,
    };
    let b = IndependenceEvidence {
        worker_id: WorkerId::new("w2"),
        host_id: "host-a".into(),
        owner_domain: "d2".into(),
        model_instance: Some("m2".into()),
        execution_nonce: "n2".into(),
        shared_cache: false,
    };
    assert!(!independence_holds(&a, &b));
}

#[test]
fn patch_changes_after_approval_invalidated() {
    let ws = WorkspaceVersion {
        repository_id: RepositoryId::new("repo"),
        version_scheme: WorkspaceVersionScheme::Git,
        git_head: Some(CommitHash("abc".into())),
        dirty_state_digest: ContentDigest::new("d"),
        tracked_state_digest: ContentDigest::new("t"),
        relevant_path_digests: Default::default(),
        index_generation: None,
    };
    let approval = PatchApproval {
        transaction_id: TransactionId::new("txn"),
        patch_digest: ContentDigest::new("sha256:approved"),
        base_version: ws.clone(),
        verification_required: true,
        verification_passed: true,
    };
    let (sk, _, kid) = pair();
    let mut b = body("w1", &kid);
    b.workspace_version = Some(ws.clone());
    let env = signed(
        &sk,
        b,
        serde_json::json!({"base_workspace_version": serde_json::to_value(&ws).unwrap()}),
    );
    let err = validate_patch_acceptance(
        &env,
        &approval,
        &ContentDigest::new("sha256:changed"),
        &ws,
        true,
        true,
    )
    .unwrap_err();
    assert!(err.to_string().contains("changed after approval"));
}

#[test]
fn remote_side_effects_denied() {
    let (sk, _, kid) = pair();
    let env = signed(
        &sk,
        body("w1", &kid),
        serde_json::json!({"invoke_tool": "shell", "issue_capability": true}),
    );
    assert!(deny_remote_side_effects(&env).is_err());
}

#[test]
fn behavior_signals_quarantine_worker() {
    let mut signals = WorkerBehaviorSignals::default();
    for _ in 0..3 {
        signals.record(ResultDisposition::RejectedInvalidSignature);
    }
    assert_eq!(
        signals.derive_operational_state(),
        lokai_domain::WorkerOperationalState::Quarantined
    );
    assert!(!signals.derive_operational_state().allows_scheduling());
}

#[test]
fn duplicate_accept_returns_same_disposition() {
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let env = signed(&sk, body("w1", &kid), serde_json::json!({"ok": true}));
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let req = RemoteResultAcceptRequest {
        envelope: env,
        expected: expected(),
        verification: ResultVerificationRequirement::StructuralValidation,
        redundant: vec![],
        artifact_bytes: vec![],
        now: Utc::now(),
        run_snapshot: None,
        lease_proof: None,
        command_envelope: None,
    };
    let first = accept_remote_result(&req, &keys, &mut dispositions, &mut signals);
    let second = accept_remote_result(&req, &keys, &mut dispositions, &mut signals);
    assert_eq!(first.disposition, ResultDisposition::Accepted);
    assert_eq!(second.disposition, first.disposition);
    assert!(second.complete_attempt.is_none());
}

#[test]
fn workspace_version_mismatch_rejected_on_accept() {
    let ws_worker = WorkspaceVersion {
        repository_id: RepositoryId::new("repo"),
        version_scheme: WorkspaceVersionScheme::Git,
        git_head: Some(CommitHash("worker".into())),
        dirty_state_digest: ContentDigest::new("d1"),
        tracked_state_digest: ContentDigest::new("t1"),
        relevant_path_digests: Default::default(),
        index_generation: None,
    };
    let ws_coord = WorkspaceVersion {
        repository_id: RepositoryId::new("repo"),
        version_scheme: WorkspaceVersionScheme::Git,
        git_head: Some(CommitHash("coord".into())),
        dirty_state_digest: ContentDigest::new("d2"),
        tracked_state_digest: ContentDigest::new("t2"),
        relevant_path_digests: Default::default(),
        index_generation: None,
    };
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let mut b = body("w1", &kid);
    b.workspace_version = Some(ws_worker);
    let env = signed(&sk, b, serde_json::json!({"ok": true}));
    let mut exp = expected();
    exp.workspace_version = Some(ws_coord);
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let outcome = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env,
            expected: exp,
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(outcome.disposition, ResultDisposition::RejectedStaleAttempt);
}

#[test]
fn malicious_worker_correct_except_sensitive_task() {
    // Worker returns valid envelopes for ordinary tasks, but poisons a sensitive
    // task with a digest-mismatched payload. Only the sensitive task is rejected;
    // behavior signals accumulate for scheduling quarantine.
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();

    let ordinary = signed(
        &sk,
        body("w1", &kid),
        serde_json::json!({"analysis": "benign"}),
    );
    let ordinary_ok = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: ordinary,
            expected: expected(),
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(ordinary_ok.disposition, ResultDisposition::Accepted);

    let mut sensitive_body = body("w1", &kid);
    sensitive_body.result_id = ResultId::new("res_sensitive");
    sensitive_body.idempotency_key = IdempotencyKey("idem_sensitive".into());
    sensitive_body.task_id = TaskId::new("task_sensitive");
    sensitive_body.attempt_id = AttemptId::new("att_sensitive");
    let mut env = signed(&sk, sensitive_body, serde_json::json!({"secrets": "exfil"}));
    // Tamper after signing: valid-looking envelope, wrong payload digest binding.
    env.payload = serde_json::json!({"secrets": "EXFIL_TAMPERED"});
    let mut exp = expected();
    exp.task_id = TaskId::new("task_sensitive");
    exp.attempt_id = AttemptId::new("att_sensitive");
    let poisoned = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env,
            expected: exp,
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_ne!(poisoned.disposition, ResultDisposition::Accepted);
    assert!(
        signals.digest_mismatches > 0
            || signals.schema_violations > 0
            || signals.malformed_output > 0
            || signals.invalid_signatures > 0
            || signals.verification_failures > 0
            || poisoned.disposition == ResultDisposition::RejectedDigestMismatch
            || poisoned.disposition == ResultDisposition::RejectedSchema
    );
}

use std::process::Command;

fn git_init_workspace(root: &std::path::Path) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success(),
        "git init"
    );
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "user.email", "test@lokai.local"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "user.name", "Lokai Test"])
        .status()
        .unwrap();
    std::fs::write(root.join("README"), "fixture\n").unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "-A"])
        .status()
        .unwrap();
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", "init"])
            .status()
            .unwrap()
            .success(),
        "git commit"
    );
}

#[test]
fn apply_verified_remote_patch_stages_and_commits() {
    use patch_pipeline::{apply_verified_remote_patch, RemotePatchApplyRequest};
    let dir = tempfile::tempdir().unwrap();
    git_init_workspace(dir.path());
    let ws = lokai_transaction::version::capture_workspace_version(dir.path(), &[])
        .expect("capture workspace version");
    let (sk, _, kid) = pair();
    let mut b = body("w1", &kid);
    b.workspace_version = Some(ws.clone());
    let env = signed(
        &sk,
        b,
        serde_json::json!({"base_workspace_version": serde_json::to_value(&ws).unwrap()}),
    );
    let result = apply_verified_remote_patch(
        dir.path(),
        &RemotePatchApplyRequest {
            envelope: env,
            touched_paths: vec![("notes.txt".into(), b"hello from remote".to_vec())],
            current_workspace: ws,
            authorization_granted: true,
            owner: "owner".into(),
            data_class: lokai_domain::DataClass::RepositorySource,
        },
    )
    .unwrap();
    assert!(result.committed);
    assert!(!result.transaction_id.to_string().is_empty());
    assert!(!result.patch_digest.0.is_empty());
    let written = std::fs::read_to_string(dir.path().join("notes.txt")).unwrap();
    assert_eq!(written, "hello from remote");
}

#[test]
fn valid_signature_false_result_not_auto_accepted_as_correct() {
    // Signature success only reaches Accepted after verification policy; structural
    // validation accepts digest match, never claims honest execution.
    let (sk, pk, kid) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid, pk);
    let env = signed(
        &sk,
        body("w1", &kid),
        serde_json::json!({"claims": ["totally_wrong"]}),
    );
    let v = apply_verification_policy(
        &ResultVerificationRequirement::StructuralValidation,
        &env,
        &[],
    );
    assert_eq!(v.disposition, ResultDisposition::Accepted);
    assert!(!v.locally_verified);
}

// ─────────────────────────────────────────────────────────────────────────
// R13: Redundant verification acceptance & fail-closed tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn redundant_verification_accepts_agreeing_independent_candidates() {
    let (sk1, pk1, kid1) = pair();
    let (sk2, pk2, kid2) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid1, pk1);
    reg(&mut keys, "w2", &kid2, pk2);

    let payload = serde_json::json!({"claims": ["verified_response"], "output": "ok"});
    let env1 = signed(&sk1, body("w1", &kid1), payload.clone());
    let mut b2 = body("w2", &kid2);
    b2.result_id = ResultId::new("res_adv_2");
    let env2 = signed(&sk2, b2, payload);

    let req_ver = ResultVerificationRequirement::IndependentRedundantVerification {
        required_agreement: 2,
    };

    let redundant = vec![RedundantCandidate {
        envelope: env2,
        independence: IndependenceEvidence {
            worker_id: WorkerId::new("w2"),
            host_id: "host_2".into(),
            owner_domain: "domain_b".into(),
            model_instance: Some("qwen:7b-inst2".into()),
            execution_nonce: "nonce_w2".into(),
            shared_cache: false,
        },
    }];

    let outcome = apply_verification_policy(&req_ver, &env1, &redundant);
    assert_eq!(outcome.disposition, ResultDisposition::Accepted);
    assert!(outcome.independently_verified);

    // Also test end-to-end accept_remote_result
    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let accept = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env1,
            expected: expected(),
            verification: req_ver,
            redundant,
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(accept.disposition, ResultDisposition::Accepted);
}

#[test]
fn redundant_verification_rejects_disagreement() {
    let (sk1, pk1, kid1) = pair();
    let (sk2, pk2, kid2) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid1, pk1);
    reg(&mut keys, "w2", &kid2, pk2);

    let env1 = signed(
        &sk1,
        body("w1", &kid1),
        serde_json::json!({"claims": ["answer_A"]}),
    );
    let mut b2 = body("w2", &kid2);
    b2.result_id = ResultId::new("res_adv_2");
    let env2 = signed(
        &sk2,
        b2,
        serde_json::json!({"claims": ["answer_B_disagrees"]}),
    );

    let req_ver = ResultVerificationRequirement::IndependentRedundantVerification {
        required_agreement: 2,
    };

    let redundant = vec![RedundantCandidate {
        envelope: env2,
        independence: IndependenceEvidence {
            worker_id: WorkerId::new("w2"),
            host_id: "host_2".into(),
            owner_domain: "domain_b".into(),
            model_instance: Some("qwen:7b-inst2".into()),
            execution_nonce: "nonce_w2".into(),
            shared_cache: false,
        },
    }];

    let outcome = apply_verification_policy(&req_ver, &env1, &redundant);
    assert_eq!(outcome.disposition, ResultDisposition::RejectedVerification);
    assert!(outcome.reason.contains("redundant agreement"));

    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let accept = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env1,
            expected: expected(),
            verification: req_ver,
            redundant,
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(accept.disposition, ResultDisposition::RejectedVerification);
}

#[test]
fn redundant_verification_fails_closed_on_single_worker() {
    let (sk1, pk1, kid1) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid1, pk1);

    let env1 = signed(
        &sk1,
        body("w1", &kid1),
        serde_json::json!({"claims": ["verified_response"]}),
    );

    let req_ver = ResultVerificationRequirement::IndependentRedundantVerification {
        required_agreement: 2,
    };

    // No redundant candidates provided — single worker only
    let outcome = apply_verification_policy(&req_ver, &env1, &[]);
    assert_eq!(outcome.disposition, ResultDisposition::RejectedVerification);
    assert!(outcome.reason.contains("redundant agreement"));

    let mut dispositions = DispositionStore::new();
    let mut signals = WorkerBehaviorSignals::default();
    let accept = accept_remote_result(
        &RemoteResultAcceptRequest {
            envelope: env1,
            expected: expected(),
            verification: req_ver,
            redundant: vec![],
            artifact_bytes: vec![],
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        },
        &keys,
        &mut dispositions,
        &mut signals,
    );
    assert_eq!(accept.disposition, ResultDisposition::RejectedVerification);
}

#[test]
fn redundant_verification_rejects_non_independent_workers() {
    let (sk1, pk1, kid1) = pair();
    let (sk2, pk2, kid2) = pair();
    let mut keys = ResultSigningKeyRegistry::new();
    reg(&mut keys, "w1", &kid1, pk1);
    reg(&mut keys, "w2", &kid2, pk2);

    let payload = serde_json::json!({"claims": ["verified_response"]});
    let env1 = signed(&sk1, body("w1", &kid1), payload.clone());
    let mut b2 = body("w2", &kid2);
    b2.result_id = ResultId::new("res_adv_2");
    let env2 = signed(&sk2, b2, payload);

    let req_ver = ResultVerificationRequirement::IndependentRedundantVerification {
        required_agreement: 2,
    };

    // Candidate shares same host_id and owner_domain (collusion / not independent)
    let redundant = vec![RedundantCandidate {
        envelope: env2,
        independence: IndependenceEvidence {
            worker_id: WorkerId::new("w2"),
            host_id: "host:w1".into(),      // same host as primary
            owner_domain: "primary".into(), // same owner as primary
            model_instance: None,
            execution_nonce: "nonce_w2".into(),
            shared_cache: false,
        },
    }];

    let outcome = apply_verification_policy(&req_ver, &env1, &redundant);
    assert_eq!(outcome.disposition, ResultDisposition::RejectedVerification);
    assert!(outcome.reason.contains("independence"));
}

#[test]
fn acceptance_requires_exact_artifact_bytes_even_for_empty_content() {
    for (data, supplied, signed_size, expected_disposition) in [
        (
            b"".as_slice(),
            vec![],
            0,
            ResultDisposition::RejectedDigestMismatch,
        ),
        (
            b"".as_slice(),
            vec![("a".into(), vec![])],
            0,
            ResultDisposition::Accepted,
        ),
        (
            b"x".as_slice(),
            vec![("a".into(), b"y".to_vec())],
            1,
            ResultDisposition::RejectedDigestMismatch,
        ),
        (
            b"x".as_slice(),
            vec![("a".into(), b"x".to_vec())],
            0,
            ResultDisposition::RejectedOversized,
        ),
        (
            b"x".as_slice(),
            vec![("a".into(), b"x".to_vec()), ("a".into(), b"x".to_vec())],
            1,
            ResultDisposition::RejectedDigestMismatch,
        ),
        (
            b"x".as_slice(),
            vec![("a".into(), b"x".to_vec())],
            1,
            ResultDisposition::Accepted,
        ),
    ] {
        let (sk, pk, kid) = pair();
        let mut keys = ResultSigningKeyRegistry::new();
        reg(&mut keys, "w1", &kid, pk);
        let mut b = body("w1", &kid);
        b.artifacts.push(SignedArtifactReference {
            artifact_id: "a".into(),
            digest: ContentDigest::new(format!("sha256:{}", hex::encode(Sha256::digest(data)))),
            size_bytes: signed_size,
            kind: "file".into(),
            declared_paths: vec!["a.txt".into()],
        });
        let req = RemoteResultAcceptRequest {
            envelope: signed(&sk, b, serde_json::json!({})),
            expected: expected(),
            verification: ResultVerificationRequirement::StructuralValidation,
            redundant: vec![],
            artifact_bytes: supplied,
            now: Utc::now(),
            run_snapshot: None,
            lease_proof: None,
            command_envelope: None,
        };
        let result = accept_remote_result(
            &req,
            &keys,
            &mut DispositionStore::new(),
            &mut WorkerBehaviorSignals::default(),
        );
        assert_eq!(
            result.disposition, expected_disposition,
            "{}",
            result.reason
        );
    }
}
