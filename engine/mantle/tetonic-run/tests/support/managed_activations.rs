use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tetonic_domain::{ActivationBinding, ExecutionScope};
use tetonic_run::managed::{
    ActivationReceipt, AdmissionContext, AuthorizedExecution, ExecutionAuthority, ManagedAdmission,
};

struct Authority(Arc<AtomicBool>);
#[async_trait::async_trait]
impl ExecutionAuthority for Authority {
    async fn authorize(
        &self,
        _: &ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> Result<(), ()> {
        self.0.load(Ordering::SeqCst).then_some(()).ok_or(())
    }
}

fn context(allowed: Arc<AtomicBool>, history: &str) -> AdmissionContext {
    AdmissionContext {
        activation: Some(ActivationBinding {
            request_id: "launch-1".into(),
            request_digest: format!("sha256:{}", "a".repeat(64)),
            audit_session_id: history.into(),
        }),
        authorization: Some(AuthorizedExecution {
            grant_id: Some("grant".into()),
            scope: ExecutionScope {
                principal_id: "alice".into(),
                organization_id: "org".into(),
                information_context_id: "private".into(),
            },
            authority: Arc::new(Authority(allowed)),
        }),
        ..Default::default()
    }
}

fn durable_service(
    path: &std::path::Path,
    artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
) -> ManagedRunService {
    let store = tetonic_memory::SharedStore::open(path, 1).unwrap();
    ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(store.clone()))),
        Some(store),
        artifacts,
        Arc::new(tetonic_policy::PolicyEngine::new(
            tetonic_policy::PolicyMode::EstateStub,
        )),
    )
}

async fn admit(
    service: &ManagedRunService,
    context: AdmissionContext,
) -> Result<ManagedAdmission, tetonic_run::ManagedRunError> {
    let (identity, job_spec) = test_identity_and_spec();
    service
        .admit_submission(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
            context,
        )
        .await
}

fn receipt(outcome: ManagedAdmission) -> ActivationReceipt {
    match outcome {
        ManagedAdmission::Existing(receipt) => receipt,
        _ => panic!("retry must never admit work"),
    }
}

#[tokio::test]
async fn concurrent_managers_admit_one_activation_and_retries_survive_reopen() {
    let (base, dir) = test_service();
    let database = dir.path().join("activations.db");
    let a = durable_service(&database, base.artifacts().clone());
    let b = durable_service(&database, base.artifacts().clone());
    let allowed = Arc::new(AtomicBool::new(true));
    // Both managers have observed no run before either attempts CreateRun.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    a.set_pre_admission_barrier(barrier.clone());
    b.set_pre_admission_barrier(barrier);
    let (first, second) = tokio::join!(
        admit(&a, context(allowed.clone(), "audit-a")),
        admit(&b, context(allowed.clone(), "audit-b"))
    );
    let (binding, replay, owner) = match (first.unwrap(), second.unwrap()) {
        (ManagedAdmission::Admitted(binding), ManagedAdmission::Existing(replay)) => {
            (binding, replay, &a)
        }
        (ManagedAdmission::Existing(replay), ManagedAdmission::Admitted(binding)) => {
            (binding, replay, &b)
        }
        _ => panic!("creation must have exactly one winner"),
    };
    assert_eq!(binding.run_id, replay.run_id);
    let snapshot = owner.inspect_run(&binding.run_id).await.unwrap();
    assert_eq!(snapshot.attempts.len(), 1);
    assert_eq!(
        snapshot.tasks[&binding.task_id]
            .binding
            .activation
            .as_ref()
            .unwrap()
            .audit_session_id,
        replay.audit_session_id
    );
    owner.cancel_run(&binding.run_id).await.unwrap();
    let reopened = durable_service(&database, base.artifacts().clone());
    assert_eq!(
        receipt(
            admit(&reopened, context(allowed.clone(), "discarded-audit"))
                .await
                .unwrap()
        ),
        replay
    );
    let after = reopened.inspect_run(&binding.run_id).await.unwrap();
    assert_eq!(
        after.sequence,
        owner.inspect_run(&binding.run_id).await.unwrap().sequence
    );
    assert_eq!(after.attempts.len(), 1);
    for variant in 0..3 {
        let mut changed = context(allowed.clone(), "discarded");
        match variant {
            0 => {
                changed.activation.as_mut().unwrap().request_digest =
                    format!("sha256:{}", "b".repeat(64))
            }
            1 => {
                changed
                    .authorization
                    .as_mut()
                    .unwrap()
                    .scope
                    .information_context_id = "other".into()
            }
            _ => changed.authorization.as_mut().unwrap().grant_id = Some("other-grant".into()),
        }
        assert!(admit(&reopened, changed).await.is_err());
    }
    let mut next = context(allowed.clone(), "next-audit");
    next.activation.as_mut().unwrap().request_id = "launch-2".into();
    let next = match admit(&reopened, next).await.unwrap() {
        ManagedAdmission::Admitted(binding) => binding,
        _ => panic!("a new key should admit distinct work"),
    };
    assert_ne!(next.run_id, binding.run_id);
    reopened.cancel_run(&next.run_id).await.unwrap();
    allowed.store(false, Ordering::SeqCst);
    assert!(
        admit(&reopened, context(allowed, "discarded"))
            .await
            .is_err(),
        "revocation must also deny old receipts"
    );
}

#[tokio::test]
async fn interrupted_admission_keeps_key_bound_without_creating_another_attempt() {
    let (base, dir) = test_service();
    let database = dir.path().join("interrupted.db");
    let service = durable_service(&database, base.artifacts().clone());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_activation_start BEFORE INSERT ON run_events WHEN NEW.sequence=2 BEGIN SELECT RAISE(ABORT,'injected admission failure'); END;").unwrap();
    let allowed = Arc::new(AtomicBool::new(true));
    assert!(admit(&service, context(allowed.clone(), "original-audit"))
        .await
        .is_err());
    connection
        .execute_batch("DROP TRIGGER fail_activation_start;")
        .unwrap();
    let reopened = durable_service(&database, base.artifacts().clone());
    let replay = receipt(
        admit(&reopened, context(allowed.clone(), "discarded-audit"))
            .await
            .unwrap(),
    );
    assert_eq!(replay.audit_session_id, "original-audit");
    let snapshot = reopened.inspect_run(&replay.run_id).await.unwrap();
    assert_eq!(snapshot.sequence, 1);
    assert!(snapshot.attempts.is_empty());
    assert!(reopened.active_bindings(&replay.run_id).is_empty());
    let mut new_key = context(allowed, "next-audit");
    new_key.activation.as_mut().unwrap().request_id = "launch-after-crash".into();
    assert!(
        matches!(
            admit(&reopened, new_key).await,
            Err(tetonic_run::ManagedRunError::ExecutionCapacityExceeded)
        ),
        "an ambiguous admission must retain capacity across restart"
    );
    assert_eq!(
        reopened
            .store()
            .unwrap()
            .read(|db| db.list_all_run_ids())
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

fn new_context(key: &str) -> AdmissionContext {
    let mut ctx = context(Arc::new(AtomicBool::new(true)), key);
    ctx.activation.as_mut().unwrap().request_id = key.into();
    ctx
}

fn admitted(result: ManagedAdmission) -> tetonic_run::ManagedBinding {
    match result {
        ManagedAdmission::Admitted(binding) => binding,
        _ => panic!("expected a new activation"),
    }
}

#[tokio::test]
async fn identity_capacity_is_atomic_across_managers_and_initiating_principals() {
    let (base, dir) = test_service();
    let database = dir.path().join("capacity-race.db");
    let a = durable_service(&database, base.artifacts().clone());
    let b = durable_service(&database, base.artifacts().clone());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    a.set_pre_admission_barrier(barrier.clone());
    b.set_pre_admission_barrier(barrier);
    let mut bob = new_context("bob-launch");
    bob.authorization.as_mut().unwrap().scope.principal_id = "bob".into();
    bob.authorization
        .as_mut()
        .unwrap()
        .scope
        .information_context_id = "bob-private".into();
    let (first, second) = tokio::join!(admit(&a, new_context("alice-launch")), admit(&b, bob));
    a.set_pre_admission_barrier(Arc::new(tokio::sync::Barrier::new(1)));
    b.set_pre_admission_barrier(Arc::new(tokio::sync::Barrier::new(1)));
    let (binding, owner) = match (first, second) {
        (
            Ok(ManagedAdmission::Admitted(binding)),
            Err(tetonic_run::ManagedRunError::ExecutionCapacityExceeded),
        ) => (binding, &a),
        (
            Err(tetonic_run::ManagedRunError::ExecutionCapacityExceeded),
            Ok(ManagedAdmission::Admitted(binding)),
        ) => (binding, &b),
        other => panic!("one admitted identity expected: {other:?}"),
    };
    assert_eq!(
        a.store()
            .unwrap()
            .read(|db| db.list_all_run_ids())
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    // A distinct registered identity can still run concurrently.
    let (mut identity, mut job_spec) = test_identity_and_spec();
    identity.id = IdentityId::new("another_agent");
    job_spec.identity_id = identity.id.clone();
    let separate = admitted(
        a.admit_submission(
            &a.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: None,
            },
            new_context("separate-agent"),
        )
        .await
        .unwrap(),
    );
    a.cancel_run(&separate.run_id).await.unwrap();
    owner.cancel_run(&binding.run_id).await.unwrap();
    let next = admitted(admit(&b, new_context("next-launch")).await.unwrap());
    b.cancel_run(&next.run_id).await.unwrap();
}

#[tokio::test]
async fn canceled_registered_run_holds_capacity_until_actual_blocking_worker_returns() {
    let (base, dir) = test_service();
    let database = dir.path().join("capacity-drain.db");
    let a = durable_service(&database, base.artifacts().clone());
    let b = durable_service(&database, base.artifacts().clone());
    tokio::task::LocalSet::new()
        .run_until(async {
            for abort_finalizer in [false, true] {
                let key = format!("blocking-{abort_finalizer}");
                let binding = admitted(admit(&a, new_context(&key)).await.unwrap());
                let entered = Arc::new(Notify::new());
                let (release, released) = tokio::sync::oneshot::channel();
                let driver = Arc::new(BlockingEffects {
                    entered: entered.clone(),
                    release: std::sync::Mutex::new(Some(released)),
                });
                let runs = a.clone();
                let attempt = binding.attempt_id.clone();
                let finalizer = tokio::task::spawn_local(async move {
                    runs.finalize(completed(attempt, driver)).await
                });
                tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
                    .await
                    .unwrap();
                if abort_finalizer {
                    finalizer.abort();
                }
                let cancel = a.cancel_run(&binding.run_id);
                tokio::pin!(cancel);
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(50), &mut cancel)
                        .await
                        .is_err()
                );
                let snapshot = a.inspect_run(&binding.run_id).await.unwrap();
                assert_eq!(snapshot.state, tetonic_domain::RunState::Canceled);
                assert!(!snapshot.attempts[&binding.attempt_id].execution_quiesced);
                assert!(matches!(
                    admit(&b, new_context(&format!("blocked-{abort_finalizer}"))).await,
                    Err(tetonic_run::ManagedRunError::ExecutionCapacityExceeded)
                ));
                release.send(()).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), &mut cancel)
                    .await
                    .unwrap()
                    .unwrap();
                let _ = finalizer.await;
                let snapshot = a.inspect_run(&binding.run_id).await.unwrap();
                assert!(snapshot.attempts[&binding.attempt_id].execution_quiesced);
                let events = a
                    .resume_events(&binding.run_id, 0, 100)
                    .await
                    .unwrap()
                    .unwrap();
                let replay = tetonic_run::replay::replay_from_events(
                    &tetonic_run::replay::empty_snapshot(binding.run_id.clone(), None),
                    &events,
                )
                .unwrap();
                assert_eq!(replay.attempts, snapshot.attempts);
                let next = admitted(
                    admit(&b, new_context(&format!("blocked-{abort_finalizer}")))
                        .await
                        .unwrap(),
                );
                b.cancel_run(&next.run_id).await.unwrap();
            }
        })
        .await;
}

#[tokio::test]
async fn failed_quiescence_write_retains_capacity_and_is_retryable() {
    let (base, dir) = test_service();
    let database = dir.path().join("capacity-write-failure.db");
    let service = durable_service(&database, base.artifacts().clone());
    let binding = admitted(admit(&service, new_context("first")).await.unwrap());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_quiescence BEFORE INSERT ON run_events WHEN NEW.event_type LIKE '%attempt.quiesced%' BEGIN SELECT RAISE(ABORT,'injected quiescence failure'); END;").unwrap();
    assert!(service.cancel_run(&binding.run_id).await.is_err());
    assert!(service.binding(&binding.attempt_id).is_some());
    assert!(
        !service.inspect_run(&binding.run_id).await.unwrap().attempts[&binding.attempt_id]
            .execution_quiesced
    );
    assert!(matches!(
        admit(&service, new_context("next")).await,
        Err(tetonic_run::ManagedRunError::ExecutionCapacityExceeded)
    ));
    connection
        .execute_batch("DROP TRIGGER fail_quiescence;")
        .unwrap();
    service.cancel_run(&binding.run_id).await.unwrap();
    let next = admitted(admit(&service, new_context("next")).await.unwrap());
    service.cancel_run(&next.run_id).await.unwrap();
}

#[tokio::test]
async fn quiescence_acknowledgement_requires_terminal_attempt_and_exact_owner() {
    use tetonic_domain::{LeaseProof, RunCommand, RunSupervisorError, StartAttempt};
    let (base, dir) = test_service();
    let service = durable_service(
        &dir.path().join("quiescence-proof.db"),
        base.artifacts().clone(),
    );
    let binding = admitted(admit(&service, new_context("first")).await.unwrap());
    let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
    let lease = snapshot.attempts[&binding.attempt_id]
        .lease
        .as_ref()
        .unwrap();
    let ack = StartAttempt {
        envelope: tetonic_run::command_envelope("quiescence-proof", None, "tetonic-manager"),
        run_id: binding.run_id.clone(),
        attempt_id: binding.attempt_id.clone(),
        lease_proof: LeaseProof {
            lease_id: lease.lease_id.clone(),
            lease_epoch: lease.lease_epoch,
            holder: lease.holder.clone(),
        },
    };
    assert!(matches!(
        tetonic_run::apply_command(&snapshot, &RunCommand::RecordAttemptQuiescence(ack.clone())),
        Err(RunSupervisorError::InvalidTransition(_))
    ));
    service.cancel_run(&binding.run_id).await.unwrap();
    let terminal = service.inspect_run(&binding.run_id).await.unwrap();
    for mismatch in 0..3 {
        let mut wrong = ack.clone();
        match mismatch {
            0 => wrong.lease_proof.lease_epoch += 1,
            1 => wrong.lease_proof.lease_id = tetonic_domain::LeaseId::new("wrong"),
            _ => wrong.lease_proof.holder = tetonic_domain::ExecutionTargetId::worker("wrong"),
        }
        assert!(
            tetonic_run::apply_command(&terminal, &RunCommand::RecordAttemptQuiescence(wrong))
                .is_err()
        );
    }
    assert!(
        tetonic_run::apply_command(&terminal, &RunCommand::RecordAttemptQuiescence(ack))
            .unwrap()
            .attempts[&binding.attempt_id]
            .execution_quiesced
    );
}
