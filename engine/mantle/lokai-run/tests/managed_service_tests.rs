//! Tests for ManagedRunService (C2 resolution & deterministic admission cancellation).

use std::sync::Arc;
use tokio::sync::Notify;

use lokai_domain::{AgentIdentity, AgentJobSpec, CandidateOutcome, CompletionKind, IdentityId};
use lokai_run::{job_input_digest, AdmitJob, DurableRunSupervisor, FinalizeJob, ManagedRunService};

struct RejectAcceptance(lokai_artifact::LocalArtifactStore);

#[async_trait::async_trait]
impl lokai_domain::artifact::ArtifactStore for RejectAcceptance {
    async fn begin_write(
        &self,
        declaration: lokai_domain::artifact::ArtifactDeclaration,
    ) -> Result<
        Box<dyn lokai_domain::artifact::ArtifactWriter>,
        lokai_domain::artifact::ArtifactError,
    > {
        self.0.begin_write(declaration).await
    }
    async fn open(
        &self,
        id: &lokai_domain::ArtifactId,
    ) -> Result<
        Box<dyn lokai_domain::artifact::ArtifactReader>,
        lokai_domain::artifact::ArtifactError,
    > {
        self.0.open(id).await
    }
    async fn metadata(
        &self,
        id: &lokai_domain::ArtifactId,
    ) -> Result<lokai_domain::artifact::ArtifactMetadata, lokai_domain::artifact::ArtifactError>
    {
        self.0.metadata(id).await
    }
    async fn mark_accepted(
        &self,
        _: &lokai_domain::ArtifactId,
    ) -> Result<lokai_domain::artifact::ArtifactMetadata, lokai_domain::artifact::ArtifactError>
    {
        Err(lokai_domain::artifact::ArtifactError::Internal(
            "injected acceptance publication failure".into(),
        ))
    }
    async fn delete(
        &self,
        id: &lokai_domain::ArtifactId,
    ) -> Result<(), lokai_domain::artifact::ArtifactError> {
        self.0.delete(id).await
    }
}

#[tokio::test]
async fn artifact_publication_failure_cannot_record_acceptance_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let db = lokai_memory::SharedStore::open(dir.path().join("run.db"), 1).unwrap();
    let artifacts = Arc::new(RejectAcceptance(
        lokai_artifact::LocalArtifactStore::new(
            dir.path(),
            lokai_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    ));
    let service = ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(db.clone()))),
        None,
        artifacts,
        Arc::new(lokai_policy::PolicyEngine::new(
            lokai_policy::PolicyMode::EstateStub,
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
    assert_ne!(snapshot.state, lokai_domain::RunState::Succeeded);
    assert!(snapshot
        .tasks
        .values()
        .all(|task| task.accepted_artifact.is_none()));
    drop(service);
    use lokai_run::RunSupervisor;
    let recovered = DurableRunSupervisor::new(Some(db));
    let snapshot = recovered.snapshot(binding.run_id).await.unwrap();
    assert_ne!(snapshot.state, lokai_domain::RunState::Succeeded);
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
        lokai_artifact::LocalArtifactStore::new(
            temp_dir.path(),
            lokai_artifact::ScanPolicy::Scan(Arc::new(|_text| false)),
        )
        .unwrap(),
    );
    let supervisor = Arc::new(DurableRunSupervisor::new(None));
    let policy = Arc::new(lokai_policy::PolicyEngine::new(
        lokai_policy::PolicyMode::EstateStub,
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
impl lokai_run::FinalizationEffectDriver for CountEffects {
    fn bind_effect_identity(
        &self,
        _: &lokai_domain::TaskId,
        _: &lokai_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        _cancel: &lokai_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        Ok(())
    }
    fn commit_workspace(&self) -> Result<Option<lokai_domain::CommitResult>, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_commit {
            Err("injected commit failure".into())
        } else {
            Ok(None)
        }
    }
}
async fn admit_root(service: &ManagedRunService) -> lokai_run::ManagedBinding {
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
    attempt: lokai_domain::AttemptId,
    driver: Arc<dyn lokai_run::FinalizationEffectDriver>,
) -> FinalizeJob {
    FinalizeJob {
        attempt,
        outcome: CandidateOutcome::Completed {
            summary: "done".into(),
            kind: CompletionKind::Answer,
        },
        policy: Some(lokai_run::FinalizationPolicy {
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
        lokai_domain::CandidateOutcome::Failed { .. }
    ));
    assert!(service.binding(&binding.attempt_id).is_none());
    assert_ne!(
        service.inspect_run(&binding.run_id).await.unwrap().state,
        lokai_domain::RunState::Succeeded
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
impl lokai_run::FinalizationEffectDriver for BlockingEffects {
    fn bind_effect_identity(
        &self,
        _: &lokai_domain::TaskId,
        _: &lokai_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        _cancel: &lokai_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        Ok(())
    }
    fn commit_workspace(&self) -> Result<Option<lokai_domain::CommitResult>, String> {
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
impl lokai_run::FinalizationEffectDriver for CancelAwareVerifier {
    fn bind_effect_identity(
        &self,
        _: &lokai_domain::TaskId,
        _: &lokai_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        cancel: &lokai_domain::work_scope::CancellationSignal,
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
    fn commit_workspace(&self) -> Result<Option<lokai_domain::CommitResult>, String> {
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
