//! Deadline regressions reuse the managed-service effect fixtures.
use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tetonic_domain::{AttemptState, FailureClass, RunState};
use tetonic_run::managed::AdmissionContext;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn wait_past(deadline: u64) {
    let remaining = Duration::from_secs(deadline)
        .saturating_sub(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
    tokio::time::sleep(remaining + Duration::from_millis(30)).await;
}

async fn admit(
    service: &ManagedRunService,
    parent_attempt: Option<tetonic_domain::AttemptId>,
    deadline: Option<u64>,
) -> Result<tetonic_run::ManagedBinding, tetonic_run::ManagedRunError> {
    let (identity, job_spec) = test_identity_and_spec();
    service
        .admit_with_context(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt,
            },
            AdmissionContext {
                deadline,
                ..Default::default()
            },
        )
        .await
}

async fn assert_timed_out(service: &ManagedRunService, binding: &tetonic_run::ManagedBinding) {
    let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
    assert_eq!(snapshot.state, RunState::Failed);
    assert_eq!(
        snapshot.attempts[&binding.attempt_id].state,
        AttemptState::TimedOut
    );
    assert_eq!(
        snapshot.attempts[&binding.attempt_id].failure_class,
        Some(FailureClass::TimedOut)
    );
    assert!(snapshot.tasks[&binding.task_id].accepted_artifact.is_none());
    assert!(service.binding(&binding.attempt_id).is_none());
}

#[tokio::test]
async fn durable_result_gates_reject_at_the_exact_deadline() {
    let (service, _dir) = test_service();
    let deadline = now() + 60;
    let binding = admit(&service, None, Some(deadline)).await.unwrap();
    let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
    let lease = snapshot.attempts[&binding.attempt_id]
        .lease
        .as_ref()
        .unwrap();
    let proof = tetonic_domain::LeaseProof {
        lease_id: lease.lease_id.clone(),
        lease_epoch: lease.lease_epoch,
        holder: lease.holder.clone(),
    };
    let mut envelope = tetonic_run::command_envelope("deadline-gate", None, "test");
    envelope.timestamp = deadline - 1;
    let mut claim = tetonic_domain::ClaimFinalization {
        envelope: envelope.clone(),
        run_id: binding.run_id.clone(),
        task_id: binding.task_id.clone(),
        attempt_id: binding.attempt_id.clone(),
        task_version: 1,
        input_digest: binding.job_spec.input_digest.clone(),
        lease_proof: proof.clone(),
    };
    let mut complete = tetonic_domain::CompleteAttempt {
        envelope,
        run_id: binding.run_id.clone(),
        attempt_id: binding.attempt_id.clone(),
        task_version: 1,
        workspace_version: None,
        input_digest: binding.job_spec.input_digest.clone(),
        result_digest: "sha256:output".into(),
        lease_proof: proof,
    };
    assert!(tetonic_run::acceptance::try_claim_finalization(&snapshot, &claim).is_ok());
    assert!(tetonic_run::acceptance::try_accept_completion(&snapshot, &complete).is_ok());
    claim.envelope.timestamp = deadline;
    complete.envelope.timestamp = deadline;
    assert!(matches!(
        tetonic_run::acceptance::try_claim_finalization(&snapshot, &claim),
        Err(tetonic_domain::RunSupervisorError::DeadlineExceeded(
            tetonic_domain::TimeoutKind::Task
        ))
    ));
    assert!(matches!(
        tetonic_run::acceptance::try_accept_completion(&snapshot, &complete),
        Err(tetonic_domain::RunSupervisorError::DeadlineExceeded(
            tetonic_domain::TimeoutKind::Task
        ))
    ));
    service.cancel_run(&binding.run_id).await.unwrap();
}

#[tokio::test]
async fn deadline_is_durable_rejects_expired_admission_and_cannot_be_extended_by_children() {
    let (mut service, dir) = test_service();
    let store = tetonic_memory::SharedStore::open(dir.path().join("deadline.db"), 1).unwrap();
    service = ManagedRunService::new(
        Arc::new(DurableRunSupervisor::new(Some(store.clone()))),
        Some(store.clone()),
        service.artifacts().clone(),
        service.policy().clone(),
    );
    assert!(admit(&service, None, Some(now())).await.is_err());
    assert!(store
        .read(|db| db.list_all_run_ids())
        .await
        .unwrap()
        .unwrap()
        .is_empty());
    let deadline = now() + 60;
    let root = admit(&service, None, Some(deadline)).await.unwrap();
    for requested in [None, Some(deadline + 100), Some(deadline - 10)] {
        let child = admit(&service, Some(root.attempt_id.clone()), requested)
            .await
            .unwrap();
        let expected = requested.map_or(deadline, |requested| deadline.min(requested));
        let snapshot = service.inspect_run(&root.run_id).await.unwrap();
        assert_eq!(
            snapshot.tasks[&child.task_id].binding.deadline,
            Some(expected)
        );
    }
    // A separate connection reads the same authoritative binding.
    let reopened = tetonic_memory::SharedStore::open(dir.path().join("deadline.db"), 1).unwrap();
    let run = root.run_id.0.clone();
    let snapshot = reopened
        .read(move |db| db.load_run_snapshot(&run))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.tasks[&root.task_id].binding.deadline,
        Some(deadline)
    );
    assert_eq!(snapshot.deadlines.run_deadline, Some(deadline));
    service.cancel_run(&root.run_id).await.unwrap();
}

#[tokio::test]
async fn expired_finalization_never_starts_effects() {
    let (service, _dir) = test_service();
    let deadline = now() + 2;
    let binding = admit(&service, None, Some(deadline)).await.unwrap();
    wait_past(deadline).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let outcome = service
        .finalize(completed(
            binding.attempt_id.clone(),
            Arc::new(CountEffects {
                calls: calls.clone(),
                fail_commit: false,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_timed_out(&service, &binding).await;
}

#[tokio::test]
async fn deadline_cancels_cooperative_verification_and_records_timeout() {
    let (service, _dir) = test_service();
    let binding = admit(&service, None, Some(now() + 2)).await.unwrap();
    let entered = Arc::new(Notify::new());
    let mut job = completed(
        binding.attempt_id.clone(),
        Arc::new(CancelAwareVerifier(entered)),
    );
    job.policy.as_mut().unwrap().verify_cmd = Some("fixture".into());
    let outcome = tokio::time::timeout(Duration::from_secs(5), service.finalize(job))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
    assert_timed_out(&service, &binding).await;
}

#[tokio::test]
async fn deadline_keeps_ownership_until_noncooperative_commit_has_returned() {
    let (service, _dir) = test_service();
    tokio::task::LocalSet::new()
        .run_until(async {
            let deadline = now() + 2;
            let binding = admit(&service, None, Some(deadline)).await.unwrap();
            let dispatch = service.dispatch_for_attempt(&binding.attempt_id).unwrap();
            let mut terminal = service.arm_attempt_join(&binding.attempt_id);
            let entered = Arc::new(Notify::new());
            let (release, released) = tokio::sync::oneshot::channel();
            let job = completed(
                binding.attempt_id.clone(),
                Arc::new(BlockingEffects {
                    entered: entered.clone(),
                    release: std::sync::Mutex::new(Some(released)),
                }),
            );
            let runs = service.clone();
            let task = tokio::task::spawn_local(async move { runs.finalize(job).await });
            tokio::time::timeout(Duration::from_secs(5), entered.notified())
                .await
                .unwrap();
            wait_past(deadline).await;
            assert!(service.binding(&binding.attempt_id).is_some());
            assert!(service.release_dispatch(&dispatch).await.is_err());
            assert!(matches!(
                terminal.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert!(
                !task.is_finished(),
                "expiry must not detach the real effect owner"
            );
            release.send(()).unwrap();
            let outcome = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
            assert!(matches!(
                terminal.await.unwrap().outcome,
                CandidateOutcome::Failed { .. }
            ));
            assert_timed_out(&service, &binding).await;
        })
        .await;
}
