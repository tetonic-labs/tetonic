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
        admit(&reopened, context(allowed, "discarded-audit"))
            .await
            .unwrap(),
    );
    assert_eq!(replay.audit_session_id, "original-audit");
    let snapshot = reopened.inspect_run(&replay.run_id).await.unwrap();
    assert_eq!(snapshot.sequence, 1);
    assert!(snapshot.attempts.is_empty());
    assert!(reopened.active_bindings(&replay.run_id).is_empty());
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
