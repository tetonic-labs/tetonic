//! Cancellation can reach the journal before an attempt's finalizer catches up.
use super::*;
use tetonic_domain::{AttemptState, CancelRun, RunCommand, RunState};
use tetonic_run::RunSupervisor;

#[tokio::test]
async fn durable_run_cancel_wins_over_late_child_finalization() {
    let (base, _dir) = test_service();
    let supervisor = Arc::new(DurableRunSupervisor::new(None));
    let service = ManagedRunService::new(
        supervisor.clone(),
        None,
        base.artifacts().clone(),
        base.policy().clone(),
    );
    let parent = admit_root(&service).await;
    let (identity, job_spec) = test_identity_and_spec();
    let child = service
        .admit(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: Some(parent.attempt_id.clone()),
            },
        )
        .await
        .unwrap();
    let completion = service.arm_attempt_join(&child.attempt_id);

    // Force the ordering inside cancel_run: journal cancellation has committed,
    // but local cleanup and terminal delivery have not yet consumed the child.
    supervisor
        .handle(RunCommand::CancelRun(CancelRun {
            envelope: tetonic_run::command_envelope(
                format!("cancel_run:{}", parent.run_id),
                None,
                "lokai-manager",
            ),
            run_id: parent.run_id.clone(),
        }))
        .await
        .unwrap();
    let outcome = service
        .finalize(FinalizeJob {
            attempt: child.attempt_id.clone(),
            outcome: CandidateOutcome::Canceled {
                reason: "parent authority revoked".into(),
            },
            policy: None,
            finish_run: false,
        })
        .await
        .expect("durable cancellation must not become a finalization failure");
    assert!(matches!(outcome, CandidateOutcome::Canceled { .. }));
    assert!(matches!(
        completion.await.unwrap().outcome,
        CandidateOutcome::Canceled { .. }
    ));
    let snapshot = service.inspect_run(&parent.run_id).await.unwrap();
    assert_eq!(snapshot.state, RunState::Canceled);
    assert_eq!(
        snapshot.attempts[&child.attempt_id].state,
        AttemptState::Canceled
    );
    assert!(snapshot.tasks[&child.task_id].accepted_artifact.is_none());
    assert!(service.binding(&child.attempt_id).is_none());
    // Finishing the remaining cancellation cleanup must remain idempotent.
    service.cancel_run(&parent.run_id).await.unwrap();
}

#[tokio::test]
async fn local_stop_without_durable_cancellation_does_not_report_canceled() {
    let (service, _dir) = test_service();
    let binding = admit_root(&service).await;
    let ticket = service.dispatch_for_attempt(&binding.attempt_id).unwrap();
    service.cancel_dispatch(&ticket).unwrap();
    let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let outcome = service
        .finalize(completed(
            binding.attempt_id.clone(),
            Arc::new(CountEffects {
                calls: effects.clone(),
                fail_commit: false,
            }),
        ))
        .await;
    assert!(
        outcome.is_err(),
        "a local stop is not a durable cancellation receipt"
    );
    assert_eq!(effects.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(service.binding(&binding.attempt_id).is_some());
    assert!(
        !service
            .inspect_run(&binding.run_id)
            .await
            .unwrap()
            .cancellation
            .run_canceled
    );
    service.cancel_run(&binding.run_id).await.unwrap();
}
