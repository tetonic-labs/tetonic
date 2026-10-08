use super::*;
use crate::{ContextOwner, TeamRow};
use tetonic_domain::{ActivationBinding, ExecutionScope, RunSnapshot, TaskInputBinding};

#[path = "work_usage_resume_tests.rs"]
mod resume;

fn seed(db: &Store) {
    db.bootstrap_control("owner", "org", "Org").unwrap();
    db.create_team(&TeamRow {
        org_id: "org".into(),
        team_id: "team".into(),
        name: "Team".into(),
        owner_principal_id: "owner".into(),
    })
    .unwrap();
    db.create_information_context(
        "owner",
        "context",
        &ContextOwner::Team {
            org_id: "org".into(),
            team_id: "team".into(),
        },
    )
    .unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "owner",
        org: "org",
        team: "team",
        work_id: "work",
        title: "Work",
        request_id: "request@agent",
        goal_id: None,
    })
    .unwrap();
    db.authorize_work_budget("owner", "org", "team", "work", "fund", 100)
        .unwrap();
    let agent = db
        .register_organization_agent(
            "owner",
            "org",
            "agent",
            "general",
            &serde_json::json!({"instructions":"Help"}),
        )
        .unwrap();
    let job = tetonic_domain::AgentJobSpec {
        identity_id: tetonic_domain::IdentityId::new(agent.identity.identity_id),
        definition_digest: agent.identity.bound_definition_digest,
        input_digest: "input".into(),
        capability_bindings: vec![],
        artifact_bindings: vec![],
        recovery_id: "recovery".into(),
    };
    let binding = TaskInputBinding {
        job_spec: Some(job.clone()),
        execution_scope: Some(ExecutionScope {
            principal_id: "owner".into(),
            organization_id: "org".into(),
            information_context_id: "context".into(),
        }),
        activation: Some(ActivationBinding {
            request_id: work_activation_request_id("request@agent"),
            request_digest: "digest".into(),
            audit_session_id: "audit".into(),
        }),
        ..Default::default()
    };
    let run:RunSnapshot=serde_json::from_value(serde_json::json!({"run_id":"run","state":"active","sequence":1,"workspace_version":null,
        "tasks":{"task":{"task_id":"task","state":"running","binding":binding,"accepted_artifact":null,"active_attempt":"attempt"}},
        "attempts":{"attempt":{"attempt_id":"attempt","task_id":"task","state":"running","execution_claimed":true,"execution_quiesced":false,"task_version":1,"workspace_version":null,"input_digest":"input","result_digest":null,"delivery_key":null,
        "lease":{"lease_id":"lease","attempt_id":"attempt","lease_epoch":1,"holder":"local","issued_at":1,"expires_at":chrono::Utc::now().timestamp()+3600,"heartbeat_interval_secs":30,"last_heartbeat_sequence":0},"failure_class":null,"failure_reason":null}},"dependencies":{},"events":[],"job_spec":job})).unwrap();
    db.persist_run_projection(&run).unwrap();
}
fn begin(db: &Store, call: &str) -> Result<Option<i64>> {
    db.begin_work_inference(crate::BeginWorkInference {
        actor: "owner",
        org: "org",
        team: "team",
        work: "work",
        run: "run",
        task: "task",
        attempt: "attempt",
        call,
        model: "test-model",
        now: 100,
    })
}
fn stop(db: &Store) {
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    run.state = RunState::Canceled;
    let a = run.attempts.get_mut(&AttemptId::new("attempt")).unwrap();
    a.state = AttemptState::Canceled;
    a.execution_quiesced = true;
    db.persist_run_projection(&run).unwrap();
}

fn limited(db: &Store, call: &str, limit: i64) -> Result<Option<i64>> {
    db.begin_work_inference_with_limit(
        crate::BeginWorkInference {
            actor: "owner",
            org: "org",
            team: "team",
            work: "work",
            run: "run",
            task: "task",
            attempt: "attempt",
            call,
            model: "model",
            now: 100,
        },
        Some(limit),
    )
}

fn add_child(db: &Store) -> RunSnapshot {
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "owner",
        org: "org",
        team: "team",
        delegation_id: "delegation",
        parent_work_id: "work",
        child_work_id: "child",
        child_title: "Help",
        request_id: "child-request",
        parent_budget_tokens: 100,
        child_budget_tokens: 50,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
    .unwrap();
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    let mut task = run.tasks[&TaskId::new("task")].clone();
    task.task_id = TaskId::new("child-task");
    task.active_attempt = Some(AttemptId::new("child-attempt"));
    task.binding.job_spec.as_mut().unwrap().identity_id =
        tetonic_domain::IdentityId::new("child-agent");
    task.binding.execution_grant_id = Some("derived-grant".into());
    task.binding.delegation = Some(tetonic_domain::DelegatedTaskBinding {
        parent_lease: None,
        parent_attempt: AttemptId::new("attempt"),
        activation: ActivationBinding {
            request_id: work_activation_request_id("child-request/child"),
            request_digest: "child-digest".into(),
            audit_session_id: "child-audit".into(),
        },
    });
    task.binding.activation = None;
    let mut attempt = run.attempts[&AttemptId::new("attempt")].clone();
    attempt.attempt_id = AttemptId::new("child-attempt");
    attempt.task_id = task.task_id.clone();
    attempt.lease.as_mut().unwrap().attempt_id = attempt.attempt_id.clone();
    run.tasks.insert(task.task_id.clone(), task);
    run.attempts.insert(attempt.attempt_id.clone(), attempt);
    run
}

fn begin_child(db: &Store, call: &str) -> Result<Option<i64>> {
    db.begin_work_inference(crate::BeginWorkInference {
        actor: "owner",
        org: "org",
        team: "team",
        work: "child",
        run: "run",
        task: "child-task",
        attempt: "child-attempt",
        call,
        model: "model",
        now: 100,
    })
}

#[test]
fn parent_own_share_leaves_funded_capacity_for_later_delegation() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    assert_eq!(limited(&db, "parent-1", 30).unwrap(), Some(30));
    let run = add_child(&db);
    db.persist_run_projection(&run).unwrap();
    assert_eq!(begin_child(&db, "child-1").unwrap(), Some(50));
    db.finish_work_inference("parent-1", Some(4), Some(6))
        .unwrap();
    assert_eq!(
        limited(&db, "parent-2", 100).unwrap(),
        Some(20),
        "changing a call cannot enlarge an attempt's own share"
    );
    let budget = db.work_budget("owner", "org", "team", "work").unwrap();
    assert_eq!(budget.available_tokens, 20);
    assert_eq!(
        db.work_budget("owner", "org", "team", "child")
            .unwrap()
            .available_tokens,
        0
    );
}

#[test]
fn one_branch_overrun_stops_new_tree_spend_but_keeps_inflight_reports() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    limited(&db, "parent", 30).unwrap();
    let run = add_child(&db);
    db.persist_run_projection(&run).unwrap();
    begin_child(&db, "child").unwrap();
    db.finish_work_inference("parent", Some(20), Some(15))
        .unwrap();
    db.finish_work_inference("child", Some(1), Some(2)).unwrap();
    assert!(begin_child(&db, "child-again").is_err());
    assert!(db
        .reserve_work_budget("owner", "org", "team", "child", "new-reservation", 1)
        .is_err());
    assert!(db
        .create_work_delegation(crate::CreateWorkDelegation {
            actor: "owner",
            org: "org",
            team: "team",
            delegation_id: "more",
            parent_work_id: "work",
            child_work_id: "more-work",
            child_title: "Help",
            request_id: "more-request",
            parent_budget_tokens: 100,
            child_budget_tokens: 1,
            stop_scope: "inherit",
            peer_org: None,
            peer_team: None
        })
        .is_err());
    let usage = db.team_work_usage("owner", "org", "team").unwrap();
    assert!(
        usage
            .iter()
            .find(|row| row.work_id == "work")
            .unwrap()
            .over_limit
    );
    assert_eq!(
        usage
            .iter()
            .map(|row| row.input_tokens + row.output_tokens)
            .sum::<i64>(),
        38
    );
}

#[test]
fn child_slots_count_parent_and_siblings_and_require_quiescence_to_release() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    let mut run = add_child(&db);
    db.set_team_execution_limits("owner", "org", "team", 1, 1)
        .unwrap();
    assert!(matches!(
        db.sync_child_capacity(&run, true),
        Err(StoreError::TeamCapacityExceeded)
    ));
    db.set_team_execution_limits("owner", "org", "team", 2, 2)
        .unwrap();
    db.sync_child_capacity(&run, true).unwrap();
    assert!(matches!(
        db.enforce_registered_capacity("org", "owner", Some("team"), ""),
        Err(StoreError::TeamCapacityExceeded)
    ));
    run.tasks.get_mut(&TaskId::new("child-task")).unwrap().state =
        tetonic_domain::TaskState::Succeeded;
    run.attempts
        .get_mut(&AttemptId::new("child-attempt"))
        .unwrap()
        .state = AttemptState::Succeeded;
    db.sync_child_capacity(&run, true).unwrap();
    assert!(db
        .enforce_registered_capacity("org", "owner", Some("team"), "")
        .is_err());
    run.attempts
        .get_mut(&AttemptId::new("child-attempt"))
        .unwrap()
        .execution_quiesced = true;
    db.sync_child_capacity(&run, true).unwrap();
    db.enforce_registered_capacity("org", "owner", Some("team"), "")
        .unwrap();
}

#[test]
fn child_capacity_upgrade_recovers_interrupted_admission_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v57.db");
    {
        let db = Store::open(&path).unwrap();
        seed(&db);
        let mut run = add_child(&db);
        run.attempts.remove(&AttemptId::new("child-attempt"));
        run.tasks
            .get_mut(&TaskId::new("child-task"))
            .unwrap()
            .active_attempt = None;
        db.persist_run_projection(&run).unwrap();
        db.set_team_execution_limits("owner", "org", "team", 1, 2)
            .unwrap();
        db.remove_agent_edits_schema_for_test();
        db.conn.execute_batch("DROP TABLE work_human_questions; DROP TABLE huddle_execution_directions; DROP TABLE huddle_execution_work; DROP TABLE huddle_executions; DROP TABLE registered_child_capacity; DELETE FROM schema_versions WHERE version>=58;").unwrap();
    }
    let db = Store::open(&path).unwrap();
    assert!(matches!(
        db.preflight_registered_capacity("org", "owner", "context"),
        Err(StoreError::TeamCapacityExceeded)
    ));
}

#[test]
fn child_admission_race_commits_one_capacity_hold_and_one_journal_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capacity-race.db");
    let db = Store::open(&path).unwrap();
    seed(&db);
    let candidate = add_child(&db);
    db.set_team_execution_limits("owner", "org", "team", 1, 2)
        .unwrap();
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|index| {
            let (path, gate, mut snapshot) = (path.clone(), gate.clone(), candidate.clone());
            std::thread::spawn(move || {
                let db = Store::open(path).unwrap();
                let mut task = snapshot.tasks.remove(&TaskId::new("child-task")).unwrap();
                snapshot.attempts.remove(&AttemptId::new("child-attempt"));
                task.task_id = TaskId::new(format!("child-{index}"));
                task.active_attempt = None;
                task.binding.job_spec.as_mut().unwrap().identity_id =
                    tetonic_domain::IdentityId::new(format!("agent-{index}"));
                snapshot.tasks.insert(task.task_id.clone(), task);
                snapshot.sequence = 2;
                let payload = serde_json::json!({"child":index});
                let event = tetonic_domain::RunEventEnvelope {
                    event_id: tetonic_domain::ids::EventId::new(format!("event-{index}")),
                    run_id: snapshot.run_id.clone(),
                    sequence: 2,
                    event_type: tetonic_domain::EventType::Other("child-test".into()),
                    schema_version: 1,
                    command_id: None,
                    causation_id: None,
                    correlation_id: None,
                    actor: tetonic_domain::EventActor {
                        name: "test".into(),
                    },
                    occurred_at: chrono::Utc::now(),
                    recorded_at: chrono::Utc::now(),
                    data_class: tetonic_domain::DataClass::Secret,
                    payload_digest: crate::payload_digest::digest_event_payload(&payload).unwrap(),
                    payload,
                };
                gate.wait();
                db.commit_run_command(&snapshot, &event, None)
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results
        .iter()
        .any(|result| matches!(result, Err(StoreError::TeamCapacityExceeded))));
    assert_eq!(db.list_run_events("run").unwrap().len(), 1);
    assert_eq!(db.load_run_snapshot("run").unwrap().unwrap().tasks.len(), 2);
}
#[test]
fn calls_share_one_attempt_reservation_and_settle_only_after_quiescence() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    assert_eq!(begin(&db, "one").unwrap(), Some(100));
    assert!(begin(&db, "parallel").is_err());
    assert!(db.settle_work_inference("attempt").is_err());
    db.finish_work_inference("one", Some(10), Some(20)).unwrap();
    db.finish_work_inference("one", Some(10), Some(20)).unwrap();
    assert!(db.finish_work_inference("one", Some(11), Some(20)).is_err());
    assert_eq!(begin(&db, "two").unwrap(), Some(70));
    db.finish_work_inference("two", Some(5), Some(15)).unwrap();
    assert_eq!(
        db.team_work_usage("owner", "org", "team").unwrap()[0].held_tokens,
        50
    );
    assert_eq!(
        db.work_budget("owner", "org", "team", "work")
            .unwrap()
            .available_tokens,
        0
    );
    stop(&db);
    db.settle_work_inference("attempt").unwrap();
    db.settle_work_inference("attempt").unwrap();
    let usage = db
        .team_work_usage("owner", "org", "team")
        .unwrap()
        .remove(0);
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.calls,
            usage.held_tokens,
            usage.released_tokens
        ),
        (15, 35, 2, 0, 50)
    );
    assert_eq!(usage.budget.unwrap().available_tokens, 50);
    assert!(begin(&db, "after-stop").is_err());
}
#[test]
fn settlement_rejects_a_different_lease_owner_or_nonterminal_attempt() {
    for changed_owner in [false, true] {
        let db = Store::open(":memory:").unwrap();
        seed(&db);
        begin(&db, "one").unwrap();
        db.finish_work_inference("one", Some(10), Some(20)).unwrap();
        stop(&db);
        let mut run = db.load_run_snapshot("run").unwrap().unwrap();
        let a = run.attempts.get_mut(&AttemptId::new("attempt")).unwrap();
        if changed_owner {
            a.lease.as_mut().unwrap().lease_epoch += 1;
        } else {
            a.state = AttemptState::Starting;
        }
        db.persist_run_projection(&run).unwrap();
        assert!(db.settle_work_inference("attempt").is_err());
        assert_eq!(
            db.work_budget("owner", "org", "team", "work")
                .unwrap()
                .available_tokens,
            0
        );
    }
}

#[test]
fn unknown_usage_survives_restart_and_never_refunds_or_allows_another_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    {
        let db = Store::open(&path).unwrap();
        seed(&db);
        begin(&db, "pending").unwrap();
        stop(&db);
    }
    let db = Store::open(&path).unwrap();
    db.settle_work_inference("attempt").unwrap();
    let usage = db
        .team_work_usage("owner", "org", "team")
        .unwrap()
        .remove(0);
    assert_eq!(
        (
            usage.unknown_calls,
            usage.pending_calls,
            usage.held_tokens,
            usage.released_tokens
        ),
        (1, 0, 100, 0)
    );
    assert_eq!(usage.budget.unwrap().available_tokens, 0);
    assert!(db.team_work_usage("outsider", "org", "team").is_err());
    assert!(begin(&db, "retry").is_err());
}
#[test]
fn partial_reports_and_overruns_deny_further_inference_without_hiding_usage() {
    for report in [(Some(20), None), (Some(80), Some(40))] {
        let db = Store::open(":memory:").unwrap();
        seed(&db);
        begin(&db, "one").unwrap();
        db.finish_work_inference("one", report.0, report.1).unwrap();
        assert!(begin(&db, "two").is_err());
        stop(&db);
        db.settle_work_inference("attempt").unwrap();
        let usage = db
            .team_work_usage("owner", "org", "team")
            .unwrap()
            .remove(0);
        assert_eq!(usage.released_tokens, 0);
        if report.1.is_none() {
            assert_eq!(usage.unknown_calls, 1);
            assert_eq!(usage.held_tokens, 80);
        } else {
            assert!(usage.over_limit);
            assert_eq!(usage.input_tokens + usage.output_tokens, 120);
        }
    }
}
#[test]
fn parent_claim_scope_request_and_lease_fence_are_enforced() {
    for variant in 0..4 {
        let db = Store::open(":memory:").unwrap();
        seed(&db);
        if variant == 0 {
            begin(&db, "one").unwrap();
            db.finish_work_inference("one", Some(1), Some(1)).unwrap();
        }
        let mut run = db.load_run_snapshot("run").unwrap().unwrap();
        match variant {
            0 => {
                run.attempts
                    .get_mut(&AttemptId::new("attempt"))
                    .unwrap()
                    .lease
                    .as_mut()
                    .unwrap()
                    .lease_epoch += 1
            }
            1 => {
                run.attempts
                    .get_mut(&AttemptId::new("attempt"))
                    .unwrap()
                    .execution_claimed = false
            }
            2 => {
                run.tasks
                    .get_mut(&TaskId::new("task"))
                    .unwrap()
                    .binding
                    .activation
                    .as_mut()
                    .unwrap()
                    .request_id = "other".into()
            }
            _ => {
                run.tasks
                    .get_mut(&TaskId::new("task"))
                    .unwrap()
                    .binding
                    .execution_scope
                    .as_mut()
                    .unwrap()
                    .organization_id = "other".into()
            }
        }
        db.persist_run_projection(&run).unwrap();
        assert!(begin(&db, "blocked").is_err());
    }
}
#[test]
fn concurrent_calls_cannot_consume_the_same_attempt_allowance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.db");
    let db = Store::open(&path).unwrap();
    seed(&db);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let path = path.clone();
            let gate = barrier.clone();
            std::thread::spawn(move || {
                let db = Store::open(path).unwrap();
                gate.wait();
                begin(&db, &format!("call-{i}")).is_ok()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .filter_map(|w| w.join().ok())
            .filter(|allowed| *allowed)
            .count(),
        1
    );
    assert_eq!(
        db.work_budget("owner", "org", "team", "work")
            .unwrap()
            .reserved_tokens,
        100
    );
}
#[test]
fn budget_settings_are_scoped_versioned_and_retry_safe() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    assert!(db
        .set_team_budget_setting("outsider", "org", "team", "change", 0, Some(30))
        .is_err());
    let saved = db
        .set_team_budget_setting("owner", "org", "team", "change", 0, Some(30))
        .unwrap();
    assert_eq!(
        db.set_team_budget_setting("owner", "org", "team", "change", 0, Some(30))
            .unwrap(),
        saved
    );
    assert!(db
        .set_team_budget_setting("owner", "org", "team", "change", 0, Some(40))
        .is_err());
    assert!(db
        .set_team_budget_setting("owner", "org", "team", "different", 0, Some(40))
        .is_err());
    assert_eq!(
        db.work_budget("owner", "org", "team", "work")
            .unwrap()
            .token_limit,
        100
    );
}
