//! Shared test helpers for lokai-run integration tests (M3-1 / M3-2).

#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use tetonic_domain::{
    AttemptId, CommandEnvelope, CompleteAttempt, CreateAttempt, ExecutionTargetId, FailAttempt,
    LeaseAttempt, LeaseId, LeaseProof, MarkTaskReady, RunCommand, RunId, SessionId, StartAttempt,
    TaskId, TaskInputBinding, TaskState,
};
use tetonic_memory::Store;
use tetonic_run::{binding_input_digest, command_envelope, DurableRunSupervisor, RunSupervisor};

pub fn env(seq: Option<u64>, command_id: &str) -> CommandEnvelope {
    command_envelope(command_id, seq, "test")
}

pub async fn current_seq(sup: &DurableRunSupervisor, run: &RunId) -> u64 {
    sup.snapshot(run.clone()).await.unwrap().sequence
}

pub async fn next_env(sup: &DurableRunSupervisor, run: &RunId, label: &str) -> CommandEnvelope {
    let seq = current_seq(sup, run).await;
    env(Some(seq), &format!("{label}_{seq}"))
}

pub fn mem_supervisor() -> DurableRunSupervisor {
    DurableRunSupervisor::new(None)
}

pub fn db_supervisor() -> (DurableRunSupervisor, tetonic_memory::SharedStore) {
    let db_path = std::env::temp_dir().join(format!("lokai_test_{}.db", uuid::Uuid::new_v4()));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let sup = DurableRunSupervisor::new(Some(store.clone()));
    (sup, store)
}

pub async fn create_started_run(sup: &DurableRunSupervisor) -> (RunId, TaskId, SessionId) {
    let session = SessionId::new("sess_1");
    let run = RunId::new("run_1");
    let root = TaskId::new("task_root");
    sup.handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
        envelope: env(None, "create"),
        session_id: Some(session.clone()),
        run_id: run.clone(),
        root_task_id: root.clone(),
        root_binding: TaskInputBinding::default(),
        speculation: None,
        job_spec: None,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::StartRun(tetonic_domain::StartRun {
        envelope: env(Some(1), "start"),
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    (run, root, session)
}

pub fn default_lease_cmd(
    envelope: CommandEnvelope,
    run_id: RunId,
    attempt_id: AttemptId,
) -> LeaseAttempt {
    LeaseAttempt {
        envelope,
        run_id,
        attempt_id,
        lease_id: LeaseId::new("lease_1"),
        lease_epoch: 0,
        holder: ExecutionTargetId::worker("worker"),
        issued_at: 1,
        expires_at: u64::MAX,
        heartbeat_interval_secs: 30,
    }
}

pub async fn lease_proof_from_snapshot(
    sup: &DurableRunSupervisor,
    run: &RunId,
    attempt_id: &AttemptId,
) -> LeaseProof {
    let snap = sup.snapshot(run.clone()).await.unwrap();
    let lease = snap
        .attempts
        .get(attempt_id)
        .unwrap()
        .lease
        .as_ref()
        .unwrap();
    LeaseProof {
        lease_id: lease.lease_id.clone(),
        lease_epoch: lease.lease_epoch,
        holder: lease.holder.clone(),
    }
}

pub async fn input_digest_for_task(
    sup: &DurableRunSupervisor,
    run: &RunId,
    task: &TaskId,
) -> String {
    let snap = sup.snapshot(run.clone()).await.unwrap();
    binding_input_digest(&snap.tasks.get(task).unwrap().binding)
}

pub async fn complete_cmd(
    sup: &DurableRunSupervisor,
    run: &RunId,
    task: &TaskId,
    attempt_id: AttemptId,
    result_digest: &str,
) -> CompleteAttempt {
    let proof = lease_proof_from_snapshot(sup, run, &attempt_id).await;
    let input_digest = input_digest_for_task(sup, run, task).await;
    CompleteAttempt {
        envelope: next_env(sup, run, "complete").await,
        run_id: run.clone(),
        attempt_id,
        task_version: 1,
        workspace_version: None,
        input_digest,
        result_digest: result_digest.to_string(),
        lease_proof: proof,
    }
}

pub async fn ready_task_with_attempt(
    sup: &DurableRunSupervisor,
    run: &RunId,
    task: &TaskId,
) -> AttemptId {
    ready_task_with_attempt_delivery(sup, run, task, None).await
}

pub async fn ready_task_with_attempt_delivery(
    sup: &DurableRunSupervisor,
    run: &RunId,
    task: &TaskId,
    delivery_key: Option<String>,
) -> AttemptId {
    sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: next_env(sup, run, "ready").await,
        run_id: run.clone(),
        task_id: task.clone(),
    }))
    .await
    .unwrap();
    let attempt = AttemptId::new(format!("att_{task}"));
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(sup, run, "create_attempt").await,
        run_id: run.clone(),
        task_id: task.clone(),
        attempt_id: attempt.clone(),
        delivery_key,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::LeaseAttempt(default_lease_cmd(
        next_env(sup, run, "lease").await,
        run.clone(),
        attempt.clone(),
    )))
    .await
    .unwrap();
    let proof = lease_proof_from_snapshot(sup, run, &attempt).await;
    sup.handle(RunCommand::StartAttempt(StartAttempt {
        envelope: next_env(sup, run, "start").await,
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        lease_proof: proof,
    }))
    .await
    .unwrap();
    attempt
}

pub async fn fail_dependency_task(sup: &DurableRunSupervisor, run: &RunId, dep: &TaskId) {
    let dep_attempt = ready_task_with_attempt(sup, run, dep).await;
    fail_running_attempt(
        sup,
        run,
        dep_attempt,
        tetonic_domain::FailureClass::PermanentExecutionFailure,
        "fail",
    )
    .await;
}

pub async fn create_started_run_speculative(
    sup: &DurableRunSupervisor,
    max_attempts: u32,
) -> (RunId, TaskId, SessionId) {
    let session = SessionId::new("sess_spec");
    let run = RunId::new("run_spec");
    let root = TaskId::new("task_root");
    sup.handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
        envelope: env(None, "create_spec"),
        session_id: Some(session.clone()),
        run_id: run.clone(),
        root_task_id: root.clone(),
        root_binding: TaskInputBinding::default(),
        speculation: Some(tetonic_domain::SpeculationConfig {
            allowed: true,
            max_simultaneous_attempts: max_attempts,
            require_result_agreement: false,
        }),
        job_spec: None,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::StartRun(tetonic_domain::StartRun {
        envelope: env(Some(1), "start_spec"),
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    (run, root, session)
}

pub async fn fail_running_attempt(
    sup: &DurableRunSupervisor,
    run: &RunId,
    attempt_id: AttemptId,
    failure_class: tetonic_domain::FailureClass,
    reason: &str,
) {
    let proof = lease_proof_from_snapshot(sup, run, &attempt_id).await;
    sup.handle(RunCommand::FailAttempt(FailAttempt {
        envelope: next_env(sup, run, "fail").await,
        run_id: run.clone(),
        attempt_id,
        failure_class,
        reason: reason.to_string(),
        lease_proof: Some(proof),
        timeout_kind: None,
    }))
    .await
    .unwrap();
}

pub async fn sup2_snapshot(
    store: &tetonic_memory::SharedStore,
    run: &RunId,
) -> tetonic_domain::RunSnapshot {
    let sup2 = DurableRunSupervisor::new(Some(store.clone()));
    sup2.snapshot(run.clone()).await.unwrap()
}

pub fn task_state(snap: &tetonic_domain::RunSnapshot, task: &TaskId) -> TaskState {
    snap.tasks.get(task).unwrap().state.clone()
}
