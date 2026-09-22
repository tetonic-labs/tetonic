//! M3-2 required fault-injection tests — distributed failure semantics.

mod harness;

use std::sync::Arc;
use std::thread;

use harness::*;
use tetonic_domain::{
    AttemptId, CancelRun, CommandEnvelope, CompleteAttempt, CreateAttempt, DependencyPolicy,
    ExecutionTargetId, ExpireLease, FailAttempt, FailureClass, HeartbeatReport, LeaseAttempt,
    LeaseId, MarkTaskReady, RecordHeartbeat, RecordSideEffectCommit, RejectArtifact, RetryPolicy,
    RunCommand, RunId, RunState, RunSupervisorError, StartAttempt, TaskId, TaskInputBinding,
    TaskState, TimeoutKind,
};
use tetonic_run::{command_envelope, DurableRunSupervisor, RunSupervisor};

#[tokio::test]
async fn duplicate_supervisor_command() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let cmd = RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: env(None, "dup_cmd"),
        run_id: run.clone(),
        task_id: root.clone(),
    });
    let first = sup.handle(cmd.clone()).await.unwrap();
    let second = sup.handle(cmd).await.unwrap();
    assert_eq!(first.sequence, second.sequence);
    assert!(second.idempotent_replay);
    assert_eq!(
        first.snapshot.tasks.get(&root).unwrap().state,
        second.snapshot.tasks.get(&root).unwrap().state
    );
}

#[tokio::test]
async fn duplicate_worker_dispatch() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: next_env(&sup, &run, "ready").await,
        run_id: run.clone(),
        task_id: root.clone(),
    }))
    .await
    .unwrap();
    let attempt = AttemptId::new("att_dispatch");
    let delivery = "delivery:worker:1".to_string();
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "dispatch_1").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        delivery_key: Some(delivery.clone()),
    }))
    .await
    .unwrap();
    let snap = sup.snapshot(run.clone()).await.unwrap();
    assert_eq!(snap.attempts.len(), 1);
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "dispatch_2").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        delivery_key: Some(delivery),
    }))
    .await
    .unwrap();
    let snap2 = sup.snapshot(run).await.unwrap();
    assert_eq!(snap2.attempts.len(), 1);
}

#[tokio::test]
async fn duplicate_result() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:dup").await,
    ))
    .await
    .unwrap();
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:dup").await,
    ))
    .await
    .unwrap();
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(
        snap.attempts.get(&attempt).unwrap().state,
        tetonic_domain::AttemptState::Succeeded
    );
}

#[tokio::test]
async fn result_after_lease_expiration() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::ExpireLease(ExpireLease {
        envelope: next_env(&sup, &run, "expire").await,
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        expired_at: 100,
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
        RunSupervisorError::StaleResult(_) | RunSupervisorError::InvalidTransition(_)
    ));
}

#[tokio::test]
async fn result_from_old_lease_epoch() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let mut proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
    proof.lease_epoch = proof.lease_epoch.saturating_sub(1);
    let input_digest = input_digest_for_task(&sup, &run, &root).await;
    let err = sup
        .handle(RunCommand::CompleteAttempt(CompleteAttempt {
            envelope: next_env(&sup, &run, "stale_epoch").await,
            run_id: run.clone(),
            attempt_id: attempt,
            task_version: 1,
            workspace_version: None,
            input_digest,
            result_digest: "sha256:stale".into(),
            lease_proof: proof,
        }))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::StaleLeaseEpoch { .. } | RunSupervisorError::StaleResult(_)
    ));
}

#[tokio::test]
async fn two_attempts_completing_concurrently() {
    let sup = Arc::new(mem_supervisor());
    let (run, root, _) = create_started_run_speculative(&sup, 2).await;
    sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: next_env(&sup, &run, "ready").await,
        run_id: run.clone(),
        task_id: root.clone(),
    }))
    .await
    .unwrap();
    let att1 = AttemptId::new("att_spec_1");
    let att2 = AttemptId::new("att_spec_2");
    for att in [&att1, &att2] {
        sup.handle(RunCommand::CreateAttempt(CreateAttempt {
            envelope: next_env(&sup, &run, &format!("create_{att}")).await,
            run_id: run.clone(),
            task_id: root.clone(),
            attempt_id: (*att).clone(),
            delivery_key: None,
        }))
        .await
        .unwrap();
    }
    for att in [&att1, &att2] {
        sup.handle(RunCommand::LeaseAttempt(default_lease_cmd(
            next_env(&sup, &run, &format!("lease_{att}")).await,
            run.clone(),
            (*att).clone(),
        )))
        .await
        .unwrap();
        let proof = lease_proof_from_snapshot(&sup, &run, att).await;
        sup.handle(RunCommand::StartAttempt(StartAttempt {
            envelope: next_env(&sup, &run, &format!("start_{att}")).await,
            run_id: run.clone(),
            attempt_id: (*att).clone(),
            lease_proof: proof,
        }))
        .await
        .unwrap();
    }
    let seq = current_seq(&sup, &run).await;
    let input_digest = input_digest_for_task(&sup, &run, &root).await;
    let digest_a = input_digest.clone();
    let digest_b = input_digest;
    let s1 = sup.clone();
    let s2 = sup.clone();
    let r1 = run.clone();
    let r2 = run.clone();
    let p1 = lease_proof_from_snapshot(&sup, &run, &att1).await;
    let p2 = lease_proof_from_snapshot(&sup, &run, &att2).await;
    let t1 = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            s1.handle(RunCommand::CompleteAttempt(CompleteAttempt {
                envelope: command_envelope("win_a", Some(seq), "test"),
                run_id: r1,
                attempt_id: att1,
                task_version: 1,
                workspace_version: None,
                input_digest: digest_a,
                result_digest: "sha256:a".into(),
                lease_proof: p1,
            }))
            .await
        })
    });
    let t2 = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            s2.handle(RunCommand::CompleteAttempt(CompleteAttempt {
                envelope: command_envelope("win_b", Some(seq), "test"),
                run_id: r2,
                attempt_id: att2,
                task_version: 1,
                workspace_version: None,
                input_digest: digest_b,
                result_digest: "sha256:b".into(),
                lease_proof: p2,
            }))
            .await
        })
    });
    let results = [t1.join().unwrap(), t2.join().unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
}

#[tokio::test]
async fn coordinator_restart_while_lease_is_active() {
    let (sup, store) = db_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: next_env(&sup, &run, "ready").await,
        run_id: run.clone(),
        task_id: root.clone(),
    }))
    .await
    .unwrap();
    let attempt = AttemptId::new("att_recovery");
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "create").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        delivery_key: None,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::LeaseAttempt(LeaseAttempt {
        envelope: next_env(&sup, &run, "lease").await,
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        lease_id: LeaseId::new("lease_rec"),
        lease_epoch: 0,
        holder: ExecutionTargetId::worker("worker"),
        issued_at: 1,
        expires_at: 5,
        heartbeat_interval_secs: 1,
    }))
    .await
    .unwrap();
    let sup2 = DurableRunSupervisor::new(Some(store.clone()));
    let snap = sup2.snapshot(run.clone()).await.unwrap();
    assert!(
        snap.tasks.get(&root).unwrap().state == TaskState::Ready
            || snap.tasks.get(&root).unwrap().state == TaskState::Leased
            || snap.attempts.get(&attempt).unwrap().state
                == tetonic_domain::AttemptState::LeaseExpired
    );
}

#[tokio::test]
async fn cancellation_during_heartbeat_renewal() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
        envelope: next_env(&sup, &run, "ready").await,
        run_id: run.clone(),
        task_id: root.clone(),
    }))
    .await
    .unwrap();
    let attempt = AttemptId::new("att_hb");
    sup.handle(RunCommand::CreateAttempt(CreateAttempt {
        envelope: next_env(&sup, &run, "create").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt.clone(),
        delivery_key: None,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::LeaseAttempt(default_lease_cmd(
        next_env(&sup, &run, "lease").await,
        run.clone(),
        attempt.clone(),
    )))
    .await
    .unwrap();
    let proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
    sup.handle(RunCommand::StartAttempt(StartAttempt {
        envelope: next_env(&sup, &run, "start").await,
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        lease_proof: proof.clone(),
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::CancelRun(CancelRun {
        envelope: next_env(&sup, &run, "cancel").await,
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::RecordHeartbeat(RecordHeartbeat {
            envelope: next_env(&sup, &run, "heartbeat").await,
            run_id: run.clone(),
            attempt_id: attempt,
            lease_proof: proof,
            heartbeat_sequence: 1,
            report: HeartbeatReport {
                attempt_state: "running".into(),
                progress_marker: None,
                resource_usage: None,
                output_size_bytes: None,
                lease_renewal_requested: true,
                executor_health: None,
            },
            renewed_expires_at: Some(u64::MAX),
        }))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::InvalidTransition(_) | RunSupervisorError::RunNotAccepting(_)
    ));
}

#[tokio::test]
async fn cancellation_during_process_execution() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CancelRun(CancelRun {
        envelope: next_env(&sup, &run, "cancel").await,
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::CompleteAttempt(
            complete_cmd(&sup, &run, &root, attempt, "late_ok").await,
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::InvalidTransition(_)
            | RunSupervisorError::StaleResult(_)
            | RunSupervisorError::RunNotAccepting(_)
    ));
    let snap = sup.snapshot(run).await.unwrap();
    assert_ne!(snap.state, RunState::Succeeded);
}

#[tokio::test]
async fn timeout_followed_by_late_success() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
    sup.handle(RunCommand::FailAttempt(FailAttempt {
        envelope: next_env(&sup, &run, "timeout").await,
        run_id: run.clone(),
        attempt_id: attempt.clone(),
        failure_class: FailureClass::TimedOut,
        reason: "attempt execution timeout".into(),
        lease_proof: Some(proof.clone()),
        timeout_kind: Some(TimeoutKind::AttemptExecution),
    }))
    .await
    .unwrap();
    let err = sup
        .handle(RunCommand::CompleteAttempt(CompleteAttempt {
            envelope: next_env(&sup, &run, "late_success").await,
            run_id: run.clone(),
            attempt_id: attempt,
            task_version: 1,
            workspace_version: None,
            input_digest: input_digest_for_task(&sup, &run, &root).await,
            result_digest: "sha256:late".into(),
            lease_proof: proof,
        }))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RunSupervisorError::StaleResult(_) | RunSupervisorError::InvalidTransition(_)
    ));
}

#[tokio::test]
async fn retry_after_committed_side_effect() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::RecordSideEffectCommit(RecordSideEffectCommit {
        envelope: next_env(&sup, &run, "side_effect").await,
        run_id: run.clone(),
        task_id: root.clone(),
        operation_key: "txn_write".into(),
        transaction_id: None,
        committed_at: 1,
    }))
    .await
    .unwrap();
    fail_running_attempt(
        &sup,
        &run,
        attempt,
        FailureClass::TransientTransport,
        "retry blocked",
    )
    .await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&root).unwrap().state, TaskState::Failed);
}

#[tokio::test]
async fn permanent_failure_incorrectly_marked_transient() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    fail_running_attempt(
        &sup,
        &run,
        attempt,
        FailureClass::PermanentExecutionFailure,
        "permanent",
    )
    .await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&root).unwrap().state, TaskState::Failed);
    assert_eq!(
        snap.tasks.get(&root).unwrap().retry.last_failure_class,
        Some(FailureClass::PermanentExecutionFailure)
    );
}

#[tokio::test]
async fn dependency_failure_under_every_policy() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let dep = TaskId::new("dep_all");
    let policies = [
        (DependencyPolicy::RequireSuccess, TaskState::Skipped),
        (DependencyPolicy::AllowPartial, TaskState::Ready),
        (DependencyPolicy::ContinueOnFailure, TaskState::Ready),
    ];
    for (i, (policy, _expected)) in policies.into_iter().enumerate() {
        let consumer = TaskId::new(format!("consumer_{i}"));
        for task in [&dep, &consumer] {
            if !sup
                .snapshot(run.clone())
                .await
                .unwrap()
                .tasks
                .contains_key(task)
            {
                sup.handle(RunCommand::AddTask(tetonic_domain::AddTask {
                    envelope: next_env(&sup, &run, &format!("add_{task}")).await,
                    run_id: run.clone(),
                    task_id: (*task).clone(),
                    binding: TaskInputBinding::default(),
                }))
                .await
                .unwrap();
            }
        }
        sup.handle(RunCommand::AddDependency(tetonic_domain::AddDependency {
            envelope: next_env(&sup, &run, &format!("dep_{i}")).await,
            run_id: run.clone(),
            task_id: consumer.clone(),
            depends_on: dep.clone(),
            policy,
        }))
        .await
        .unwrap();
    }
    fail_dependency_task(&sup, &run, &dep).await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(
        snap.tasks.get(&TaskId::new("consumer_0")).unwrap().state,
        TaskState::Skipped
    );
    assert_eq!(
        snap.tasks.get(&TaskId::new("consumer_1")).unwrap().state,
        TaskState::Ready
    );
    assert_eq!(
        snap.tasks.get(&TaskId::new("consumer_2")).unwrap().state,
        TaskState::Ready
    );
}

#[tokio::test]
async fn resource_exhaustion() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    fail_running_attempt(&sup, &run, attempt, FailureClass::ResourceExhausted, "oom").await;
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&root).unwrap().state, TaskState::Ready);
    assert!(snap.tasks.get(&root).unwrap().retry.next_retry_at.is_some());
}

#[tokio::test]
async fn verification_failure_remediation() {
    let sup = mem_supervisor();
    let session = tetonic_domain::SessionId::new("sess_verify");
    let run = RunId::new("run_verify");
    let root = TaskId::new("task_root");
    let mut binding = TaskInputBinding::default();
    binding
        .verification_policy
        .remediation
        .retry_with_revised_input = true;
    sup.handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
        envelope: env(None, "create_verify"),
        session_id: Some(session),
        run_id: run.clone(),
        root_task_id: root.clone(),
        root_binding: binding,
        speculation: None,
        job_spec: None,
    }))
    .await
    .unwrap();
    sup.handle(RunCommand::StartRun(tetonic_domain::StartRun {
        envelope: env(Some(1), "start_verify"),
        run_id: run.clone(),
    }))
    .await
    .unwrap();
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    sup.handle(RunCommand::CompleteAttempt(
        complete_cmd(&sup, &run, &root, attempt.clone(), "sha256:verify_me").await,
    ))
    .await
    .unwrap();
    sup.handle(RunCommand::RejectArtifact(RejectArtifact {
        envelope: next_env(&sup, &run, "reject").await,
        run_id: run.clone(),
        task_id: root.clone(),
        attempt_id: attempt,
        artifact_id: "art_verify".into(),
        reason: "verification failed".into(),
    }))
    .await
    .unwrap();
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&root).unwrap().state, TaskState::Ready);
    assert_eq!(
        snap.tasks.get(&root).unwrap().retry.last_failure_class,
        Some(FailureClass::VerificationFailed)
    );
}

#[tokio::test]
async fn result_with_mismatched_input_digest() {
    let sup = mem_supervisor();
    let (run, root, _) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &root).await;
    let proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
    let err = sup
        .handle(RunCommand::CompleteAttempt(CompleteAttempt {
            envelope: next_env(&sup, &run, "bad_digest").await,
            run_id: run.clone(),
            attempt_id: attempt,
            task_version: 1,
            workspace_version: None,
            input_digest: "input:wrong".into(),
            result_digest: "sha256:ok".into(),
            lease_proof: proof,
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::Conflict(_)));
}

#[tokio::test]
async fn retry_limits_and_backoff_enforced() {
    let sup = mem_supervisor();
    let (run, _, _) = create_started_run(&sup).await;
    let task = TaskId::new("task_retry");
    let binding = TaskInputBinding {
        retry_policy: RetryPolicy {
            max_attempts: 2,
            retryable_classes: vec![FailureClass::TransientTransport],
            initial_delay_ms: 5_000,
            backoff_factor: 2.0,
            max_delay_ms: 10_000,
            jitter: false,
            require_different_target: false,
            refresh_workspace: false,
        },
        ..TaskInputBinding::default()
    };
    sup.handle(RunCommand::AddTask(tetonic_domain::AddTask {
        envelope: next_env(&sup, &run, "add_retry_task").await,
        run_id: run.clone(),
        task_id: task.clone(),
        binding,
    }))
    .await
    .unwrap();
    for attempt_round in 0..2 {
        let ts = 10_000 + attempt_round * 10_000;
        sup.handle(RunCommand::MarkTaskReady(MarkTaskReady {
            envelope: CommandEnvelope {
                command_id: format!("ready_retry_{attempt_round}"),
                expected_sequence: None,
                trace: Default::default(),
                actor: "test".into(),
                timestamp: ts,
                workspace_version: None,
                idempotency_key: None,
            },
            run_id: run.clone(),
            task_id: task.clone(),
        }))
        .await
        .unwrap();
        let attempt = AttemptId::new(format!("att_retry_{attempt_round}"));
        sup.handle(RunCommand::CreateAttempt(CreateAttempt {
            envelope: next_env(&sup, &run, &format!("create_retry_{attempt_round}")).await,
            run_id: run.clone(),
            task_id: task.clone(),
            attempt_id: attempt.clone(),
            delivery_key: None,
        }))
        .await
        .unwrap();
        sup.handle(RunCommand::LeaseAttempt(default_lease_cmd(
            next_env(&sup, &run, &format!("lease_retry_{attempt_round}")).await,
            run.clone(),
            attempt.clone(),
        )))
        .await
        .unwrap();
        let proof = lease_proof_from_snapshot(&sup, &run, &attempt).await;
        sup.handle(RunCommand::StartAttempt(StartAttempt {
            envelope: next_env(&sup, &run, &format!("start_retry_{attempt_round}")).await,
            run_id: run.clone(),
            attempt_id: attempt.clone(),
            lease_proof: proof,
        }))
        .await
        .unwrap();
        fail_running_attempt(
            &sup,
            &run,
            attempt,
            FailureClass::TransientTransport,
            "transient",
        )
        .await;
    }
    let err = sup
        .handle(RunCommand::MarkTaskReady(MarkTaskReady {
            envelope: CommandEnvelope {
                command_id: "ready_retry_blocked".into(),
                expected_sequence: None,
                trace: Default::default(),
                actor: "test".into(),
                timestamp: 100_000,
                workspace_version: None,
                idempotency_key: None,
            },
            run_id: run.clone(),
            task_id: task.clone(),
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, RunSupervisorError::InvalidTransition(_)));
    let snap = sup.snapshot(run).await.unwrap();
    assert_eq!(snap.tasks.get(&task).unwrap().state, TaskState::Failed);
}
