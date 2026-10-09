use super::*;
use tetonic_domain::{AddTask, DelegatedTaskBinding, LeaseProof, RunCommand, TaskInputBinding};

struct PausedProvider {
    provider: Provider,
    arrived: Arc<Notify>,
    proceed: Arc<Notify>,
}
#[async_trait::async_trait]
impl tetonic_inference::InferenceProvider for PausedProvider {
    async fn chat(
        &self,
        request: tetonic_inference::ChatRequest,
        sink: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
        if matches!(self.provider.0.load(Ordering::SeqCst), 0 | 2) {
            self.arrived.notify_one();
            self.proceed.notified().await;
        }
        self.provider.chat(request, sink).await
    }
}

async fn reached(notify: &Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
        .await
        .unwrap();
}

fn dispatch(
    snapshot: &tetonic_domain::RunSnapshot,
    binding: &tetonic_run::ManagedBinding,
    proof: LeaseProof,
) -> RunCommand {
    RunCommand::AddTask(AddTask {
        envelope: tetonic_run::command_envelope("test-dispatch", Some(snapshot.sequence), "test"),
        run_id: binding.run_id.clone(),
        task_id: tetonic_domain::TaskId::new("new-child"),
        binding: TaskInputBinding {
            delegation: Some(DelegatedTaskBinding {
                parent_attempt: binding.attempt_id.clone(),
                parent_lease: Some(proof),
                activation: ActivationBinding {
                    request_id: "child".into(),
                    request_digest: "child-input".into(),
                    audit_session_id: "child-audit".into(),
                },
            }),
            ..Default::default()
        },
    })
}

#[tokio::test(flavor = "current_thread")]
async fn suspended_parent_cannot_dispatch_and_old_handle_cannot_borrow_resumed_lease() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (base, dir) = test_service();
            let service = waiting_service(
                &dir.path().join("delegation-wait.db"),
                base.artifacts().clone(),
            );
            let allowed = Arc::new(AtomicBool::new(true));
            let answer = Arc::new(AtomicBool::new(false));
            let arrived = Arc::new(Notify::new());
            let proceed = Arc::new(Notify::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let provider = Arc::new(PausedProvider {
                provider: Provider(calls.clone()),
                arrived: arrived.clone(),
                proceed: proceed.clone(),
            });
            let context = admission(allowed.clone(), 30);
            let scope = context.authorization.as_ref().unwrap().scope.clone();
            let ManagedSubmission::Started {
                binding,
                completion,
            } = service
                .submit_identity_job_with_context(
                    command(),
                    agent_with_provider(
                        provider,
                        Arc::new(AtomicUsize::new(0)),
                        answer.clone(),
                        100,
                    ),
                    context,
                    None,
                )
                .await
                .unwrap()
            else {
                panic!("not started")
            };
            reached(&arrived).await;
            let old = service.delegation_parent(&binding.attempt_id).unwrap();
            let first = service.inspect_run(&binding.run_id).await.unwrap();
            let lease = first.attempts[&binding.attempt_id].lease.as_ref().unwrap();
            let old_proof = LeaseProof {
                lease_id: lease.lease_id.clone(),
                lease_epoch: lease.lease_epoch,
                holder: lease.holder.clone(),
            };
            assert_eq!(old.authorize_dispatch().await.unwrap(), old_proof);
            assert!(tetonic_run::apply_command(
                &first,
                &dispatch(&first, &binding, old_proof.clone())
            )
            .is_ok());
            proceed.notify_one();
            let waiting = parked(&service, &binding).await;
            assert!(old.authorize_child_scope(&scope).await.is_err());
            assert!(old.authorize_dispatch().await.is_err());
            assert!(service.delegation_parent(&binding.attempt_id).is_err());
            assert!(tetonic_run::apply_command(
                &waiting,
                &dispatch(&waiting, &binding, old_proof.clone())
            )
            .is_err());
            assert!(old
                .authorize_continuation_scope(
                    &scope,
                    tetonic_memory::DelegationLifetime::ParentWork
                )
                .await
                .is_ok());
            allowed.store(false, Ordering::SeqCst);
            assert!(old
                .authorize_continuation_scope(
                    &scope,
                    tetonic_memory::DelegationLifetime::ParentWork
                )
                .await
                .is_err());
            allowed.store(true, Ordering::SeqCst);
            answer.store(true, Ordering::SeqCst);
            reached(&arrived).await;
            let resumed = service.inspect_run(&binding.run_id).await.unwrap();
            assert!(
                resumed.attempts[&binding.attempt_id]
                    .lease
                    .as_ref()
                    .unwrap()
                    .lease_epoch
                    > old_proof.lease_epoch
            );
            assert!(
                old.authorize_child_scope(&scope).await.is_err(),
                "same attempt ID must not lend the new lease to an old handle"
            );
            assert!(
                tetonic_run::apply_command(
                    &resumed,
                    &dispatch(&resumed, &binding, old_proof.clone())
                )
                .is_err(),
                "journal rejects stale dispatch even with the latest sequence"
            );
            let current = service.delegation_parent(&binding.attempt_id).unwrap();
            assert!(current.authorize_child_scope(&scope).await.is_ok());
            assert!(old.authorize_dispatch().await.is_err());
            assert!(
                current.authorize_dispatch().await.unwrap().lease_epoch > old_proof.lease_epoch
            );
            let lease = resumed.attempts[&binding.attempt_id]
                .lease
                .as_ref()
                .unwrap();
            assert!(tetonic_run::apply_command(
                &resumed,
                &dispatch(
                    &resumed,
                    &binding,
                    LeaseProof {
                        lease_id: lease.lease_id.clone(),
                        lease_epoch: lease.lease_epoch,
                        holder: lease.holder.clone()
                    }
                )
            )
            .is_ok());
            service.cancel_run(&binding.run_id).await.unwrap();
            assert!(old
                .authorize_continuation_scope(
                    &scope,
                    tetonic_memory::DelegationLifetime::ParentWork
                )
                .await
                .is_err());
            assert!(current.authorize_child_scope(&scope).await.is_err());
            let _ = completion.await;
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "cancellation must not complete the held inference"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stale_executor_cannot_use_a_still_valid_grant_to_issue_an_effect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (base, dir) = test_service();
            let service = waiting_service(
                &dir.path().join("stale-effect.db"),
                base.artifacts().clone(),
            );
            let arrived = Arc::new(Notify::new());
            let proceed = Arc::new(Notify::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let effects = Arc::new(AtomicUsize::new(0));
            let provider = Arc::new(PausedProvider {
                provider: Provider(calls.clone()),
                arrived: arrived.clone(),
                proceed: proceed.clone(),
            });
            let ManagedSubmission::Started {
                binding,
                completion,
            } = service
                .submit_identity_job_with_context(
                    command(),
                    agent_with_provider(
                        provider,
                        effects.clone(),
                        Arc::new(AtomicBool::new(false)),
                        100,
                    ),
                    admission(Arc::new(AtomicBool::new(true)), 30),
                    None,
                )
                .await
                .unwrap()
            else {
                panic!("not started")
            };
            reached(&arrived).await;
            // Storage fault fixture: simulate replacement while an old provider
            // response is outstanding. No new live executor is fabricated here.
            let mut replacement = service.inspect_run(&binding.run_id).await.unwrap();
            replacement
                .attempts
                .get_mut(&binding.attempt_id)
                .unwrap()
                .lease
                .as_mut()
                .unwrap()
                .lease_epoch += 1;
            let epoch = replacement.attempts[&binding.attempt_id]
                .lease
                .as_ref()
                .unwrap()
                .lease_epoch;
            service
                .store()
                .unwrap()
                .write(move |db| db.persist_run_projection(&replacement))
                .await
                .unwrap()
                .unwrap();
            proceed.notify_one();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(!result.outcome.is_completed());
            assert_eq!(effects.load(Ordering::SeqCst), 0);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let actual = service.inspect_run(&binding.run_id).await.unwrap();
            assert!(
                !actual.cancellation.run_canceled,
                "the loser must not cancel the replacement"
            );
            assert_eq!(
                actual.attempts[&binding.attempt_id]
                    .lease
                    .as_ref()
                    .unwrap()
                    .lease_epoch,
                epoch
            );
        })
        .await;
}
