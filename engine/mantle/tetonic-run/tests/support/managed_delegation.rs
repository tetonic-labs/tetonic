use super::*;

#[tokio::test]
async fn governed_child_cannot_use_foreign_runtime_handle_or_permissive_authority() {
    let (base, dir) = test_service();
    let database = dir.path().join("child-proof.db");
    let service = durable_service(&database, base.artifacts().clone());
    let other = durable_service(&database, base.artifacts().clone());
    let allowed = Arc::new(AtomicBool::new(true));
    let binding = match admit(&service, context(allowed.clone(), "audit"))
        .await
        .unwrap()
    {
        ManagedAdmission::Admitted(binding) => binding,
        _ => panic!("new parent"),
    };
    let handle = service.delegation_parent(&binding.attempt_id).unwrap();
    for manager in [&other, &service] {
        let mut child = context(allowed.clone(), "child-audit");
        child.activation.as_mut().unwrap().request_id = "child-request".into();
        child.delegation_parent = Some(handle.clone());
        let (identity, job_spec) = test_identity_and_spec();
        let ticket = manager.reserve_dispatch();
        let denied = manager
            .admit_submission(
                &ticket.id,
                AdmitJob {
                    identity,
                    job_spec,
                    role: None,
                    parent_attempt: Some(binding.attempt_id.clone()),
                },
                child,
            )
            .await;
        assert!(matches!(
            denied,
            Err(tetonic_run::ManagedRunError::InvalidRequest(_))
        ));
        manager.release_dispatch(&ticket.id).await.unwrap();
    }
    let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
    assert_eq!(
        snapshot.tasks.len(),
        1,
        "neither registry guessing nor a custom allow-all authority may create a child"
    );
    service.cancel_run(&binding.run_id).await.unwrap();
}

#[tokio::test]
async fn parent_handle_rechecks_scope_authority_and_runtime_lifetime() {
    let (base, dir) = test_service();
    let unscoped = admit_root(&base).await;
    assert!(base.delegation_parent(&unscoped.attempt_id).is_err());
    base.cancel_run(&unscoped.run_id).await.unwrap();

    let service = durable_service(&dir.path().join("parent.db"), base.artifacts().clone());
    let allowed = Arc::new(AtomicBool::new(true));
    let admission = context(allowed.clone(), "audit");
    let scope = admission.authorization.as_ref().unwrap().scope.clone();
    let binding = match admit(&service, admission).await.unwrap() {
        ManagedAdmission::Admitted(binding) => binding,
        _ => panic!("new parent"),
    };
    let handle = service.delegation_parent(&binding.attempt_id).unwrap();
    assert!(handle.authorize_child_scope(&scope).await.is_ok());
    let mut foreign = scope.clone();
    foreign.information_context_id = "other".into();
    assert!(handle.authorize_child_scope(&foreign).await.is_err());
    allowed.store(false, Ordering::SeqCst);
    assert!(handle.authorize_child_scope(&scope).await.is_err());
    allowed.store(true, Ordering::SeqCst);
    service.cancel_run(&binding.run_id).await.unwrap();
    assert!(handle.authorize_child_scope(&scope).await.is_err());
    assert!(service.delegation_parent(&binding.attempt_id).is_err());
    drop(service);
    assert!(handle.authorize_child_scope(&scope).await.is_err());
}

struct PausingAuthority {
    pause: AtomicBool,
    entered: Notify,
    release: Notify,
}

#[async_trait::async_trait]
impl ExecutionAuthority for PausingAuthority {
    async fn authorize(
        &self,
        _: &ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> Result<(), ()> {
        if self.pause.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(())
    }
}

#[tokio::test]
async fn parent_cancel_during_authority_await_cannot_publish_child_permission() {
    let (base, dir) = test_service();
    let service = durable_service(&dir.path().join("parent-race.db"), base.artifacts().clone());
    let authority = Arc::new(PausingAuthority {
        pause: AtomicBool::new(false),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let mut admission = context(Arc::new(AtomicBool::new(true)), "audit");
    let scope = admission.authorization.as_ref().unwrap().scope.clone();
    admission.authorization.as_mut().unwrap().authority = authority.clone();
    let binding = match admit(&service, admission).await.unwrap() {
        ManagedAdmission::Admitted(binding) => binding,
        _ => panic!("new parent"),
    };
    let handle = service.delegation_parent(&binding.attempt_id).unwrap();
    authority.pause.store(true, Ordering::SeqCst);
    let checking = tokio::spawn(async move { handle.authorize_child_scope(&scope).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        authority.entered.notified(),
    )
    .await
    .unwrap();
    service.cancel_run(&binding.run_id).await.unwrap();
    authority.release.notify_one();
    assert!(checking.await.unwrap().is_err());
}
