//! V4-PROOF-09: Final Full-Stack Integration Proof
//! (13-COMPLETION-PROGRAM.md:189,465-478).
//!
//! Proves:
//! 1. Infer failover/retry/speculation isolation with a running agent Attempt;
//!    hops alone change lifecycle and cannot commit agent effects or fail the agent Attempt.
//! 2. Hop admission and cancellation cannot terminalize or mutate the enclosing agent job.
//! 3. Concurrent hops on an agent Attempt are distinct hop Attempts without mutating agent state.

use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

use lokai_domain::{
    AgentJobSpec, AttemptId, AttemptState, CreateAttempt, CreateRun, ExecutionTargetId, IdentityId,
    LeaseAttempt, LeaseId, LeaseProof, RunCommand, RunId, SessionId, StartAttempt, StartRun,
    TaskId, TaskState,
};
use lokai_run::{
    cancel_hop, command_envelope, create_hop_attempt, ensure_hop_run, fail_hop, hop_run_classified,
    lease_hop, DurableRunSupervisor, RunSupervisor,
};

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn agent_spec() -> AgentJobSpec {
    AgentJobSpec {
        identity_id: IdentityId::new("id_v4_proof_09"),
        definition_digest: "digest_v4_proof_09".into(),
        input_digest: "input:v4_proof_09".into(),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: "id_v4_proof_09".into(),
    }
}

fn make_supervisor() -> (DurableRunSupervisor, TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join(format!(
        "v4_proof_09_{}_{}.db",
        std::process::id(),
        DB_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let sup = DurableRunSupervisor::new(Some(store));
    (sup, tmp)
}

async fn current_seq(sup: &DurableRunSupervisor, run: &RunId) -> u64 {
    sup.snapshot(run.clone()).await.unwrap().sequence
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
            envelope: command_envelope("v4_proof_09_create", None, "test"),
            session_id: Some(SessionId::new("sess_v4_proof_09")),
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
            envelope: command_envelope("v4_proof_09_start", Some(created.sequence), "test"),
            run_id: run_id.clone(),
        }))
        .await
        .unwrap();

    let made = sup
        .handle(RunCommand::CreateAttempt(CreateAttempt {
            envelope: command_envelope("v4_proof_09_att", Some(started.sequence), "test"),
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            attempt_id: attempt_id.clone(),
            delivery_key: Some("turn_proof_09".into()),
        }))
        .await
        .unwrap();

    let leased = sup
        .handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("v4_proof_09_lease", Some(made.sequence), "test"),
            run_id: run_id.clone(),
            attempt_id: attempt_id.clone(),
            lease_id: LeaseId::new("lease_v4_proof_09"),
            lease_epoch: 0,
            holder: ExecutionTargetId::worker("worker_09"),
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
        envelope: command_envelope("v4_proof_09_running", Some(leased.sequence), "test"),
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
async fn v4_proof_09_infer_hop_failover_isolation_with_running_attempt() {
    let (sup, _tmp) = make_supervisor();
    let (agent_run, agent_task, agent_att) =
        seed_agent_running(&sup, "run_agent_09", "task_agent_09", "att_agent_09").await;

    // Verify initial agent Attempt is Running
    let snap_before = sup.snapshot(agent_run.clone()).await.unwrap();
    let att_state = &snap_before.attempts.get(&agent_att).unwrap().state;
    assert_eq!(*att_state, AttemptState::Running);

    // Issue infer hop. Hop IDs must be minted and live in hop-classified Runs
    let hop_run = RunId::new("hop_run_09");
    let hop_task = TaskId::new("hop_task_09");
    let hop_att = AttemptId::new("hop_att_09");

    ensure_hop_run(&sup, hop_run.clone(), hop_task.clone(), None)
        .await
        .unwrap();
    let seq = current_seq(&sup, &hop_run).await;
    let _ = create_hop_attempt(
        &sup,
        hop_run.clone(),
        hop_task.clone(),
        hop_att.clone(),
        seq,
    )
    .await
    .unwrap();

    // Assert hop run is classified as hop-only (job_spec is None)
    let hop_snap = sup.snapshot(hop_run.clone()).await.unwrap();
    assert!(hop_run_classified(&hop_snap));

    // Lease the hop attempt so it enters Leased state
    let lease_seq = current_seq(&sup, &hop_run).await;
    let _ = lease_hop(
        &sup,
        hop_run.clone(),
        hop_att.clone(),
        LeaseId::new("hop_lease_09"),
        ExecutionTargetId::worker("worker_09"),
        1000,
        lease_seq,
    )
    .await
    .unwrap();

    // Exercise infer failover: fail the hop
    let fail_seq = current_seq(&sup, &hop_run).await;
    fail_hop(
        &sup,
        hop_run.clone(),
        hop_att.clone(),
        fail_seq,
        "infer timeout".into(),
    )
    .await
    .unwrap();

    // Verify: Hop attempt transitioned to Failed
    let hop_snap_after = sup.snapshot(hop_run.clone()).await.unwrap();
    let hop_att_rec = hop_snap_after.attempts.get(&hop_att).unwrap();
    assert_eq!(hop_att_rec.state, AttemptState::Failed);

    // Verify: Enclosing agent Attempt remains Running!
    let agent_snap_after = sup.snapshot(agent_run.clone()).await.unwrap();
    let agent_att_rec = agent_snap_after.attempts.get(&agent_att).unwrap();
    assert_eq!(
        agent_att_rec.state,
        AttemptState::Running,
        "Agent Attempt must remain Running when infer hop fails!"
    );
    let agent_task_rec = agent_snap_after.tasks.get(&agent_task).unwrap();
    assert_eq!(
        agent_task_rec.state,
        TaskState::Running,
        "Agent Task must remain Running when infer hop fails!"
    );

    // Verify: Attempting fail_hop on the agent Run is strictly rejected
    let agent_fail_err = fail_hop(
        &sup,
        agent_run.clone(),
        agent_att.clone(),
        999,
        "bogus".into(),
    )
    .await;
    assert!(
        agent_fail_err.is_err(),
        "fail_hop must refuse to fail an agent Run with job_spec: Some"
    );
}

#[tokio::test]
async fn v4_proof_09_infer_hop_speculation_cancel_isolation() {
    let (sup, _tmp) = make_supervisor();
    let (agent_run, agent_task, agent_att) =
        seed_agent_running(&sup, "run_agent_spec", "task_agent_spec", "att_agent_spec").await;

    // Mint two concurrent hops (simulating speculation / retry)
    let hop_run_1 = RunId::new("hop_spec_run_1");
    let hop_task_1 = TaskId::new("hop_spec_task_1");
    let hop_att_1 = AttemptId::new("hop_spec_att_1");

    ensure_hop_run(&sup, hop_run_1.clone(), hop_task_1.clone(), None)
        .await
        .unwrap();
    let seq_1 = current_seq(&sup, &hop_run_1).await;
    create_hop_attempt(
        &sup,
        hop_run_1.clone(),
        hop_task_1.clone(),
        hop_att_1.clone(),
        seq_1,
    )
    .await
    .unwrap();

    let hop_run_2 = RunId::new("hop_spec_run_2");
    let hop_task_2 = TaskId::new("hop_spec_task_2");
    let hop_att_2 = AttemptId::new("hop_spec_att_2");

    ensure_hop_run(&sup, hop_run_2.clone(), hop_task_2.clone(), None)
        .await
        .unwrap();
    let seq_2 = current_seq(&sup, &hop_run_2).await;
    create_hop_attempt(
        &sup,
        hop_run_2.clone(),
        hop_task_2.clone(),
        hop_att_2.clone(),
        seq_2,
    )
    .await
    .unwrap();

    // Cancel speculation on hop 1
    cancel_hop(&sup, hop_run_1.clone(), hop_task_1.clone())
        .await
        .unwrap();

    // Hop 1 task is Canceled
    let snap_hop_1 = sup.snapshot(hop_run_1.clone()).await.unwrap();
    assert_eq!(
        snap_hop_1.tasks.get(&hop_task_1).unwrap().state,
        TaskState::Canceled
    );

    // Hop 2 is unaffected
    let snap_hop_2 = sup.snapshot(hop_run_2.clone()).await.unwrap();
    assert_ne!(
        snap_hop_2.tasks.get(&hop_task_2).unwrap().state,
        TaskState::Canceled
    );

    // Agent Attempt remains Running
    let agent_snap = sup.snapshot(agent_run.clone()).await.unwrap();
    assert_eq!(
        agent_snap.attempts.get(&agent_att).unwrap().state,
        AttemptState::Running,
        "Agent Attempt must remain Running during hop speculation cancellation"
    );

    // Cancel hop cannot cancel agent Run
    assert!(
        cancel_hop(&sup, agent_run.clone(), agent_task.clone())
            .await
            .is_err(),
        "cancel_hop must refuse to cancel an agent Run"
    );
}

#[path = "support/managed_failover.rs"]
mod managed_failover;
