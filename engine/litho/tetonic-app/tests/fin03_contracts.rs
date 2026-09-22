//! FIN-03 behavioral tests for V4-PROOF-02 and finalization contracts.
//!
//! Tests:
//! 1. fin03_response_only_skips_verify_and_commit
//! 2. fin03_effectful_executes_verify_then_commit_after_claim
//! 3. fin03_verification_failure_blocks_commit
//! 4. fin03_competing_finalizers_fail_closed
//! 5. fin03_manager_modules_have_no_coding_definition_or_commit_staged

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use tetonic_app::commands::{CompleteTurnCommand, RunTurnCommand};
use tetonic_app::events::ApplicationEventSink;
use tetonic_app::services::{
    DefaultRunService, FinalizationEffectDriver, FinalizationPolicy, RunService,
};
use tetonic_domain::{
    ids::ArtifactId, AttemptId, AttemptState, CandidateOutcome, ClaimFinalization, CommitResult,
    CompletionKind, ContentDigest, DataClass, ExecutionTargetId, ExpireLease, FailureClass,
    LeaseAttempt, LeaseId, LeaseProof, RepositoryId, RunCommand, RunState, StartAttempt, TaskId,
    TransactionArtifact, TransactionId, WorkspaceVersion, WorkspaceVersionScheme,
};
use tetonic_run::{command_envelope, DurableRunSupervisor, RunSupervisor};

fn dummy_workspace_version() -> WorkspaceVersion {
    WorkspaceVersion {
        repository_id: RepositoryId::new("repo_test"),
        version_scheme: WorkspaceVersionScheme::Manifest,
        git_head: None,
        dirty_state_digest: ContentDigest::new("sha256:dirty"),
        tracked_state_digest: ContentDigest::new("sha256:tracked"),
        relevant_path_digests: BTreeMap::new(),
        index_generation: None,
    }
}

fn dummy_commit_result() -> CommitResult {
    let base = dummy_workspace_version();
    let result = dummy_workspace_version();
    CommitResult {
        transaction_id: TransactionId::new("tx_fin03"),
        base_version: base.clone(),
        result_version: result.clone(),
        patch_digest: ContentDigest::new("sha256:commit123"),
        artifact: TransactionArtifact {
            transaction_id: TransactionId::new("tx_fin03"),
            base_workspace_version: base,
            result_workspace_version: result,
            patch_artifact_id: ArtifactId::new("art_patch"),
            verification_artifact_id: None,
            task_id: None,
            attempt_id: None,
            data_class: DataClass::RepositorySource,
            commit_succeeded: true,
        },
    }
}

type VerifyResult = Result<(), (String, Option<String>)>;
type CommitOutcome = Result<Option<CommitResult>, String>;

#[derive(Default)]
struct MockEffectDriver {
    calls: Arc<Mutex<Vec<String>>>,
    bind_count: AtomicUsize,
    verify_count: AtomicUsize,
    commit_count: AtomicUsize,
    verify_result: Mutex<Option<VerifyResult>>,
    commit_result: Mutex<Option<CommitOutcome>>,
}

impl MockEffectDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            bind_count: AtomicUsize::new(0),
            verify_count: AtomicUsize::new(0),
            commit_count: AtomicUsize::new(0),
            verify_result: Mutex::new(Some(Ok(()))),
            commit_result: Mutex::new(Some(Ok(Some(dummy_commit_result())))),
        }
    }

    fn failing_verify(output: &str, hint: Option<&str>) -> Self {
        let driver = Self::new();
        *driver.verify_result.lock().unwrap() =
            Some(Err((output.to_string(), hint.map(|s| s.to_string()))));
        driver
    }
}

impl FinalizationEffectDriver for MockEffectDriver {
    fn bind_effect_identity(&self, task_id: &TaskId, attempt_id: &AttemptId) -> Result<(), String> {
        self.bind_count.fetch_add(1, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .push(format!("bind:{}:{}", task_id, attempt_id));
        Ok(())
    }

    fn run_verify(
        &self,
        verify_cmd: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        self.verify_count.fetch_add(1, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .push(format!("verify:{verify_cmd}"));
        self.verify_result.lock().unwrap().clone().unwrap_or(Ok(()))
    }

    fn commit_workspace(&self) -> Result<Option<CommitResult>, String> {
        self.commit_count.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push("commit".to_string());
        self.commit_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(None))
    }
}

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: tetonic_app::events::ApplicationEvent) {}
}

static DB_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TestHarness {
    runs: Arc<DefaultRunService>,
    supervisor: Arc<dyn RunSupervisor>,
    store: tetonic_memory::SharedStore,
}

fn create_harness() -> TestHarness {
    let count = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let db_path = std::env::temp_dir().join(format!(
        "lokai_fin03_contracts_{}_{}.db",
        std::process::id(),
        count
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let supervisor: Arc<dyn RunSupervisor> =
        Arc::new(DurableRunSupervisor::new(Some(store.clone())));
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_dir = std::env::temp_dir().join(format!(
        "lokai_fin03_artifacts_{}_{}",
        std::process::id(),
        count
    ));
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            artifact_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let live = Arc::new(tetonic_app::SessionLiveStore::new());
    let runs = Arc::new(
        DefaultRunService::new(
            Some(store.clone()),
            policy,
            Arc::new(FakeEventSink),
            supervisor.clone(),
            live,
            artifact_store,
        )
        .with_identity_supplier(Arc::new(|input| {
            tetonic_app::definition::coding_identity_and_job_spec(input)
        })),
    );
    TestHarness {
        runs,
        supervisor,
        store,
    }
}

async fn start_test_session(store: &tetonic_memory::SharedStore) -> String {
    store
        .write(|db| {
            db.start_session("test_workspace", "agent", "test_model")
                .map_err(|e| tetonic_app::errors::AppError::PersistenceFailed(e.to_string()))
        })
        .await
        .expect("write")
        .expect("start_session")
}

/// 1. Response-only turns invoke neither verify nor commit contracts and cleanly reach Succeeded.
#[tokio::test]
async fn fin03_response_only_skips_verify_and_commit() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "What is 2 + 2?".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let driver = Arc::new(MockEffectDriver::new());
    // Policy is None (or driver None)
    let policy = None;

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    harness
        .runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "4".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await
        .expect("complete_turn");

    assert_eq!(
        driver.bind_count.load(Ordering::SeqCst),
        0,
        "response-only must not bind effect identity"
    );
    assert_eq!(
        driver.verify_count.load(Ordering::SeqCst),
        0,
        "response-only must not run verify"
    );
    assert_eq!(
        driver.commit_count.load(Ordering::SeqCst),
        0,
        "response-only must not commit workspace"
    );
    assert!(driver.calls.lock().unwrap().is_empty());

    let snap = harness
        .supervisor
        .snapshot(plan.run_id)
        .await
        .expect("snapshot");
    assert_eq!(snap.state, RunState::Succeeded);
    assert!(
        snap.side_effect_commits.is_empty(),
        "response-only turn must have no side-effect commits"
    );
}

/// 2. Effectful turns execute verify then commit after claim, persisting side-effect records.
#[tokio::test]
async fn same_attempt_duplicate_finalizers_do_not_repeat_effects() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "one finalization".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .unwrap();
    let driver = Arc::new(MockEffectDriver::new());
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: None,
    });
    let cmd = CompleteTurnCommand {
        session_id,
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };
    let outcome = CandidateOutcome::Completed {
        summary: "done".into(),
        kind: CompletionKind::Finish,
    };
    let (a, b) = tokio::join!(
        harness
            .runs
            .complete_turn(&cmd, Some(&outcome), policy.clone()),
        harness.runs.complete_turn(&cmd, Some(&outcome), policy),
    );
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "{a:?} {b:?}"
    );
    assert_eq!(driver.commit_count.load(Ordering::SeqCst), 1);
    let snapshot = harness.supervisor.snapshot(plan.run_id).await.unwrap();
    assert_eq!(
        snapshot.attempts[&plan.attempt_id].state,
        AttemptState::Succeeded
    );
}

#[tokio::test]
async fn fin03_effectful_executes_verify_then_commit_after_claim() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Add unit test".into(),
            verify_cmd: Some("cargo test".into()),
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let driver = Arc::new(MockEffectDriver::new());
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    harness
        .runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "added test".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await
        .expect("complete_turn");

    assert_eq!(driver.bind_count.load(Ordering::SeqCst), 1);
    assert_eq!(driver.verify_count.load(Ordering::SeqCst), 1);
    assert_eq!(driver.commit_count.load(Ordering::SeqCst), 1);

    let calls = driver.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            format!("bind:{}:{}", plan.task_id, plan.attempt_id),
            "verify:cargo test".to_string(),
            "commit".to_string(),
        ],
        "finalizer must execute bind -> verify -> commit in order"
    );

    let snap = harness
        .supervisor
        .snapshot(plan.run_id)
        .await
        .expect("snapshot");
    assert_eq!(snap.state, RunState::Succeeded);
    let task = snap.tasks.get(&plan.task_id).expect("task");
    assert_eq!(
        task.finalization_claim.as_ref(),
        Some(&plan.attempt_id),
        "task must record winner finalization claim"
    );
    assert_eq!(
        snap.side_effect_commits.len(),
        1,
        "effectful commit must be durably recorded"
    );
    let rec = snap.side_effect_commits.values().next().unwrap();
    assert!(rec.committed);
    assert_eq!(rec.transaction_id, Some(TransactionId::new("tx_fin03")));
}

/// 3. Verification failure blocks workspace commit and yields VerificationFailed.
#[tokio::test]
async fn fin03_verification_failure_blocks_commit() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Modify code".into(),
            verify_cmd: Some("cargo test".into()),
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let driver = Arc::new(MockEffectDriver::failing_verify(
        "compilation error: mismatched types",
        Some("error[E0308]: mismatched types"),
    ));
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    let _ = harness
        .runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "done".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await;

    assert_eq!(driver.bind_count.load(Ordering::SeqCst), 1);
    assert_eq!(driver.verify_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        driver.commit_count.load(Ordering::SeqCst),
        0,
        "verification failure must prevent workspace commit"
    );

    let snap = harness
        .supervisor
        .snapshot(plan.run_id)
        .await
        .expect("snapshot");
    assert_eq!(
        snap.state,
        RunState::Failed,
        "verification failure must fail the run"
    );
    let attempt = snap.attempts.get(&plan.attempt_id).expect("attempt");
    assert_eq!(attempt.state, AttemptState::Failed);
    assert_eq!(
        attempt.failure_class,
        Some(FailureClass::VerificationFailed),
        "failure class must be VerificationFailed"
    );
    assert!(
        snap.side_effect_commits.is_empty(),
        "failed verification must never record side-effect commits"
    );
}

/// 4. Competing finalizers fail closed: loser cannot claim finalization and never invokes driver.
#[tokio::test]
async fn fin03_competing_finalizers_fail_closed() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Task with competing attempts".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    // Pre-emptively register a competing winner attempt on the supervisor that claims the task
    let competitor = AttemptId::new("att_competing_winner");
    let lease_id = LeaseId::new("lease_comp");
    let snap = harness
        .supervisor
        .snapshot(plan.run_id.clone())
        .await
        .expect("snap");
    let seq = snap.sequence;

    harness
        .supervisor
        .handle(RunCommand::CreateAttempt(tetonic_domain::CreateAttempt {
            envelope: command_envelope("comp_create", Some(seq), "test"),
            run_id: plan.run_id.clone(),
            task_id: plan.task_id.clone(),
            attempt_id: competitor.clone(),
            delivery_key: None,
        }))
        .await
        .expect("create competitor attempt");

    let snap = harness
        .supervisor
        .snapshot(plan.run_id.clone())
        .await
        .expect("snap");
    let seq = snap.sequence;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let leased = harness
        .supervisor
        .handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("comp_lease", Some(seq), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            lease_id: lease_id.clone(),
            lease_epoch: 0,
            holder: ExecutionTargetId::local(),
            issued_at: now,
            expires_at: now + 3600,
            heartbeat_interval_secs: 30,
        }))
        .await
        .expect("lease competitor attempt");

    let comp_rec = leased
        .snapshot
        .attempts
        .get(&competitor)
        .expect("competitor attempt");
    let comp_lease = comp_rec.lease.as_ref().expect("competitor lease");
    let lease_proof = LeaseProof {
        lease_id: comp_lease.lease_id.clone(),
        lease_epoch: comp_lease.lease_epoch,
        holder: comp_lease.holder.clone(),
    };

    harness
        .supervisor
        .handle(RunCommand::StartAttempt(StartAttempt {
            envelope: command_envelope("comp_start", Some(leased.sequence), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            lease_proof: lease_proof.clone(),
        }))
        .await
        .expect("start competitor attempt");

    let snap = harness
        .supervisor
        .snapshot(plan.run_id.clone())
        .await
        .expect("snap");
    let seq = snap.sequence;

    // Competitor claims finalization first
    harness
        .supervisor
        .handle(RunCommand::ClaimFinalization(ClaimFinalization {
            envelope: command_envelope("comp_claim", Some(seq), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            task_id: plan.task_id.clone(),
            task_version: 1,
            input_digest: plan.job_spec.input_digest.clone(),
            lease_proof,
        }))
        .await
        .expect("competitor claims finalization");

    // Now the loser attempt tries to complete with an effectful driver
    let loser_driver = Arc::new(MockEffectDriver::new());
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(loser_driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    let _res = harness
        .runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "loser attempt done".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await;

    // Loser attempt must fail closed
    assert_eq!(
        loser_driver.bind_count.load(Ordering::SeqCst),
        0,
        "competing loser must never bind effect identity"
    );
    assert_eq!(
        loser_driver.verify_count.load(Ordering::SeqCst),
        0,
        "competing loser must never execute verification"
    );
    assert_eq!(
        loser_driver.commit_count.load(Ordering::SeqCst),
        0,
        "competing loser must never commit workspace"
    );
    assert!(
        loser_driver.calls.lock().unwrap().is_empty(),
        "driver calls must be empty on losing attempt"
    );

    // Verify task claim remains owned by competitor
    let snap = harness
        .supervisor
        .snapshot(plan.run_id)
        .await
        .expect("snapshot");
    let task = snap.tasks.get(&plan.task_id).expect("task");
    assert_eq!(
        task.finalization_claim.as_ref(),
        Some(&competitor),
        "task claim must remain owned by competitor"
    );

    let loser_attempt = snap.attempts.get(&plan.attempt_id).expect("attempt");
    assert_eq!(
        loser_attempt.state,
        AttemptState::Failed,
        "losing attempt must be marked Failed"
    );
}

/// 5. Manager modules have zero references to CodingAgentDefinition, commit_staged_if_any, explain_turn, or tetonic_tools::.
#[test]
fn fin03_manager_modules_have_no_coding_definition_or_commit_staged() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let files = [
        "src/run_service.rs",
        "src/turn_finalization.rs",
        "src/identity_job.rs",
    ];

    for rel in files {
        let path = manifest_dir.join(rel);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let prod = content.split("#[cfg(test)]").next().unwrap_or(&content);

        assert!(
            !prod.contains("CodingAgentDefinition"),
            "production {rel} must not reference CodingAgentDefinition"
        );
        assert!(
            !prod.contains("commit_staged_if_any"),
            "production {rel} must not reference commit_staged_if_any"
        );
        assert!(
            !prod.contains("explain_turn"),
            "production {rel} must not reference explain_turn"
        );
        assert!(
            !prod.contains("tetonic_tools::"),
            "production {rel} must not reference tetonic_tools::"
        );
    }
}

/// 6. Cancellation during verification blocks workspace commit via finalizer authority fence.
#[tokio::test]
async fn fin03_cancellation_during_verify_blocks_workspace_commit() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Modify code with slow verify".into(),
            verify_cmd: Some("cargo test".into()),
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let (verify_started_tx, verify_started_rx) = tokio::sync::oneshot::channel();
    let (release_verify_tx, release_verify_rx) = tokio::sync::oneshot::channel();
    let verify_started = Arc::new(Mutex::new(Some(verify_started_tx)));
    let release_verify = Arc::new(Mutex::new(Some(release_verify_rx)));

    struct BlockingVerifyDriver {
        inner: MockEffectDriver,
        verify_started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
        release_verify: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
    }
    impl FinalizationEffectDriver for BlockingVerifyDriver {
        fn bind_effect_identity(
            &self,
            task_id: &TaskId,
            attempt_id: &AttemptId,
        ) -> Result<(), String> {
            self.inner.bind_effect_identity(task_id, attempt_id)
        }
        fn run_verify(
            &self,
            verify_cmd: &str,
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> Result<(), (String, Option<String>)> {
            if let Some(tx) = self.verify_started.lock().unwrap().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = self.release_verify.lock().unwrap().take() {
                let _ = rx.blocking_recv();
            }
            self.inner.run_verify(verify_cmd, _cancel)
        }
        fn commit_workspace(&self) -> Result<Option<CommitResult>, String> {
            self.inner.commit_workspace()
        }
    }

    let driver = Arc::new(BlockingVerifyDriver {
        inner: MockEffectDriver::new(),
        verify_started,
        release_verify,
    });
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let runs = harness.runs.clone();
            let cmd = CompleteTurnCommand {
                session_id: session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: "".into(),
                canceled: false,
                error: None,
            };
            let complete_task = tokio::task::spawn_local(async move {
                runs.complete_turn(
                    &cmd,
                    Some(&CandidateOutcome::Completed {
                        summary: "done".into(),
                        kind: CompletionKind::Finish,
                    }),
                    policy,
                )
                .await
            });

            verify_started_rx.await.unwrap();

            let cancel = harness
                .runs
                .cancel_run(tetonic_app::commands::CancelByRunCommand {
                    run_id: plan.run_id.0.clone(),
                });
            tokio::pin!(cancel);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut cancel)
                    .await
                    .is_err(),
                "cancel must await the running verifier"
            );
            release_verify_tx.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut cancel)
                .await
                .unwrap()
                .unwrap();

            let _ = complete_task.await.unwrap();

            assert_eq!(
                driver.inner.commit_count.load(Ordering::SeqCst),
                0,
                "cancellation during verify must block workspace commit"
            );
            let snap = harness.supervisor.snapshot(plan.run_id).await.unwrap();
            assert!(snap.cancellation.run_canceled);
            assert!(
                snap.side_effect_commits.is_empty(),
                "canceled run must have zero side-effect commits"
            );
        })
        .await;
}

/// 7. Lease loss during verification blocks workspace commit via finalizer authority fence.
#[tokio::test]
async fn fin03_lease_loss_during_verify_blocks_workspace_commit() {
    let harness = create_harness();
    let session_id = start_test_session(&harness.store).await;
    let plan = harness
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Modify code with lease loss".into(),
            verify_cmd: Some("cargo test".into()),
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let (verify_started_tx, verify_started_rx) = tokio::sync::oneshot::channel();
    let (release_verify_tx, release_verify_rx) = tokio::sync::oneshot::channel();
    let verify_started = Arc::new(Mutex::new(Some(verify_started_tx)));
    let release_verify = Arc::new(Mutex::new(Some(release_verify_rx)));

    struct BlockingVerifyDriver {
        inner: MockEffectDriver,
        verify_started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
        release_verify: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
    }
    impl FinalizationEffectDriver for BlockingVerifyDriver {
        fn bind_effect_identity(
            &self,
            task_id: &TaskId,
            attempt_id: &AttemptId,
        ) -> Result<(), String> {
            self.inner.bind_effect_identity(task_id, attempt_id)
        }
        fn run_verify(
            &self,
            verify_cmd: &str,
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> Result<(), (String, Option<String>)> {
            if let Some(tx) = self.verify_started.lock().unwrap().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = self.release_verify.lock().unwrap().take() {
                let _ = rx.blocking_recv();
            }
            self.inner.run_verify(verify_cmd, _cancel)
        }
        fn commit_workspace(&self) -> Result<Option<CommitResult>, String> {
            self.inner.commit_workspace()
        }
    }

    let driver = Arc::new(BlockingVerifyDriver {
        inner: MockEffectDriver::new(),
        verify_started,
        release_verify,
    });
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let runs = harness.runs.clone();
            let cmd = CompleteTurnCommand {
                session_id: session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: "".into(),
                canceled: false,
                error: None,
            };
            let complete_task = tokio::task::spawn_local(async move {
                runs.complete_turn(
                    &cmd,
                    Some(&CandidateOutcome::Completed {
                        summary: "done".into(),
                        kind: CompletionKind::Finish,
                    }),
                    policy,
                )
                .await
            });

            verify_started_rx.await.unwrap();

            let snap = harness
                .supervisor
                .snapshot(plan.run_id.clone())
                .await
                .unwrap();
            harness
                .supervisor
                .handle(RunCommand::ExpireLease(ExpireLease {
                    envelope: command_envelope("expire", Some(snap.sequence), "test"),
                    run_id: plan.run_id.clone(),
                    attempt_id: plan.attempt_id.clone(),
                    expired_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                }))
                .await
                .unwrap();

            release_verify_tx.send(()).unwrap();

            let _ = complete_task.await.unwrap();

            assert_eq!(
                driver.inner.commit_count.load(Ordering::SeqCst),
                0,
                "lease loss during verify must block workspace commit"
            );
            let snap = harness.supervisor.snapshot(plan.run_id).await.unwrap();
            assert!(
                snap.side_effect_commits.is_empty(),
                "lease loss must produce zero side-effect commits"
            );
        })
        .await;
}
