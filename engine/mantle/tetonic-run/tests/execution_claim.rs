mod harness;

use harness::*;
use tetonic_domain::{RunCommand, StartAttempt};
use tetonic_run::{DurableRunSupervisor, RunSupervisor};

#[tokio::test]
async fn execution_permission_is_single_use_across_concurrency_lost_response_and_restart() {
    let (sup, store) = db_supervisor();
    let (run, task, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &task).await;
    let claim = RunCommand::ClaimExecution(StartAttempt {
        envelope: env(None, "dispatch"),
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        lease_proof: lease_proof_from_snapshot(&sup, &run, &attempt).await,
    });
    let (a, b) = tokio::join!(sup.handle(claim.clone()), sup.handle(claim.clone()));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(
        sup.handle(claim.clone()).await.is_err(),
        "lost response retry must not grant execution again"
    );
    drop(sup);
    let recovered = DurableRunSupervisor::new(Some(store));
    assert!(recovered.snapshot(run).await.unwrap().attempts[&attempt].execution_claimed);
    assert!(recovered.handle(claim.clone()).await.is_err());
    let RunCommand::ClaimExecution(mut changed_key) = claim else {
        unreachable!()
    };
    changed_key.envelope.command_id = "another-delivery".into();
    assert!(recovered
        .handle(RunCommand::ClaimExecution(changed_key))
        .await
        .is_err());
}
