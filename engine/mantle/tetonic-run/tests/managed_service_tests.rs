//! Tests for ManagedRunService (C2 resolution & deterministic admission cancellation).

#[path = "support/managed_deadlines.rs"]
mod deadlines;
#[path = "support/managed_activations.rs"]
mod activations;

use std::sync::Arc;
use tokio::sync::Notify;

use tetonic_domain::{AgentIdentity, AgentJobSpec, CandidateOutcome, CompletionKind, IdentityId};
use tetonic_run::{
    job_input_digest, AdmitJob, DurableRunSupervisor, FinalizeJob, ManagedRunService,
};

struct RejectAcceptance(tetonic_artifact::LocalArtifactStore);

#[async_trait::async_trait]
impl tetonic_domain::artifact::ArtifactStore for RejectAcceptance {
    async fn begin_write(
        &self,
        declaration: tetonic_domain::artifact::ArtifactDeclaration,
    ) -> Result<
        Box<dyn tetonic_domain::artifact::ArtifactWriter>,
        tetonic_domain::artifact::ArtifactError,
    > {
        self.0.begin_write(declaration).await
    }
    async fn open(
        &self,
        id: &tetonic_domain::ArtifactId,
    ) -> Result<
        Box<dyn tetonic_domain::artifact::ArtifactReader>,
        tetonic_domain::artifact::ArtifactError,
    > {
        self.0.open(id).await
    }
    async fn metadata(
        &self,
        id: &tetonic_domain::ArtifactId,
    ) -> Result<tetonic_domain::artifact::ArtifactMetadata, tetonic_domain::artifact::ArtifactError>
    {
        self.0.metadata(id).await
    }
    async fn mark_accepted(
        &self,
        _: &tetonic_domain::ArtifactId,
    ) -> Result<tetonic_domain::artifact::ArtifactMetadata, tetonic_domain::artifact::ArtifactError>
    {
        Err(tetonic_domain::artifact::ArtifactError::Internal(
            "injected acceptance publication failure".into(),
        ))
    }
    async fn delete(
        &self,
        id: &tetonic_domain::ArtifactId,
    ) -> Result<(), tetonic_domain::artifact::ArtifactError> {
        self.0.delete(id).await
    }
}

#[tokio::test]
async fn artifact_publication_failure_cannot_record_acceptance_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let db = tetonic_memory::SharedStore::open(dir.path().join("run.db"), 1).unwrap();
    let artifacts = Arc::new(RejectAcceptance(
        tetonic_artifact::LocalArtifactStore::new(
            dir.path(),
            tetonic_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    ));
    let service = ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(db.clone()))),
        None,
        artifacts,
        Arc::new(tetonic_policy::PolicyEngine::new(
            tetonic_policy::PolicyMode::EstateStub,
        )),
    );
    let binding = admit_root(&service).await;
    let result = service
        .finalize(FinalizeJob {
            attempt: binding.attempt_id,
            outcome: CandidateOutcome::Completed {
                summary: "answer".into(),
                kind: CompletionKind::Answer,
            },
            policy: None,
            finish_run: true,
        })
        .await;
    assert!(result.is_err());
    let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
    assert_ne!(snapshot.state, tetonic_domain::RunState::Succeeded);
    assert!(snapshot
        .tasks
        .values()
        .all(|task| task.accepted_artifact.is_none()));
    drop(service);
    use tetonic_run::RunSupervisor;
    let recovered = DurableRunSupervisor::new(Some(db));
    let snapshot = recovered.snapshot(binding.run_id).await.unwrap();
    assert_ne!(snapshot.state, tetonic_domain::RunState::Succeeded);
    assert!(snapshot
        .tasks
        .values()
        .all(|task| task.accepted_artifact.is_none()));
}

fn test_identity_and_spec() -> (AgentIdentity, AgentJobSpec) {
    let id = AgentIdentity {
        id: IdentityId::new("test_agent"),
        owning_application: "test_app".into(),
        bound_definition_digest: "sha256:def".into(),
        privilege_class: "default".into(),
        toolset_subscriptions: vec![],
        context_bindings: vec![],
        recovery_id: "rec_1".into(),
    };
    let spec = AgentJobSpec {
        identity_id: id.id.clone(),
        definition_digest: "sha256:def".into(),
        input_digest: job_input_digest("hello"),
        capability_bindings: vec![],
        artifact_bindings: vec![],
        recovery_id: "rec_1".into(),
    };
    (id, spec)
}

fn test_service() -> (ManagedRunService, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let artifacts = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            temp_dir.path(),
            tetonic_artifact::ScanPolicy::Scan(Arc::new(|_text| false)),
        )
        .unwrap(),
    );
    let supervisor = Arc::new(DurableRunSupervisor::new(None));
    let policy = Arc::new(tetonic_policy::PolicyEngine::new(
        tetonic_policy::PolicyMode::EstateStub,
    ));
    let service = ManagedRunService::new(supervisor, None, artifacts, policy);
    (service, temp_dir)
}

#[tokio::test]
async fn cancel_before_attach_aborts_newly_attached_task() {
    let (service, _temp) = test_service();
    let ticket = service.reserve_dispatch();

    // Cancel before attach
    service.cancel_dispatch(&ticket.id).unwrap();

    // Spawn a dummy task that stays alive until aborted
    let task = tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });

    let attach_res = service.attach_task(&ticket.id, task.abort_handle());
    assert!(
        attach_res.is_err(),
        "attaching to canceled ticket must return error"
    );
    let join_res = task.await;
    assert!(join_res.unwrap_err().is_cancelled(), "task must be aborted");
}

#[tokio::test]
async fn cancel_during_admission_cannot_orphan_attempt() {
    let (service, _temp) = test_service();
    let ticket = service.reserve_dispatch();

    // Deterministic pause/resume hook after durable admission
    let pause = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    service.set_post_admission_hook(pause.clone(), resume.clone());

    let (id, spec) = test_identity_and_spec();
    let admit_job = AdmitJob {
        identity: id,
        job_spec: spec,
        role: None,
        parent_attempt: None,
    };

    let svc = service.clone();
    let t_id = ticket.id.clone();
    let admit_handle = tokio::spawn(async move { svc.admit(&t_id, admit_job).await });

    // Wait until durable admission has created the run/attempt
    pause.notified().await;

    // While admission is paused right before registry binding, cancel dispatch!
    service.cancel_dispatch(&ticket.id).unwrap();

    // Resume admission
    resume.notify_one();

    let admit_result = admit_handle.await.unwrap();
    assert!(
        admit_result.is_err(),
        "admission must fail when canceled during admission: {:?}",
        admit_result
    );
}

#[tokio::test]
async fn root_and_child_share_run_and_registry() {
    let (service, _temp) = test_service();

    // 1. Admit root
    let root_ticket = service.reserve_dispatch();
    let (root_id, root_spec) = test_identity_and_spec();
    let root_binding = service
        .admit(
            &root_ticket.id,
            AdmitJob {
                identity: root_id,
                job_spec: root_spec,
                role: None,
                parent_attempt: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        service.binding(&root_binding.attempt_id),
        Some(root_binding.clone())
    );

    // 2. Admit child under root
    let child_ticket = service.reserve_dispatch();
    let (child_id, child_spec) = test_identity_and_spec();
    let child_binding = service
        .admit(
            &child_ticket.id,
            AdmitJob {
                identity: child_id,
                job_spec: child_spec,
                role: Some("critic".into()),
                parent_attempt: Some(root_binding.attempt_id.clone()),
            },
        )
        .await
        .unwrap();

    // Both root and child must share the same run_id!
    assert_eq!(root_binding.run_id, child_binding.run_id);
    assert_ne!(root_binding.attempt_id, child_binding.attempt_id);

    // 3. Heartbeat
    service.heartbeat(&root_binding.attempt_id).await.unwrap();
    service.heartbeat(&child_binding.attempt_id).await.unwrap();

    // 4. Finalize child
    let child_outcome = service
        .finalize(FinalizeJob {
            attempt: child_binding.attempt_id.clone(),
            outcome: CandidateOutcome::Completed {
                summary: "child finished".into(),
                kind: CompletionKind::Answer,
            },
            policy: None,
            finish_run: false, // do not finish run when child completes
        })
        .await
        .unwrap();

    assert!(matches!(child_outcome, CandidateOutcome::Completed { .. }));

    // 5. Finalize root
    let root_outcome = service
        .finalize(FinalizeJob {
            attempt: root_binding.attempt_id.clone(),
            outcome: CandidateOutcome::Completed {
                summary: "root finished".into(),
                kind: CompletionKind::Answer,
            },
            policy: None,
            finish_run: true,
        })
        .await
        .unwrap();

    assert!(matches!(root_outcome, CandidateOutcome::Completed { .. }));

    // Both dispatches can now be cleanly released
    service.release_dispatch(&root_ticket.id).await.unwrap();
    service.release_dispatch(&child_ticket.id).await.unwrap();
}

struct CountEffects {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    fail_commit: bool,
}
impl tetonic_run::FinalizationEffectDriver for CountEffects {
    fn bind_effect_identity(
        &self,
        _: &tetonic_domain::TaskId,
        _: &tetonic_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        Ok(())
    }
    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_commit {
            Err("injected commit failure".into())
        } else {
            Ok(None)
        }
    }
}
async fn admit_root(service: &ManagedRunService) -> tetonic_run::ManagedBinding {
    let (identity, job_spec) = test_identity_and_spec();
    service
        .admit(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
        )
        .await
        .unwrap()
}
fn completed(
    attempt: tetonic_domain::AttemptId,
    driver: Arc<dyn tetonic_run::FinalizationEffectDriver>,
) -> FinalizeJob {
    FinalizeJob {
        attempt,
        outcome: CandidateOutcome::Completed {
            summary: "done".into(),
            kind: CompletionKind::Answer,
        },
        policy: Some(tetonic_run::FinalizationPolicy {
            effect_driver: Some(driver),
            verify_cmd: None,
        }),
        finish_run: true,
    }
}
#[tokio::test]
async fn canceled_claim_cannot_execute_effects() {
    let (service, _temp) = test_service();
    let binding = admit_root(&service).await;
    service.cancel_run(&binding.run_id).await.unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    assert!(service
        .finalize(completed(
            binding.attempt_id,
            Arc::new(CountEffects {
                calls: calls.clone(),
                fail_commit: false
            })
        ))
        .await
        .is_err());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
#[tokio::test]
async fn failed_commit_cannot_report_success() {
    let (service, _temp) = test_service();
    let binding = admit_root(&service).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let outcome = service
        .finalize(completed(
            binding.attempt_id.clone(),
            Arc::new(CountEffects {
                calls,
                fail_commit: true,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        tetonic_domain::CandidateOutcome::Failed { .. }
    ));
    assert!(service.binding(&binding.attempt_id).is_none());
    assert_ne!(
        service.inspect_run(&binding.run_id).await.unwrap().state,
        tetonic_domain::RunState::Succeeded
    );
}
#[tokio::test]
async fn canceled_child_does_not_cancel_parent() {
    let (service, _temp) = test_service();
    let parent = admit_root(&service).await;
    let (identity, job_spec) = test_identity_and_spec();
    let child = service
        .admit(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: Some(parent.attempt_id),
            },
        )
        .await
        .unwrap();
    service
        .finalize(FinalizeJob {
            attempt: child.attempt_id,
            outcome: CandidateOutcome::Canceled {
                reason: "stop child".into(),
            },
            policy: None,
            finish_run: false,
        })
        .await
        .unwrap();
    assert!(
        !service
            .inspect_run(&parent.run_id)
            .await
            .unwrap()
            .cancellation
            .run_canceled
    );
}
#[tokio::test]
async fn mismatched_identity_rejected_before_admission() {
    let (service, _temp) = test_service();
    let (identity, mut job_spec) = test_identity_and_spec();
    job_spec.definition_digest = "different".into();
    assert!(service
        .admit(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None
            }
        )
        .await
        .is_err());
}

struct BlockingEffects {
    entered: Arc<tokio::sync::Notify>,
    release: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}
impl tetonic_run::FinalizationEffectDriver for BlockingEffects {
    fn bind_effect_identity(
        &self,
        _: &tetonic_domain::TaskId,
        _: &tetonic_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        Ok(())
    }
    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        self.entered.notify_one();
        self.release
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .blocking_recv()
            .unwrap();
        Ok(None)
    }
}

#[tokio::test]
async fn cancellation_keeps_owner_alive_until_blocking_effect_returns() {
    let (service, _temp) = test_service();
    tokio::task::LocalSet::new()
        .run_until(async {
            let binding = admit_root(&service).await;
            let ticket = service.dispatch_for_attempt(&binding.attempt_id).unwrap();
            let attempt = binding.attempt_id.clone();
            let mut terminal = service.arm_attempt_join(&attempt);
            let entered = Arc::new(tokio::sync::Notify::new());
            let (release, released) = tokio::sync::oneshot::channel();
            let driver = Arc::new(BlockingEffects {
                entered: entered.clone(),
                release: std::sync::Mutex::new(Some(released)),
            });
            let (done, mut finished) = tokio::sync::oneshot::channel();
            let runs = service.clone();
            service
                .spawn_dispatch(&ticket, async move {
                    let result = runs.finalize(completed(binding.attempt_id, driver)).await;
                    let _ = done.send(result);
                })
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
                .await
                .unwrap();
            let cancel = service.cancel_run(&binding.run_id);
            tokio::pin!(cancel);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut cancel)
                    .await
                    .is_err()
            );
            assert!(service.binding(&attempt).is_some());
            assert!(matches!(
                terminal.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert!(service.release_dispatch(&ticket).await.is_err());
            assert!(
                matches!(
                    finished.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ),
                "cancellation must not drop the effect owner"
            );
            release.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut cancel)
                .await
                .unwrap()
                .unwrap();
            assert!(service.binding(&attempt).is_none());
            assert!(matches!(
                terminal.await.unwrap().outcome,
                CandidateOutcome::Canceled { .. }
            ));
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), finished)
                .await
                .unwrap()
                .expect("effect owner must finish");
            assert!(outcome.is_err());
        })
        .await;
}

#[tokio::test]
async fn dropped_finalizer_cannot_release_its_blocking_worker() {
    let (service, _temp) = test_service();
    tokio::task::LocalSet::new()
        .run_until(async {
            let binding = admit_root(&service).await;
            let entered = Arc::new(tokio::sync::Notify::new());
            let (release, released) = tokio::sync::oneshot::channel();
            let driver = Arc::new(BlockingEffects {
                entered: entered.clone(),
                release: std::sync::Mutex::new(Some(released)),
            });
            let runs = service.clone();
            let attempt = binding.attempt_id.clone();
            let finalizer =
                tokio::task::spawn_local(
                    async move { runs.finalize(completed(attempt, driver)).await },
                );
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
                .await
                .unwrap();
            finalizer.abort();
            assert!(finalizer.await.unwrap_err().is_cancelled());
            let cancel = service.cancel_run(&binding.run_id);
            tokio::pin!(cancel);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut cancel)
                    .await
                    .is_err()
            );
            assert!(service.binding(&binding.attempt_id).is_some());
            release.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut cancel)
                .await
                .unwrap()
                .unwrap();
            assert!(service.binding(&binding.attempt_id).is_none());
        })
        .await;
}

struct CancelAwareVerifier(Arc<tokio::sync::Notify>);
impl tetonic_run::FinalizationEffectDriver for CancelAwareVerifier {
    fn bind_effect_identity(
        &self,
        _: &tetonic_domain::TaskId,
        _: &tetonic_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        self.0.notify_one();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cancel.is_canceled() {
            assert!(
                std::time::Instant::now() < deadline,
                "verification never received cancellation"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err(("verification canceled".into(), None))
    }
    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        panic!("canceled verification must not commit")
    }
}
#[tokio::test]
async fn finalization_passes_live_cancellation_to_verifier() {
    let (service, _temp) = test_service();
    tokio::task::LocalSet::new()
        .run_until(async {
            let binding = admit_root(&service).await;
            let entered = Arc::new(tokio::sync::Notify::new());
            let mut job = completed(
                binding.attempt_id.clone(),
                Arc::new(CancelAwareVerifier(entered.clone())),
            );
            job.policy.as_mut().unwrap().verify_cmd = Some("fixture".into());
            let runs = service.clone();
            let finalizer = tokio::task::spawn_local(async move { runs.finalize(job).await });
            tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
                .await
                .unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                service.cancel_run(&binding.run_id),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(finalizer.await.unwrap().is_err());
            assert!(service.binding(&binding.attempt_id).is_none());
        })
        .await;
}

#[tokio::test]
async fn legacy_admission_rejects_scoped_and_unknown_sessions_before_identity_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = tetonic_memory::SharedStore::open(dir.path().join("run.db"), 1).unwrap();
    let legacy = db
        .write(|db| {
            db.bootstrap_control("alice", "org", "Org")?;
            db.create_information_context(
                "alice",
                "private",
                &tetonic_memory::ContextOwner::Private {
                    org_id: "org".into(),
                },
            )?;
            db.insert_open_discussion("alice", "private", "discussion")?;
            db.start_session("workspace", "test", "model")
        })
        .await
        .unwrap()
        .unwrap();
    let artifacts = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            dir.path().join("artifacts"),
            tetonic_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    );
    let service = ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(db.clone()))),
        Some(db.clone()),
        artifacts,
        Arc::new(tetonic_policy::PolicyEngine::new(
            tetonic_policy::PolicyMode::EstateStub,
        )),
    );
    for session in ["discussion", "unknown"] {
        let (identity, job_spec) = test_identity_and_spec();
        let ticket = service.reserve_dispatch();
        let denied = service
            .admit_with_context(
                &ticket.id,
                AdmitJob {
                    identity,
                    job_spec,
                    role: None,
                    parent_attempt: None,
                },
                tetonic_run::managed::AdmissionContext {
                    session_id: Some(tetonic_domain::SessionId::new(session)),
                    ..Default::default()
                },
            )
            .await;
        assert!(denied
            .unwrap_err()
            .to_string()
            .contains("session is not available"));
        assert!(db
            .read(|db| db.get_agent_identity("test_agent"))
            .await
            .unwrap()
            .unwrap()
            .is_none());
        service.cancel_dispatch(&ticket.id).unwrap();
    }
    let (identity, job_spec) = test_identity_and_spec();
    let ticket = service.reserve_dispatch();
    let binding = service
        .admit_with_context(
            &ticket.id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                session_id: Some(tetonic_domain::SessionId::new(legacy.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(binding.session_id.unwrap().0, legacy);
    service.cancel_dispatch(&ticket.id).unwrap();
}

struct CountingAuthority {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    allow: usize,
}

#[async_trait::async_trait]
impl tetonic_run::managed::ExecutionAuthority for CountingAuthority {
    async fn authorize(
        &self,
        _: &tetonic_domain::ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> Result<(), ()> {
        let call = self
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (call < self.allow).then_some(()).ok_or(())
    }
}

struct ActionBrain {
    perceived: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl tetonic_domain::Brain for ActionBrain {
    async fn complete(
        &self,
        _: tetonic_domain::BrainRequest,
        _: &mut tetonic_domain::BrainTokenSink<'_>,
    ) -> Result<tetonic_domain::BrainResponse, tetonic_domain::BrainError> {
        Err(tetonic_domain::BrainError::Configuration(
            "world attempt must not run a coding turn".into(),
        ))
    }

    async fn perceive(
        &self,
        perception: tetonic_domain::Perception,
    ) -> Result<Option<tetonic_domain::WorldAction>, tetonic_domain::BrainError> {
        self.perceived
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if perception.urgency == tetonic_domain::Urgency::High {
            Ok(Some(tetonic_domain::WorldAction::bare(
                "emergency_action",
                tetonic_domain::BrainPathway::Reflexive {
                    model: "managed-world".into(),
                },
            )))
        } else {
            Ok(None)
        }
    }

    fn describe(&self) -> &str {
        "managed-world"
    }

    fn last_cost(&self) -> tetonic_domain::BrainCost {
        Default::default()
    }
}

struct RecordingWorld {
    manifest: tetonic_domain::WorldManifest,
    executed: std::sync::Mutex<Vec<String>>,
    perception_rx: std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<tetonic_domain::Perception>>>,
}

impl RecordingWorld {
    fn new() -> (
        Arc<Self>,
        tokio::sync::mpsc::Sender<tetonic_domain::Perception>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let adapter = Arc::new(Self {
            manifest: tetonic_domain::WorldManifest::new("managed", "1").with_affordance(
                tetonic_domain::Affordance::instant("emergency_action", "test action"),
            ),
            executed: std::sync::Mutex::new(Vec::new()),
            perception_rx: std::sync::Mutex::new(Some(rx)),
        });
        (adapter, tx)
    }
}

#[async_trait::async_trait]
impl tetonic_domain::WorldAdapter for RecordingWorld {
    fn open(
        &self,
    ) -> (
        tetonic_domain::PerceptionSender,
        tetonic_domain::PerceptionReceiver,
    ) {
        let rx = self
            .perception_rx
            .lock()
            .expect("perception lock")
            .take()
            .expect("world opens once");
        let (tx, _) = tokio::sync::mpsc::channel(1);
        (tx, rx)
    }

    async fn execute(
        &self,
        action: tetonic_domain::WorldAction,
    ) -> Result<tetonic_domain::ActionResult, tetonic_domain::WorldError> {
        self.executed
            .lock()
            .expect("executed lock")
            .push(action.kind);
        Ok(tetonic_domain::ActionResult {
            success: true,
            feedback: Some("executed".into()),
            state_changed: true,
        })
    }

    fn describe(&self) -> &str {
        "recording-world"
    }

    fn manifest(&self) -> tetonic_domain::WorldManifest {
        self.manifest.clone()
    }
}

fn world_invocation() -> tetonic_domain::AgentInvocation {
    tetonic_domain::AgentInvocation {
        instructions: String::new(),
        user_input: "hello".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 1,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    }
}

fn world_perception() -> tetonic_domain::Perception {
    tetonic_domain::Perception {
        when: chrono::Utc::now(),
        sequence: 1,
        urgency: tetonic_domain::Urgency::High,
        signals: vec![],
        events: vec![],
        state: tetonic_domain::WorldState {
            schema_id: "test".into(),
            data: serde_json::Value::Null,
        },
    }
}

fn world_agent(
    brain: Arc<ActionBrain>,
    world: Arc<RecordingWorld>,
) -> tetonic_core::Agent {
    tetonic_core::Agent::default()
        .with_brain(brain)
        .with_world_adapter(world)
}

async fn submit_world(
    service: &ManagedRunService,
    agent: tetonic_core::Agent,
    context: tetonic_run::managed::AdmissionContext,
) -> tetonic_run::managed::ManagedSubmission {
    let (identity, job_spec) = test_identity_and_spec();
    service
        .submit_identity_job_with_context(
            tetonic_run::StartIdentityJobCommand {
                identity,
                job_spec,
                invocation: world_invocation(),
            },
            agent,
            context,
            None,
        )
        .await
        .expect("world submission")
}

#[tokio::test(flavor = "current_thread")]
async fn managed_world_attempt_reaches_the_adapter_and_records_completion() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (service, _dir) = test_service();
            let perceived = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (world, perception) = RecordingWorld::new();
            let agent = world_agent(
                Arc::new(ActionBrain {
                    perceived: perceived.clone(),
                }),
                world.clone(),
            );
            let started = submit_world(
                &service,
                agent,
                tetonic_run::managed::AdmissionContext::default(),
            )
            .await;
            let tetonic_run::managed::ManagedSubmission::Started {
                completion, ..
            } = started
            else {
                panic!("world work must be admitted, not replayed");
            };
            perception
                .send(world_perception())
                .await
                .expect("perception delivered");
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while !perceived.load(std::sync::atomic::Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("managed world loop must perceive");
            drop(perception);
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .expect("world completion")
                .expect("completion receipt");
            assert!(
                matches!(
                    result.outcome,
                    CandidateOutcome::Completed {
                        kind: CompletionKind::Finish,
                        ..
                    }
                ),
                "managed world completion must be recorded, got {:?}",
                result.outcome
            );
            assert_eq!(
                world.executed.lock().expect("executed").as_slice(),
                ["emergency_action"]
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn context_bindings_do_not_grant_unadvertised_capabilities() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (service, _dir) = test_service();
            let (mut identity, mut job_spec) = test_identity_and_spec();
            identity.context_bindings = vec!["recall".into()];
            job_spec.capability_bindings = vec!["recall".into()];
            let mut invocation = world_invocation();
            invocation.instructions = "do not run".into();
            let agent = tetonic_core::Agent::new(
                Arc::new(BlockOnceProvider),
                BlockingTool {
                    entered: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    saw_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                },
                tetonic_core::AgentConfig::default(),
            );
            match service
                .submit_identity_job_with_context(
                    tetonic_run::StartIdentityJobCommand {
                        identity,
                        job_spec,
                        invocation,
                    },
                    agent,
                    tetonic_run::managed::AdmissionContext::default(),
                    None,
                )
                .await
            {
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("unresolved capability"),
                        "context binding must not satisfy a tool the agent does not advertise, got {text}"
                    );
                }
                Ok(_) => panic!("context binding must not admit an unadvertised capability"),
            }
        })
        .await;
}

struct BlockingTool {
    entered: Arc<std::sync::atomic::AtomicBool>,
    saw_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Clone for BlockingTool {
    fn clone(&self) -> Self {
        Self {
            entered: self.entered.clone(),
            saw_cancel: self.saw_cancel.clone(),
        }
    }
}

impl tetonic_domain::ToolHost for BlockingTool {
    fn clone_box(&self) -> Box<dyn tetonic_domain::ToolHost> {
        Box::new(self.clone())
    }
    fn propose(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Option<tetonic_domain::tool_host::ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        name == "block"
    }
    fn is_read_only(&self, _: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<tetonic_domain::tool_host::ToolAdvertisement> {
        vec![tetonic_domain::tool_host::ToolAdvertisement {
            name: "block".into(),
            description: "wait until the attempt is canceled".into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]
    }
    fn validate_tool_args(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _: &serde_json::Value,
        _: Option<&tetonic_domain::AuthorizedAction>,
        cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> tetonic_domain::ToolOutcome {
        if name != "block" {
            return tetonic_domain::ToolOutcome::fail("unsupported tool", "not_found");
        }
        self.entered
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cancel.is_canceled() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let saw = cancel.is_canceled();
        self.saw_cancel
            .store(saw, std::sync::atomic::Ordering::SeqCst);
        tetonic_domain::ToolOutcome::fail("attempt canceled", "canceled")
    }
}

struct BlockOnceProvider;

#[async_trait::async_trait]
impl tetonic_inference::InferenceProvider for BlockOnceProvider {
    async fn chat(
        &self,
        _: tetonic_inference::ChatRequest,
        _: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
        Ok(tetonic_inference::ChatResponse {
            message: tetonic_inference::Message::assistant("").with_tool_calls(vec![
                tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: "block".into(),
                        arguments: serde_json::json!({}),
                    },
                },
            ]),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn managed_cancel_reaches_a_blocking_tool() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (service, _dir) = test_service();
            let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let saw_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (identity, job_spec) = test_identity_and_spec();
            let mut invocation = world_invocation();
            invocation.instructions = "wait until canceled".into();
            let agent = tetonic_core::Agent::new(
                Arc::new(BlockOnceProvider),
                BlockingTool {
                    entered: entered.clone(),
                    saw_cancel: saw_cancel.clone(),
                },
                tetonic_core::AgentConfig::default(),
            );
            let started = service
                .submit_identity_job_with_context(
                    tetonic_run::StartIdentityJobCommand {
                        identity,
                        job_spec,
                        invocation,
                    },
                    agent,
                    tetonic_run::managed::AdmissionContext::default(),
                    None,
                )
                .await
                .expect("blocking tool submission");
            let tetonic_run::managed::ManagedSubmission::Started {
                binding,
                mut completion,
            } = started
            else {
                panic!("blocking tool work must be admitted");
            };
            let entered_flag = entered.clone();
            tokio::select! {
                _ = async {
                    while !entered_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                } => {}
                result = &mut completion => panic!("attempt finished before the tool started: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                    panic!("managed attempt must start the owned tool");
                }
            };
            service.cancel_run(&binding.run_id).await.unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .expect("canceled tool completion")
                .expect("completion receipt");
            assert!(
                matches!(result.outcome, CandidateOutcome::Canceled { .. }),
                "cancel must stop the managed attempt, got {:?}",
                result.outcome
            );
            assert!(
                saw_cancel.load(std::sync::atomic::Ordering::SeqCst),
                "cancel must reach the tool the attempt owns"
            );
        })
        .await;
}

struct OwnedProcessTool {
    entered: Arc<std::sync::atomic::AtomicBool>,
    stopped: Arc<std::sync::Mutex<String>>,
}

impl Clone for OwnedProcessTool {
    fn clone(&self) -> Self {
        Self {
            entered: self.entered.clone(),
            stopped: self.stopped.clone(),
        }
    }
}

impl tetonic_domain::ToolHost for OwnedProcessTool {
    fn clone_box(&self) -> Box<dyn tetonic_domain::ToolHost> {
        Box::new(self.clone())
    }
    fn propose(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Option<tetonic_domain::tool_host::ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        name == "block"
    }
    fn is_read_only(&self, _: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<tetonic_domain::tool_host::ToolAdvertisement> {
        vec![tetonic_domain::tool_host::ToolAdvertisement {
            name: "block".into(),
            description: "run a process until the attempt is canceled".into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]
    }
    fn validate_tool_args(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _: &serde_json::Value,
        _: Option<&tetonic_domain::AuthorizedAction>,
        cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> tetonic_domain::ToolOutcome {
        if name != "block" {
            return tetonic_domain::ToolOutcome::fail("unsupported tool", "not_found");
        }
        self.entered
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut cmd = std::process::Command::new("ping");
        if cfg!(windows) {
            cmd.args(["-n", "30", "127.0.0.1"]);
        } else {
            cmd.args(["-c", "30", "127.0.0.1"]);
        }
        let text = match tetonic_sandbox::command_output_with_signal(
            &mut cmd,
            std::time::Duration::from_secs(40),
            Some(cancel),
            tetonic_sandbox::EnvMode::Minimal,
        ) {
            Ok(_) => "process finished".to_string(),
            Err(error) => error,
        };
        *self.stopped.lock().expect("process result") = text.clone();
        tetonic_domain::ToolOutcome::fail(text, "canceled")
    }
}

struct ToggleAuthority {
    revoked: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl tetonic_run::managed::ExecutionAuthority for ToggleAuthority {
    async fn authorize(
        &self,
        _: &tetonic_domain::ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> Result<(), ()> {
        Ok(())
    }

    async fn revoked_during_execution(
        &self,
        _: &tetonic_domain::ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> bool {
        self.revoked.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn revoking_execution_stops_the_owned_tool() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (base, dir) = test_service();
            let database = dir.path().join("revoke-inflight.db");
            let store = tetonic_memory::SharedStore::open(&database, 1).unwrap();
            let service = ManagedRunService::new(
                Arc::new(DurableRunSupervisor::new(Some(store.clone()))),
                Some(store),
                base.artifacts().clone(),
                Arc::new(tetonic_policy::PolicyEngine::new(
                    tetonic_policy::PolicyMode::EstateStub,
                )),
            );
            let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let saw_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let revoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (identity, job_spec) = test_identity_and_spec();
            let mut invocation = world_invocation();
            invocation.instructions = "wait until the grant is revoked".into();
            let agent = tetonic_core::Agent::new(
                Arc::new(BlockOnceProvider),
                BlockingTool {
                    entered: entered.clone(),
                    saw_cancel: saw_cancel.clone(),
                },
                tetonic_core::AgentConfig::default(),
            );
            let started = service
                .submit_identity_job_with_context(
                    tetonic_run::StartIdentityJobCommand {
                        identity,
                        job_spec,
                        invocation,
                    },
                    agent,
                    tetonic_run::managed::AdmissionContext {
                        authorization: Some(tetonic_run::managed::AuthorizedExecution {
                            grant_id: Some("grant".into()),
                            scope: tetonic_domain::ExecutionScope {
                                principal_id: "alice".into(),
                                organization_id: "org".into(),
                                information_context_id: "private".into(),
                            },
                            authority: Arc::new(ToggleAuthority {
                                revoked: revoked.clone(),
                            }),
                        }),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .expect("revocable submission");
            let tetonic_run::managed::ManagedSubmission::Started {
                mut completion, ..
            } = started
            else {
                panic!("revocable work must be admitted");
            };
            let entered_flag = entered.clone();
            tokio::select! {
                _ = async {
                    while !entered_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                } => {}
                result = &mut completion => panic!("attempt finished before the tool started: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                    panic!("managed attempt must start the owned tool");
                }
            };
            revoked.store(true, std::sync::atomic::Ordering::SeqCst);
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .expect("revoked tool completion")
                .expect("completion receipt");
            assert!(
                matches!(result.outcome, CandidateOutcome::Canceled { .. }),
                "revocation must stop the managed attempt, got {:?}",
                result.outcome
            );
            assert!(
                saw_cancel.load(std::sync::atomic::Ordering::SeqCst),
                "revocation must reach the tool the attempt owns"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn managed_cancel_stops_the_owned_process() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (service, _dir) = test_service();
            let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = Arc::new(std::sync::Mutex::new(String::new()));
            let (identity, job_spec) = test_identity_and_spec();
            let mut invocation = world_invocation();
            invocation.instructions = "wait until canceled".into();
            let agent = tetonic_core::Agent::new(
                Arc::new(BlockOnceProvider),
                OwnedProcessTool {
                    entered: entered.clone(),
                    stopped: stopped.clone(),
                },
                tetonic_core::AgentConfig::default(),
            );
            let started = service
                .submit_identity_job_with_context(
                    tetonic_run::StartIdentityJobCommand {
                        identity,
                        job_spec,
                        invocation,
                    },
                    agent,
                    tetonic_run::managed::AdmissionContext::default(),
                    None,
                )
                .await
                .expect("process tool submission");
            let tetonic_run::managed::ManagedSubmission::Started {
                binding,
                mut completion,
            } = started
            else {
                panic!("process tool work must be admitted");
            };
            let entered_flag = entered.clone();
            tokio::select! {
                _ = async {
                    while !entered_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                } => {}
                result = &mut completion => panic!("attempt finished before the process started: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                    panic!("managed attempt must start the owned process");
                }
            };
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            service.cancel_run(&binding.run_id).await.unwrap();
            let started = std::time::Instant::now();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .expect("canceled process completion")
                .expect("completion receipt");
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "cancel did not stop the owned process"
            );
            assert!(
                matches!(result.outcome, CandidateOutcome::Canceled { .. }),
                "cancel must stop the managed attempt, got {:?}",
                result.outcome
            );
            let stopped = stopped.lock().expect("process result").clone();
            assert_eq!(
                stopped, "command canceled",
                "cancel must stop the process the attempt owns, got {stopped}"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn managed_world_denial_never_reaches_the_adapter_and_cancel_stops_the_wait() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (base, dir) = test_service();
            let database = dir.path().join("world-deny.db");
            let store = tetonic_memory::SharedStore::open(&database, 1).unwrap();
            store
                .write_sync(|db| {
                    db.create_organization(&tetonic_memory::OrganizationRow {
                        org_id: "org".into(),
                        name: "Org".into(),
                    })
                })
                .unwrap()
                .unwrap();
            let service = ManagedRunService::new(
                Arc::new(DurableRunSupervisor::new(Some(store.clone()))),
                Some(store),
                base.artifacts().clone(),
                Arc::new(tetonic_policy::PolicyEngine::new(
                    tetonic_policy::PolicyMode::EstateStub,
                )),
            );
            let perceived = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (world, perception) = RecordingWorld::new();
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let agent = world_agent(
                Arc::new(ActionBrain {
                    perceived: perceived.clone(),
                }),
                world.clone(),
            );
            let started = submit_world(
                &service,
                agent,
                tetonic_run::managed::AdmissionContext {
                    authorization: Some(tetonic_run::managed::AuthorizedExecution {
                        grant_id: Some("grant".into()),
                        scope: tetonic_domain::ExecutionScope {
                            principal_id: "alice".into(),
                            organization_id: "org".into(),
                            information_context_id: "private".into(),
                        },
                        authority: Arc::new(CountingAuthority {
                            calls: calls.clone(),
                            // Admission and the pre-execution check pass. The effect gate denies.
                            allow: 2,
                        }),
                    }),
                    ..Default::default()
                },
            )
            .await;
            let tetonic_run::managed::ManagedSubmission::Started {
                binding,
                completion,
            } = started
            else {
                panic!("denied world work must still be admitted before the effect");
            };
            perception
                .send(world_perception())
                .await
                .expect("perception delivered");
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while !perceived.load(std::sync::atomic::Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("managed world loop must perceive before denial");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(
                world.executed.lock().expect("executed").is_empty(),
                "a denied world effect must not reach the adapter"
            );
            service.cancel_run(&binding.run_id).await.unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .expect("canceled world completion")
                .expect("completion receipt");
            assert!(
                matches!(result.outcome, CandidateOutcome::Canceled { .. }),
                "cancel must stop the managed world wait, got {:?}",
                result.outcome
            );
            assert!(world.executed.lock().expect("executed").is_empty());
        })
        .await;
}

#[tokio::test]
async fn elapsed_deadline_blocks_inference_before_the_gate() {
    let (service, _dir) = test_service();
    let (identity, job_spec) = test_identity_and_spec();
    let ticket = service.reserve_dispatch();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let binding = service
        .admit_with_context(
            &ticket.id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                deadline: Some(now + 1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!service.attempt_must_not_infer(&binding.attempt_id));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(service.attempt_must_not_infer(&binding.attempt_id));
}

#[tokio::test]
async fn revoked_authority_blocks_inference_before_the_gate() {
    let (base, dir) = test_service();
    let database = dir.path().join("infer-block.db");
    let store = tetonic_memory::SharedStore::open(&database, 1).unwrap();
    let service = ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(store.clone()))),
        Some(store),
        base.artifacts().clone(),
        Arc::new(tetonic_policy::PolicyEngine::new(
            tetonic_policy::PolicyMode::EstateStub,
        )),
    );
    let revoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (identity, job_spec) = test_identity_and_spec();
    let ticket = service.reserve_dispatch();
    let binding = service
        .admit_with_context(
            &ticket.id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                authorization: Some(tetonic_run::managed::AuthorizedExecution {
                    grant_id: Some("grant".into()),
                    scope: tetonic_domain::ExecutionScope {
                        principal_id: "alice".into(),
                        organization_id: "org".into(),
                        information_context_id: "private".into(),
                    },
                    authority: Arc::new(ToggleAuthority {
                        revoked: revoked.clone(),
                    }),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!service.attempt_authority_revoked(&binding.attempt_id).await);
    revoked.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(service.attempt_authority_revoked(&binding.attempt_id).await);
}
