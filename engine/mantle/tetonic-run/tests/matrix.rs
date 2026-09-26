//! M3-1 required test matrix — durable run supervisor.

mod harness;

use std::sync::Arc;
use std::thread;

use harness::*;
use tetonic_domain::{
    AcceptArtifact, AddDependency, AddTask, ArtifactRef, AttemptId, CancelRun, CancelTask,
    CompleteAttempt, CreateAttempt, DependencyPolicy, ExecutionTargetId, FailureClass, FinishRun,
    LeaseId, LeaseProof, RunCommand, RunFinishOutcome, RunId, RunState, RunSupervisorError,
    StartAttempt, TaskId, TaskInputBinding, TaskState,
};
use tetonic_run::{command_envelope, DurableRunSupervisor, RunSupervisor};

#[tokio::test]
async fn valid_full_run_lifecycle() {
    let sup = mem_supervisor();
    let (run, root, _session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:abc").await,
    ))
    .await
    .unwrap();
    sup.handle(RunCommand::AcceptArtifact(AcceptArtifact {
        envelope: next_env(&sup, &run, "artifact").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        artifact: ArtifactRef {
            artifact_id: "art_1".into(),
            digest: "sha256:abc".into(),
        },
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::FinishRun(FinishRun {
        envelope: next_env(&sup, &run, "finish").await,
        run_id: run.clone(),
        outcome: RunFinishOutcome::Succeeded,
    }))
    .await
    .unwrap();
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.state, RunState::Succeeded);
    assert_eq!(snap.tasks.get(&root).unwrap().state, TaskState::Succeeded);
}

#[tokio::test]
async fn invalid_state_transitions_rejected() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = AttemptId::new("att_bad");
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "create_bad").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        delivery_key: None,
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::CompleteAttempt(CompleteAttempt {
            envelope: next_env(&sup, &run, "complete_early").await,
            run_id: run.clone(),
            attempt_id: attempt,
            task_version: 1,
            workspace_version: None,
            input_digest: input_digest_for_task(&sup, &run, &root).await,
            result_digest: "early".into(),
            lease_proof: LeaseProof {
                lease_id: LeaseId::new("lease_unused"),
                lease_epoch: 1,
                holder: ExecutionTargetId::worker("worker"),
            },
        }))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::InvalidTransition(_)
            | RunSupervisorError::StaleResult(_)
            | RunSupervisorError::StaleLeaseEpoch { .. }
    ));
}

#[tokio::test]
async fn concurrent_completion_race() {
    let sup = Arc::new(mem_supervisor());
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let seq = current_seq(&sup, &run).await;
    let input_digest = input_digest_for_task(&sup, &run, &root).await;
    let digest_a = input_digest.clone();
    let digest_b = input_digest;
    let proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
    let s1 = sup.clone();
    let s2 = sup.clone();
    let r1 = run.clone();
    let r2 = run.clone();
    let a1 = attempt.clone();
    let a2 = attempt.clone();
    let p1 = proof.clone();
    let p2 = proof;
    let t1 = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            s1.handle(RunCommand::CompleteAttempt(CompleteAttempt {
                envelope: command_envelope(format!("race_a_{seq}"), Some(seq), "test"),
                run_id: r1,
                attempt_id: a1,
                task_version: 1,
                workspace_version: None,
                input_digest: digest_a,
                result_digest: "d1".into(),
                lease_proof: p1,
            }))
            .await
        })
    });
    let t2 = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            s2.handle(RunCommand::CompleteAttempt(CompleteAttempt {
                envelope: command_envelope(format!("race_b_{seq}"), Some(seq), "test"),
                run_id: r2,
                attempt_id: a2,
                task_version: 1,
                workspace_version: None,
                input_digest: digest_b,
                result_digest: "d2".into(),
                lease_proof: p2,
            }))
            .await
        })
    });
    let results = [t1.join().unwrap(), t2.join().unwrap()];
    let wins = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(wins, 1);
    assert!(results.iter().any(|r| r.is_err()));
}

#[tokio::test]
async fn cycle_insertion_rejected() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_b").await,
        run_id: run.clone(),
        task_id: TaskId::new("task_b"),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_b").await,
        run_id: run.clone(),
        task_id: TaskId::new("task_b"),
        depends_on: root.clone(),
        policy: DependencyPolicy::RequireSuccess,
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::AddDependency(AddDependency {
            envelope: next_env(&sup, &run, "dep_cycle").await,
            run_id: run.clone(),
            task_id: root.clone(),
            depends_on: TaskId::new("task_b"),
            policy: DependencyPolicy::RequireSuccess,
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::CycleDetected));
}

#[tokio::test]
async fn dependency_added_after_lease_rejected() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_other").await,
        run_id: run.clone(),
        task_id: TaskId::new("task_other"),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    let _attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let err = sup
        .handle(RunCommand::AddDependency(AddDependency {
            envelope: next_env(&sup, &run, "dep_late").await,
            run_id: run.clone(),
            task_id: root.clone(),
            depends_on: TaskId::new("task_other"),
            policy: DependencyPolicy::RequireSuccess,
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::DependencyLocked));
}

#[tokio::test]
async fn dependency_failure_policies() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let dep = TaskId::new("task_dep");
    let consumer = TaskId::new("task_consumer");
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_dep").await,
        run_id: run.clone(),
        task_id: dep.clone(),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_consumer").await,
        run_id: run.clone(),
        task_id: consumer.clone(),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_req").await,
        run_id: run.clone(),
        task_id: consumer.clone(),
        depends_on: dep.clone(),
        policy: DependencyPolicy::RequireSuccess,
    }))
    .await
    .unwrap();
    let dep_attempt = ready_task_with_attempt(&sup, &run, &dep).await;
    fail_running_attempt(
        &sup,
        &run,
        dep_attempt,
        FailureClass::PermanentExecutionFailure,
        "fail",
    )
    .await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&consumer).unwrap().state, TaskState::Skipped);
}

async fn fail_dependency_task(sup: &DurableRunSupervisor, run: &RunId, dep: &TaskId) {
    let dep_attempt = ready_task_with_attempt(sup, run, dep).await;
    fail_running_attempt(
        sup,
        run,
        dep_attempt,
        FailureClass::PermanentExecutionFailure,
        "fail",
    )
    .await;
}

#[tokio::test]
async fn dependency_allow_partial_on_failure() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let dep = TaskId::new("task_dep");
    let consumer = TaskId::new("task_consumer");
    for task in [&dep, &consumer] {
        sup.handle(RunCommand::AddTask(AddTask {
            envelope: next_env(&sup, &run, &format!("add_{task}")).await,
            run_id: run.clone(),
            task_id: (*task).clone(),
            binding: TaskInputBinding::default(),
        }))
        .await
        .unwrap();
    }
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_partial").await,
        run_id: run.clone(),
        task_id: consumer.clone(),
        depends_on: dep.clone(),
        policy: DependencyPolicy::AllowPartial,
    }))
    .await
    .unwrap();
    fail_dependency_task(&sup, &run, &dep).await;
    assert_eq!(
        task_state(&sup.snapshot(run).await.unwrap(), &consumer),
        TaskState::Ready
    );
}

#[tokio::test]
async fn dependency_continue_on_failure() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let dep = TaskId::new("task_dep");
    let consumer = TaskId::new("task_consumer");
    for task in [&dep, &consumer] {
        sup.handle(RunCommand::AddTask(AddTask {
            envelope: next_env(&sup, &run, &format!("add_{task}")).await,
            run_id: run.clone(),
            task_id: (*task).clone(),
            binding: TaskInputBinding::default(),
        }))
        .await
        .unwrap();
    }
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_continue").await,
        run_id: run.clone(),
        task_id: consumer.clone(),
        depends_on: dep.clone(),
        policy: DependencyPolicy::ContinueOnFailure,
    }))
    .await
    .unwrap();
    fail_dependency_task(&sup, &run, &dep).await;
    assert_eq!(
        task_state(&sup.snapshot(run).await.unwrap(), &consumer),
        TaskState::Ready
    );
}

#[tokio::test]
async fn dependency_fallback_task_on_failure() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let dep = TaskId::new("task_dep");
    let consumer = TaskId::new("task_consumer");
    let fallback = TaskId::new("task_fallback");
    for task in [&dep, &consumer, &fallback] {
        sup.handle(RunCommand::AddTask(AddTask {
            envelope: next_env(&sup, &run, &format!("add_{task}")).await,
            run_id: run.clone(),
            task_id: (*task).clone(),
            binding: TaskInputBinding::default(),
        }))
        .await
        .unwrap();
    }
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_fallback").await,
        run_id: run.clone(),
        task_id: consumer.clone(),
        depends_on: dep.clone(),
        policy: DependencyPolicy::FallbackTask(fallback.clone()),
    }))
    .await
    .unwrap();
    fail_dependency_task(&sup, &run, &dep).await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&consumer).unwrap().state, TaskState::Skipped);
    assert_eq!(snap.tasks.get(&fallback).unwrap().state, TaskState::Ready);
}

#[tokio::test]
async fn run_canceled_while_task_starts() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let _ = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CancelRun(CancelRun {
        envelope: next_env(&sup, &run, "cancel_run").await,
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.state, RunState::Canceled);
}

#[tokio::test]
async fn attempt_completes_after_task_cancellation() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CancelTask(CancelTask {
        envelope: next_env(&sup, &run, "cancel_task").await,
        run_id: run.clone(),
        task_id: root.clone(),
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::CompleteAttempt(
            complete_cmd(&sup, &run, &root, attempt, "late").await,
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::InvalidTransition(_) | RunSupervisorError::StaleResult(_)
    ));
}

#[tokio::test]
async fn restart_during_task_states() {
    let (sup, store) = db_supervisor();
    let session = tetonic_domain::SessionId::new("sess_restart");
    let run = RunId::new("run_restart");
    let root = TaskId::new("task_root");
    sup.handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
        envelope: env(None, "create_restart"),
        session_id: Some(session.clone()),
        run_id: run.clone(),
        root_task_id: root.clone(),
        root_binding: TaskInputBinding::default(),
        speculation: None,
        job_spec: None,
    }))
    .await
    .unwrap();
    assert_eq!(
        sup2_snapshot(&store, &run)
            .await
            .tasks
            .get(&root)
            .unwrap()
            .state,
        TaskState::Created
    );

    sup.handle(RunCommand::StartRun(tetonic_domain::StartRun {
        envelope: env(Some(1), "start_restart"),
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    assert_eq!(
        sup2_snapshot(&store, &run)
            .await
            .tasks
            .get(&root)
            .unwrap()
            .state,
        TaskState::Ready
    );

    let blocked = TaskId::new("task_blocked");
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_blocked").await,
        run_id: run.clone(),
        task_id: blocked.clone(),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::AddDependency(AddDependency {
        envelope: next_env(&sup, &run, "dep_blocked").await,
        run_id: run.clone(),
        task_id: blocked.clone(),
        depends_on: root.clone(),
        policy: DependencyPolicy::RequireSuccess,
    }))
    .await
    .unwrap();
    assert_eq!(
        sup2_snapshot(&store, &run)
            .await
            .tasks
            .get(&blocked)
            .unwrap()
            .state,
        TaskState::Blocked
    );

    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "create_restart_att").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: AttemptId::new("att_restart"),
        delivery_key: None,
    }))
    .await
    .unwrap();
    assert_eq!(
        sup2_snapshot(&store, &run)
            .await
            .tasks
            .get(&root)
            .unwrap()
            .state,
        TaskState::Leased
    );

    sup.handle(RunCommand::LeaseAttempt(default_lease_cmd(
        next_env(&sup, &run, "lease_restart").await,
        run.clone(),
        AttemptId::new("att_restart"),
    )))
    .await
    .unwrap();
    let proof = lease_proof_from_snapshot(&sup, &run, &AttemptId::new("att_restart")).await;
    sup.handle(RunCommand::StartAttempt(StartAttempt {
        envelope: next_env(&sup, &run, "start_restart").await,
        run_id: run.clone(),
        attempt_id: AttemptId::new("att_restart"),
        lease_proof: proof,
    }))
    .await
    .unwrap();
    assert_eq!(
        sup2_snapshot(&store, &run)
            .await
            .tasks
            .get(&root)
            .unwrap()
            .state,
        TaskState::Leased
    );
}

#[tokio::test]
async fn snapshot_plus_event_replay_equivalence() {
    let (sup, _) = db_supervisor();
    let (run, root, session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:abc").await,
    ))
    .await
    .unwrap();
    let projected = sup.snapshot(run.clone()).await.unwrap();
    let replayed = sup.replay_run(&run).unwrap();
    assert_eq!(projected.sequence, replayed.sequence);
    assert_eq!(
        projected.tasks.get(&root).unwrap().state,
        replayed.tasks.get(&root).unwrap().state
    );
    let _ = session;
}

#[tokio::test]
async fn stale_expected_run_sequence() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let err = sup
        .handle(RunCommand::MarkTaskReady(tetonic_domain::MarkTaskReady {
            envelope: env(Some(99), "stale_seq"),
            run_id: run.clone(),
            task_id: root.clone(),
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::StaleSequence { .. }));
}

#[tokio::test]
async fn dynamic_task_insertion_during_execution() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let _ = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::AddTask(AddTask {
        envelope: next_env(&sup, &run, "add_dynamic").await,
        run_id: run.clone(),
        task_id: TaskId::new("task_dynamic"),
        binding: TaskInputBinding::default(),
    }))
    .await
    .unwrap();
    let snap = sup.snapshot(run).await.unwrap();
    assert!(snap.tasks.contains_key(&TaskId::new("task_dynamic")));
}

#[tokio::test]
async fn task_input_changes_after_attempt_creation() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let snap = sup.snapshot(run.clone()).await.unwrap();
    assert_eq!(
        snap.attempts.get(&attempt).unwrap().task_version,
        snap.tasks
            .get(&root)
            .unwrap()
            .binding
            .task_definition_version
    );
    let mut cmd = complete_cmd(&sup, &run, &root, attempt.clone(), "x").await;
    cmd.task_version = 99;
    let err = sup
        .handle(RunCommand::CompleteAttempt(cmd))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::Conflict(_)));
}

#[tokio::test]
async fn canceled_run_rejects_new_tasks() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::CancelRun(CancelRun {
        envelope: next_env(&sup, &run, "cancel").await,
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::AddTask(AddTask {
            envelope: next_env(&sup, &run, "add_late").await,
            run_id: run.clone(),
            task_id: TaskId::new("task_late"),
            binding: TaskInputBinding::default(),
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::RunNotAccepting(_)));
}

#[tokio::test]
async fn cycle_self_dependency_rejected() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let err = sup
        .handle(RunCommand::AddDependency(AddDependency {
            envelope: next_env(&sup, &run, "self_dep").await,
            run_id: run.clone(),
            task_id: root.clone(),
            depends_on: root.clone(),
            policy: DependencyPolicy::RequireSuccess,
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::CycleDetected));
}

#[tokio::test]
async fn artifact_rejection_refuses_cross_task_identity_and_missing_projection() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:abc").await,
    ))
    .await
    .unwrap();
    let mut snapshot = sup.snapshot(run.clone()).await.unwrap();
    let mut cmd = tetonic_domain::RejectArtifact {
        envelope: next_env(&sup, &run, "reject_bad").await,
        run_id: run,
        task_id: TaskId::new("other_task"),
        attempt_id: attempt,
        artifact_id: "art_1".into(),
        reason: "invalid".into(),
    };
    assert!(matches!(
        tetonic_run::transition::apply_command(&snapshot, &RunCommand::RejectArtifact(cmd.clone())),
        Err(RunSupervisorError::Conflict(_))
    ));
    cmd.task_id = root.clone();
    snapshot.tasks.remove(&root);
    assert!(matches!(
        tetonic_run::transition::apply_command(&snapshot, &RunCommand::RejectArtifact(cmd)),
        Err(RunSupervisorError::TaskNotFound(_))
    ));
}
