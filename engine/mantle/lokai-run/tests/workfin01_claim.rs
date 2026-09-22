//! WORK-FIN-01 S1: ClaimFinalization is not apply_winner_selection.

mod harness;

use harness::*;
use lokai_domain::{CancelRun, ClaimFinalization, RunCommand, RunSupervisorError, TaskState};
use lokai_run::{try_claim_finalization, RunSupervisor};

async fn claim_cmd(
    sup: &lokai_run::DurableRunSupervisor,
    run: &lokai_domain::RunId,
    task: &lokai_domain::TaskId,
    attempt_id: lokai_domain::AttemptId,
) -> ClaimFinalization {
    let proof = lease_proof_from_snapshot(sup, run, &attempt_id).await;
    let input_digest = input_digest_for_task(sup, run, task).await;
    ClaimFinalization {
        envelope: next_env(sup, run, "claim").await,
        run_id: run.clone(),
        attempt_id,
        task_id: task.clone(),
        task_version: 1,
        input_digest,
        lease_proof: proof,
    }
}

#[tokio::test]
async fn workfin01_claim_sets_finalization_claim_not_winning_attempt() {
    let sup = mem_supervisor();
    let (run, root, _session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let cmd = claim_cmd(&sup, &run, &root, attempt.clone()).await;
    sup.handle(RunCommand::ClaimFinalization(cmd))
        .await
        .unwrap();
    let snap = sup.snapshot(run.clone()).await.unwrap();
    let task = snap.tasks.get(&root).unwrap();
    assert_eq!(task.finalization_claim.as_ref(), Some(&attempt));
    assert!(task.winning_attempt.is_none());
    assert_eq!(
        snap.attempts.get(&attempt).unwrap().state,
        lokai_domain::AttemptState::Running
    );
}

#[tokio::test]
async fn workfin01_claim_rejects_canceled() {
    let sup = mem_supervisor();
    let (run, root, _session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CancelRun(CancelRun {
        envelope: next_env(&sup, &run, "cancel").await,
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let cmd = claim_cmd(&sup, &run, &root, attempt).await;
    let err = sup
        .handle(RunCommand::ClaimFinalization(cmd))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            RunSupervisorError::InvalidTransition(_) | RunSupervisorError::RunNotAccepting(_)
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn workfin01_claim_rejects_other_winner() {
    let sup = mem_supervisor();
    let (run, root, _session) = create_started_run(&sup).await;
    let first = ready_task_with_attempt(&sup, &run, &root).await;
    let first_claim = claim_cmd(&sup, &run, &root, first.clone()).await;
    sup.handle(RunCommand::ClaimFinalization(first_claim))
        .await
        .unwrap();
    let snap = sup.snapshot(run.clone()).await.unwrap();
    let mut cmd = claim_cmd(&sup, &run, &root, first).await;
    cmd.attempt_id = lokai_domain::AttemptId::new("att_other");
    let err = try_claim_finalization(&snap, &cmd).unwrap_err();
    assert!(matches!(err, RunSupervisorError::StaleResult(_)), "{err:?}");
}

#[tokio::test]
async fn workfin01_complete_attempt_still_applies_winner_selection() {
    let sup = mem_supervisor();
    let (run, root, _session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let claim = claim_cmd(&sup, &run, &root, attempt.clone()).await;
    sup.handle(RunCommand::ClaimFinalization(claim))
        .await
        .unwrap();
    let complete = complete_cmd(&sup, &run, &root, attempt.clone(), "digest-ok").await;
    sup.handle(RunCommand::CompleteAttempt(complete))
        .await
        .unwrap();
    let snap = sup.snapshot(run.clone()).await.unwrap();
    let task = snap.tasks.get(&root).unwrap();
    assert_eq!(task.finalization_claim.as_ref(), Some(&attempt));
    assert_eq!(task.winning_attempt.as_ref(), Some(&attempt));
    assert_eq!(
        snap.attempts.get(&attempt).unwrap().state,
        lokai_domain::AttemptState::Succeeded
    );
}

#[test]
fn workfin01_try_claim_finalization_does_not_call_apply_winner_selection() {
    let src = include_str!("../src/acceptance.rs");
    let start = src
        .find("pub fn try_claim_finalization")
        .expect("try_claim");
    let rest = &src[start..];
    let end = rest
        .find("pub fn apply_winner_selection")
        .unwrap_or(rest.len());
    let body = &rest[..end];
    assert!(
        !body.contains("apply_winner_selection"),
        "try_claim_finalization must not call apply_winner_selection"
    );
}

#[test]
fn workfin01_try_claim_is_exported() {
    let _ = try_claim_finalization;
    let _ = TaskState::Canceled;
}
