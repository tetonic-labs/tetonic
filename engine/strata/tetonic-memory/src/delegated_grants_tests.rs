use super::*;
use crate::{ContextOwner, OrganizationRole, TeamRow};
use tetonic_domain::{IdentityId, RunSnapshot, TaskInputBinding};

struct Fixture {
    parent: ExecutionGrant,
    request: DelegatedGrantRequest,
}

#[test]
fn approved_child_environment_does_not_expand_coordinator_and_remains_revocable() {
    let db = Store::open(":memory:").unwrap();
    let mut f = seed(&db, true);
    f.request.job.capability_bindings.push("read_file".into());
    assert!(db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .is_err());
    f.request.approved_environment = Some("a".repeat(64));
    let child = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    assert!(allows(&db, &child, 102));
    assert_eq!(
        child.lineage.approved_environment,
        f.request.approved_environment
    );
    assert!(!db
        .get_execution_grant("alice", "org", "parent-grant")
        .unwrap()
        .unwrap()
        .job
        .capability_bindings
        .contains(&"read_file".into()));
    f.request.approved_environment = Some("b".repeat(64));
    assert!(db
        .derive_execution_grant("alice", "org", "team", &f.request, 102)
        .is_err());
    db.revoke_execution_grant("alice", "org", "parent-grant", 103)
        .unwrap();
    assert!(!allows(&db, &child, 104));
}

fn seed(db: &Store, shared: bool) -> Fixture {
    db.bootstrap_control("alice", "org", "Org").unwrap();
    db.register_control_principal("bob").unwrap();
    db.set_organization_member("org", "bob", OrganizationRole::Administrator)
        .unwrap();
    db.create_team(&TeamRow {
        org_id: "org".into(),
        team_id: "team".into(),
        name: "Team".into(),
        owner_principal_id: "alice".into(),
    })
    .unwrap();
    db.add_team_member("org", "team", "bob").unwrap();
    db.create_information_context(
        "alice",
        "context",
        &if shared {
            ContextOwner::Team {
                org_id: "org".into(),
                team_id: "team".into(),
            }
        } else {
            ContextOwner::Private {
                org_id: "org".into(),
            }
        },
    )
    .unwrap();
    let jobs: Vec<_> = ["lead", "worker"]
        .iter()
        .map(|key| {
            let agent = db
                .register_organization_agent(
                    "alice",
                    "org",
                    key,
                    "general",
                    &serde_json::json!({"instructions":key}),
                )
                .unwrap();
            AgentJobSpec {
                identity_id: IdentityId::new(agent.identity.identity_id),
                definition_digest: agent.identity.bound_definition_digest,
                input_digest: format!("input-{key}"),
                capability_bindings: vec!["finish".into(), "recall".into()],
                artifact_bindings: vec!["team-evidence".into()],
                recovery_id: format!("job-{key}"),
            }
        })
        .collect();
    let parent = ExecutionGrant {
        grant_id: "parent-grant".into(),
        scope: ExecutionScope {
            principal_id: "alice".into(),
            organization_id: "org".into(),
            information_context_id: "context".into(),
        },
        job: jobs[0].clone(),
        expires_at: 1000,
    };
    db.issue_execution_grant("alice", &parent, 100).unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "alice",
        org: "org",
        team: "team",
        work_id: "root",
        title: "Lead",
        request_id: "root-request",
        goal_id: None,
    })
    .unwrap();
    db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .unwrap();
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "alice",
        org: "org",
        team: "team",
        delegation_id: "delegation",
        parent_work_id: "root",
        child_work_id: "child",
        child_title: "Help",
        request_id: "delegate",
        parent_budget_tokens: 100,
        child_budget_tokens: 40,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
    .unwrap();
    // Durable managed records, deliberately constructed as storage fixtures.
    let binding = TaskInputBinding {
        execution_scope: Some(parent.scope.clone()),
        execution_grant_id: Some(parent.grant_id.clone()),
        job_spec: Some(parent.job.clone()),
        deadline: Some(940),
        ..Default::default()
    };
    let run: RunSnapshot = serde_json::from_value(serde_json::json!({
        "run_id":"run", "state":"active","sequence":1,"workspace_version":null,
        "tasks":{"parent-task":{"task_id":"parent-task","state":"running","binding":binding,
            "accepted_artifact":null,"active_attempt":"parent-attempt"}},
        "attempts":{"parent-attempt":{"attempt_id":"parent-attempt","task_id":"parent-task","state":"running",
            "execution_claimed":true,"execution_quiesced":false,"task_version":1,"workspace_version":null,
            "input_digest":"input-lead","result_digest":null,"delivery_key":null,
            "lease":{"lease_id":"lease","attempt_id":"parent-attempt","lease_epoch":1,"holder":"local",
                "issued_at":100,"expires_at":950,"heartbeat_interval_secs":30,"last_heartbeat_sequence":0},
            "failure_class":null,"failure_reason":null}},
        "dependencies":{},"events":[],"job_spec":parent.job
    })).unwrap();
    db.persist_run_projection(&run).unwrap();
    db.activate_team_work_item("alice", "org", "team", "root", "parent-attempt", "run")
        .unwrap();
    let mut job = jobs[1].clone();
    job.capability_bindings = vec!["finish".into()];
    job.artifact_bindings.clear();
    Fixture {
        parent,
        request: DelegatedGrantRequest {
            approved_environment: None,
            request_id: "derive-request".into(),
            grant_id: "child-grant".into(),
            parent_grant_id: "parent-grant".into(),
            delegation_id: "delegation".into(),
            job,
            expires_at: 900,
        },
    }
}

fn allows(db: &Store, child: &DelegatedExecutionGrant, at: i64) -> bool {
    db.delegated_execution_grant_allows(
        &child.grant.grant_id,
        &child.grant.scope,
        &child.grant.job,
        "run",
        "parent-attempt",
        at,
    )
    .unwrap()
}

#[test]
fn child_permission_is_exact_parent_bound_idempotent_and_does_not_allocate_again() {
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, true);
    let child = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    assert_eq!(
        child,
        db.derive_execution_grant("alice", "org", "team", &f.request, 102)
            .unwrap()
    );
    assert_eq!(child.grant.scope, f.parent.scope);
    assert_eq!(child.lineage.parent_attempt_id.0, "parent-attempt");
    assert_eq!(child.lineage.payer_principal_id, "alice");
    assert_eq!(child.lineage.stop_scope, "work/root");
    assert_eq!(child.lineage.allocated_tokens, 40);
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
    assert!(allows(&db, &child, 102));
    assert!(!db
        .execution_grant_allows("child-grant", &child.grant.scope, &child.grant.job, 102)
        .unwrap());
    assert!(db
        .issue_execution_grant("alice", &child.grant, 102)
        .is_err());
    assert!(!db
        .delegated_execution_grant_allows(
            "child-grant",
            &child.grant.scope,
            &child.grant.job,
            "other-run",
            "parent-attempt",
            102
        )
        .unwrap());
    let mut wrong_job = child.grant.job.clone();
    wrong_job.input_digest = "another input".into();
    assert!(!db
        .delegated_execution_grant_allows(
            "child-grant",
            &child.grant.scope,
            &wrong_job,
            "run",
            "parent-attempt",
            102
        )
        .unwrap());
    assert_eq!(
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM execution_grant_events WHERE action='derive'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert!(db
        .conn
        .execute("DELETE FROM execution_grant_lineage", [])
        .is_err());
    assert!(db
        .conn
        .execute(
            "UPDATE execution_grant_lineage SET parent_grant_id='child-grant'",
            []
        )
        .is_err());
}

#[test]
fn derivation_denies_broader_tools_artifacts_expiry_and_changed_retry_payloads() {
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, true);
    for kind in 0..5 {
        let mut request = f.request.clone();
        match kind {
            0 => request.job.capability_bindings.push("run_shell".into()),
            1 => request
                .job
                .artifact_bindings
                .push("private-evidence".into()),
            2 => request.expires_at = 1001,
            3 => request.job.identity_id = IdentityId::new("unknown-agent"),
            _ => request.grant_id = f.parent.grant_id.clone(),
        }
        assert!(db
            .derive_execution_grant("alice", "org", "team", &request, 101)
            .is_err());
    }
    let first = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    for kind in 0..4 {
        let mut request = f.request.clone();
        match kind {
            0 => request.job.input_digest = "changed".into(),
            1 => request.expires_at = 800,
            2 => request.grant_id = "another-grant".into(),
            _ => request.request_id = "another-request".into(),
        }
        assert!(db
            .derive_execution_grant("alice", "org", "team", &request, 101)
            .is_err());
    }
    assert!(allows(&db, &first, 102));
}

#[test]
fn shared_team_access_cannot_be_inferred_from_private_context_or_another_manager() {
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, false);
    let error = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap_err();
    assert!(matches!(error, StoreError::ControlAccessDenied));
    assert!(db
        .get_execution_grant("alice", "org", "child-grant")
        .unwrap()
        .is_none());
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, true);
    for (actor, org, team) in [
        ("bob", "org", "team"),
        ("outsider", "org", "team"),
        ("alice", "other", "team"),
        ("alice", "org", "other"),
    ] {
        assert!(matches!(
            db.derive_execution_grant(actor, org, team, &f.request, 101),
            Err(StoreError::ControlAccessDenied)
        ));
    }
}

#[test]
fn revocation_expiry_and_parent_lifecycle_immediately_remove_child_permission() {
    for change in 0..8 {
        let db = Store::open(":memory:").unwrap();
        let f = seed(&db, true);
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        assert!(allows(&db, &child, 102));
        match change {
            0 => db
                .revoke_execution_grant("alice", "org", "parent-grant", 103)
                .unwrap(),
            1 => db
                .revoke_execution_grant("alice", "org", "child-grant", 103)
                .unwrap(),
            2 => {
                db.request_control_stop("alice", "org", "work", "root", "pause", "Stop")
                    .unwrap();
            }
            3 => {
                db.park_team_work_item("alice", "org", "team", "child")
                    .unwrap();
            }
            4 => {
                db.conn
                    .execute(
                        "UPDATE control_principals SET enabled=0 WHERE principal_id='alice'",
                        [],
                    )
                    .unwrap();
            }
            _ => {
                let mut run = db.load_run_snapshot("run").unwrap().unwrap();
                match change {
                    5 => run.state = RunState::RecoveryRequired,
                    6 => {
                        run.attempts
                            .get_mut(&AttemptId::new("parent-attempt"))
                            .unwrap()
                            .lease
                            .as_mut()
                            .unwrap()
                            .expires_at = 103
                    }
                    _ => {
                        run.attempts
                            .get_mut(&AttemptId::new("parent-attempt"))
                            .unwrap()
                            .execution_quiesced = true
                    }
                }
                db.persist_run_projection(&run).unwrap();
            }
        }
        assert!(!allows(&db, &child, 104), "change {change}");
        assert!(db
            .derive_execution_grant("alice", "org", "team", &f.request, 104)
            .is_err());
    }
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, true);
    let child = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    assert!(!allows(&db, &child, 900));
}

#[test]
fn only_a_claimed_matching_parent_attempt_can_authorize_delegation() {
    for change in 0..5 {
        let db = Store::open(":memory:").unwrap();
        let f = seed(&db, true);
        let mut run = db.load_run_snapshot("run").unwrap().unwrap();
        match change {
            0 => {
                run.attempts
                    .get_mut(&AttemptId::new("parent-attempt"))
                    .unwrap()
                    .execution_claimed = false
            }
            1 => {
                run.tasks
                    .get_mut(&TaskId::new("parent-task"))
                    .unwrap()
                    .binding
                    .execution_grant_id = Some("unrelated".into())
            }
            2 => {
                run.tasks
                    .get_mut(&TaskId::new("parent-task"))
                    .unwrap()
                    .binding
                    .job_spec
                    .as_mut()
                    .unwrap()
                    .input_digest = "different".into()
            }
            3 => {
                run.tasks
                    .get_mut(&TaskId::new("parent-task"))
                    .unwrap()
                    .active_attempt = None
            }
            _ => {
                run.tasks
                    .get_mut(&TaskId::new("parent-task"))
                    .unwrap()
                    .binding
                    .deadline = Some(101)
            }
        }
        db.persist_run_projection(&run).unwrap();
        assert!(db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .is_err());
    }
}

#[test]
fn grant_and_lineage_roll_back_together_when_audit_or_lineage_insert_fails() {
    for table in ["execution_grant_events", "execution_grant_lineage"] {
        let db = Store::open(":memory:").unwrap();
        let f = seed(&db, true);
        db.conn.execute_batch(&format!("CREATE TRIGGER injected BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'injected'); END;")).unwrap();
        assert!(db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .is_err());
        assert!(db
            .get_execution_grant("alice", "org", "child-grant")
            .unwrap()
            .is_none());
        assert!(!db.execution_grant_is_delegated("child-grant").unwrap());
        assert_eq!(
            db.conn
                .query_row(
                    "SELECT COUNT(*) FROM execution_grant_events WHERE action='derive'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn exact_receipt_survives_reopen_but_recovery_does_not_resume_permission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grants.db");
    let (f, first) = {
        let db = Store::open(&path).unwrap();
        let f = seed(&db, true);
        let first = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        (f, first)
    };
    let db = Store::open(&path).unwrap();
    assert_eq!(
        first,
        db.derive_execution_grant("alice", "org", "team", &f.request, 102)
            .unwrap()
    );
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    run.state = RunState::RecoveryRequired;
    db.persist_run_projection(&run).unwrap();
    assert!(!allows(&db, &first, 103));
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
}

#[test]
fn concurrent_derivation_deliveries_have_one_grant_and_audit_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grants.db");
    let db = Store::open(&path).unwrap();
    let f = seed(&db, true);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let request = f.request.clone();
            std::thread::spawn(move || {
                let db = Store::open(path).unwrap();
                barrier.wait();
                db.derive_execution_grant("alice", "org", "team", &request, 101)
                    .unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(receipts[0], receipts[1]);
    assert_eq!(
        db.conn
            .query_row("SELECT COUNT(*) FROM execution_grant_lineage", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM execution_grant_events WHERE action='derive'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn changing_parent_lease_or_task_version_fences_old_child_permission() {
    for change in 0..3 {
        let db = Store::open(":memory:").unwrap();
        let f = seed(&db, true);
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        let mut run = db.load_run_snapshot("run").unwrap().unwrap();
        if change == 2 {
            run.tasks
                .get_mut(&TaskId::new("parent-task"))
                .unwrap()
                .binding
                .task_definition_version += 1;
        } else {
            let lease = run
                .attempts
                .get_mut(&AttemptId::new("parent-attempt"))
                .unwrap()
                .lease
                .as_mut()
                .unwrap();
            if change == 0 {
                lease.lease_epoch += 1;
            } else {
                lease.holder = tetonic_domain::ExecutionTargetId::worker("replacement");
            }
        }
        db.persist_run_projection(&run).unwrap();
        assert!(!allows(&db, &child, 102));
        assert!(db
            .derive_execution_grant("alice", "org", "team", &f.request, 102)
            .is_err());
    }
}

#[test]
fn ancestor_revocation_traverses_multiple_delegations_without_resetting_payer_or_budget() {
    let db = Store::open(":memory:").unwrap();
    let f = seed(&db, true);
    let child = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "alice",
        org: "org",
        team: "team",
        delegation_id: "grand-delegation",
        parent_work_id: "child",
        child_work_id: "grandchild",
        child_title: "Contribute",
        request_id: "grand-request",
        parent_budget_tokens: 40,
        child_budget_tokens: 20,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
    .unwrap();
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    let mut task = run.tasks[&TaskId::new("parent-task")].clone();
    task.task_id = TaskId::new("child-task");
    task.active_attempt = Some(AttemptId::new("child-attempt"));
    task.binding.execution_grant_id = Some(child.grant.grant_id.clone());
    task.binding.job_spec = Some(child.grant.job.clone());
    let mut attempt = run.attempts[&AttemptId::new("parent-attempt")].clone();
    attempt.attempt_id = AttemptId::new("child-attempt");
    attempt.task_id = task.task_id.clone();
    attempt.lease.as_mut().unwrap().attempt_id = attempt.attempt_id.clone();
    run.tasks.insert(task.task_id.clone(), task);
    run.attempts.insert(attempt.attempt_id.clone(), attempt);
    db.persist_run_projection(&run).unwrap();
    db.activate_team_work_item("alice", "org", "team", "child", "child-attempt", "run")
        .unwrap();
    let mut request = f.request.clone();
    request.request_id = "grand-grant-request".into();
    request.grant_id = "grand-grant".into();
    request.parent_grant_id = "child-grant".into();
    request.delegation_id = "grand-delegation".into();
    request.expires_at = 850;
    request.job.input_digest = "grand-input".into();
    let grand = db
        .derive_execution_grant("alice", "org", "team", &request, 102)
        .unwrap();
    let grand_allowed = |at| {
        db.delegated_execution_grant_allows(
            "grand-grant",
            &grand.grant.scope,
            &grand.grant.job,
            "run",
            "child-attempt",
            at,
        )
        .unwrap()
    };
    assert!(grand_allowed(103));
    assert_eq!(grand.lineage.payer_principal_id, "alice");
    assert_eq!(grand.lineage.stop_scope, "work/root");
    assert_eq!(grand.lineage.allocated_tokens, 20);
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
    db.revoke_execution_grant("alice", "org", "parent-grant", 104)
        .unwrap();
    assert!(!grand_allowed(105));
    assert!(!allows(&db, &child, 105));
}

#[test]
fn upgrading_version_55_preserves_roots_without_inventing_child_permission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-grants.db");
    let fixture = {
        let db = Store::open(&path).unwrap();
        let fixture = seed(&db, true);
        db.remove_agent_edits_schema_for_test();
        db.conn.execute_batch("DROP TABLE execution_grant_lineage; DELETE FROM schema_versions WHERE version>=56;").unwrap();
        fixture
    };
    let db = Store::open(&path).unwrap();
    assert_eq!(
        crate::backup::schema_version(&db.conn).unwrap(),
        crate::SCHEMA_TARGET_VERSION
    );
    assert_eq!(
        db.conn
            .query_row("SELECT COUNT(*) FROM execution_grant_lineage", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert!(db
        .execution_grant_allows(
            "parent-grant",
            &fixture.parent.scope,
            &fixture.parent.job,
            101
        )
        .unwrap());
    assert!(!db.execution_grant_is_delegated("parent-grant").unwrap());
    assert!(!db
        .execution_grant_allows(
            "child-grant",
            &fixture.parent.scope,
            &fixture.request.job,
            101
        )
        .unwrap());
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
    let child = db
        .derive_execution_grant("alice", "org", "team", &fixture.request, 102)
        .unwrap();
    assert!(allows(&db, &child, 103));
    assert!(crate::pre_migrate_backup_directory(&path).is_dir());
}
