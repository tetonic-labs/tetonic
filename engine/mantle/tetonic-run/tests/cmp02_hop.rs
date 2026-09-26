//! CMP-02 hop classification: hop APIs refuse agent Runs (`job_spec: Some`).

mod harness;

use harness::*;
use tetonic_domain::{
    AgentJobSpec, AttemptId, AttemptState, CreateAttempt, CreateRun, ExecutionTargetId, IdentityId,
    LeaseAttempt, LeaseId, LeaseProof, RunCommand, RunId, SessionId, StartAttempt, StartRun,
    TaskId, TaskState,
};
use tetonic_run::{
    cancel_hop, command_envelope, create_hop_attempt, ensure_hop_run, fail_hop, hop_run_classified,
    DurableRunSupervisor, RunSupervisor,
};

fn agent_spec() -> AgentJobSpec {
    AgentJobSpec {
        identity_id: IdentityId::new("id_cmp02"),
        definition_digest: "digest".into(),
        input_digest: "input:1".into(),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: "id_cmp02".into(),
    }
}

async fn seed_agent_running(
    sup: &DurableRunSupervisor,
    run: &str,
    task: &str,
    attempt: &str,
) -> (RunId, TaskId, AttemptId) {
    let run_id = RunId::new(run);
    let task_id = TaskId::new(task);
    let attempt_id = AttemptId::new(attempt);
    let created = sup
        .handle(RunCommand::CreateRun(CreateRun {
            envelope: command_envelope("cmp02_agent_create", None, "test"),
            session_id: Some(SessionId::new("sess")),
            run_id: run_id.clone(),
            root_task_id: task_id.clone(),
            root_binding: Default::default(),
            speculation: None,
            job_spec: Some(agent_spec()),
        }))
        .await
        .unwrap();
    let started = sup
        .handle(RunCommand::StartRun(StartRun {
            envelope: command_envelope("cmp02_agent_start", Some(created.sequence), "test"),
            run_id: run_id.clone(),
        }))
        .await
        .unwrap();
    let made = sup
        .handle(RunCommand::CreateAttempt(CreateAttempt {
            envelope: command_envelope("cmp02_agent_att", Some(started.sequence), "test"),
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            attempt_id: attempt_id.clone(),
            delivery_key: Some("turn".into()),
        }))
        .await
        .unwrap();
    let leased = sup
        .handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("cmp02_agent_lease", Some(made.sequence), "test"),
            run_id: run_id.clone(),
            attempt_id: attempt_id.clone(),
            lease_id: LeaseId::new("lease_agent"),
            lease_epoch: 0,
            holder: ExecutionTargetId::worker("worker"),
            issued_at: 1,
            expires_at: 10_000,
            heartbeat_interval_secs: 30,
        }))
        .await
        .unwrap();
    let lease = leased
        .snapshot
        .attempts
        .get(&attempt_id)
        .unwrap()
        .lease
        .as_ref()
        .unwrap();
    sup.handle(RunCommand::StartAttempt(StartAttempt {
        envelope: command_envelope("cmp02_agent_start_attempt", Some(leased.sequence), "test"),
        run_id: run_id.clone(),
        attempt_id: attempt_id.clone(),
        lease_proof: LeaseProof {
            lease_id: lease.lease_id.clone(),
            lease_epoch: lease.lease_epoch,
            holder: lease.holder.clone(),
        },
    }))
    .await
    .unwrap();
    (run_id, task_id, attempt_id)
}

#[tokio::test]
async fn cmp02_fail_hop_rejects_agent_run() {
    let sup = mem_supervisor();
    let (run, _task, attempt) =
        seed_agent_running(&sup, "run_agent", "task_agent", "att_agent").await;
    let seq = current_seq(&sup, &run).await;
    let err = fail_hop(
        &sup,
        run.clone(),
        attempt.clone(),
        seq,
        "infer failover".into(),
    )
    .await
    .expect_err("fail_hop must refuse job_spec: Some");
    assert!(err.to_string().contains("hop-classified"), "got {err}");
    let snap = sup.snapshot(run).await.unwrap();
    assert!(!hop_run_classified(&snap));
    assert_eq!(
        snap.attempts.get(&attempt).map(|a| a.state.clone()),
        Some(AttemptState::Starting)
    );
}

#[tokio::test]
async fn cmp02_cancel_hop_rejects_agent_run() {
    let sup = mem_supervisor();
    let (run, task, attempt) =
        seed_agent_running(&sup, "run_agent", "task_agent", "att_agent").await;
    let err = cancel_hop(&sup, run.clone(), task.clone())
        .await
        .expect_err("cancel_hop must refuse job_spec: Some");
    assert!(err.to_string().contains("hop-classified"), "got {err}");
    let snap = sup.snapshot(run).await.unwrap();
    assert_ne!(
        snap.tasks.get(&task).map(|t| t.state.clone()),
        Some(TaskState::Canceled)
    );
    assert_eq!(
        snap.attempts.get(&attempt).map(|a| a.state.clone()),
        Some(AttemptState::Starting)
    );
}

#[tokio::test]
async fn cmp02_ensure_hop_run_rejects_agent_run() {
    let sup = mem_supervisor();
    let (run, task, _attempt) =
        seed_agent_running(&sup, "run_agent", "task_agent", "att_agent").await;
    let err = ensure_hop_run(&sup, run.clone(), task, None)
        .await
        .expect_err("ensure_hop_run must refuse an agent Run");
    assert!(err.to_string().contains("hop-classified"), "got {err}");
    let snap = sup.snapshot(run).await.unwrap();
    assert!(snap.job_spec.is_some());
}

#[tokio::test]
async fn cmp02_create_hop_attempt_rejects_agent_run() {
    let sup = mem_supervisor();
    let (run, task, agent_att) =
        seed_agent_running(&sup, "run_agent", "task_agent", "att_agent").await;
    let seq = current_seq(&sup, &run).await;
    let hop_att = AttemptId::new("att_hop");
    let err = create_hop_attempt(&sup, run.clone(), task, hop_att.clone(), seq)
        .await
        .expect_err("create_hop_attempt must refuse an agent Run");
    assert!(err.to_string().contains("hop-classified"), "got {err}");
    let snap = sup.snapshot(run).await.unwrap();
    assert!(!snap.attempts.contains_key(&hop_att));
    assert_eq!(
        snap.attempts.get(&agent_att).map(|a| a.state.clone()),
        Some(AttemptState::Starting)
    );
}

#[tokio::test]
async fn cmp02_fail_hop_on_hop_run_fails_only_hop() {
    let sup = mem_supervisor();
    let (agent_run, _agent_task, agent_att) =
        seed_agent_running(&sup, "run_agent", "task_agent", "att_agent").await;

    let hop_run = RunId::new("run_hop");
    let hop_task = TaskId::new("task_hop");
    let hop_att = AttemptId::new("att_hop");
    ensure_hop_run(&sup, hop_run.clone(), hop_task.clone(), None)
        .await
        .unwrap();
    let seq = current_seq(&sup, &hop_run).await;
    let created = create_hop_attempt(
        &sup,
        hop_run.clone(),
        hop_task.clone(),
        hop_att.clone(),
        seq,
    )
    .await
    .unwrap();
    tetonic_run::lease_hop(
        &sup,
        hop_run.clone(),
        hop_att.clone(),
        LeaseId::new("lease_hop"),
        ExecutionTargetId::worker("worker"),
        1,
        created,
    )
    .await
    .unwrap();
    let seq = current_seq(&sup, &hop_run).await;
    fail_hop(
        &sup,
        hop_run.clone(),
        hop_att.clone(),
        seq,
        "infer failover".into(),
    )
    .await
    .unwrap();

    let hop = sup.snapshot(hop_run).await.unwrap();
    assert!(hop_run_classified(&hop));
    assert_eq!(
        hop.attempts.get(&hop_att).map(|a| a.state.clone()),
        Some(AttemptState::Failed)
    );

    let agent = sup.snapshot(agent_run).await.unwrap();
    assert_eq!(
        agent.attempts.get(&agent_att).map(|a| a.state.clone()),
        Some(AttemptState::Starting)
    );
}
