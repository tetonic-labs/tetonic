use super::*;
use tetonic_domain::{ArtifactRef, AttemptState, AttemptSuspension, TaskState};

fn park(db: &Store) -> RunSnapshot {
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    let attempt = run
        .attempts
        .get_mut(&AttemptId::new("parent-attempt"))
        .unwrap();
    attempt.state = AttemptState::Suspended;
    attempt.execution_quiesced = true;
    attempt.suspension = Some(AttemptSuspension {
        checkpoint: ArtifactRef {
            artifact_id: "protected-wait".into(),
            digest: "a".repeat(64),
        },
        reason: tetonic_domain::SuspensionReason::HumanInput,
        suspended_at: 200,
        remaining_seconds: 740,
    });
    run.tasks
        .get_mut(&TaskId::new("parent-task"))
        .unwrap()
        .state = TaskState::Parked;
    db.persist_run_projection(&run).unwrap();
    run
}

#[test]
fn work_permission_survives_wait_reopen_and_new_epoch_without_renewing_grant_or_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("durable-grants.db");
    let (f, child) = {
        let db = Store::open(&path).unwrap();
        let mut f = seed(&db, true);
        f.request.lifetime = DelegationLifetime::ParentWork;
        f.request.expires_at = 999;
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        park(&db);
        // The saved deadline (940) and worker lease (950) have passed. The
        // intentional wait is valid, but permission still expires at 999.
        assert!(allows(&db, &child, 960));
        assert!(!allows(&db, &child, 999));
        let mut additional = f.request.clone();
        additional.request_id = "new-request".into();
        additional.grant_id = "new-grant".into();
        assert!(matches!(
            db.derive_execution_grant("alice", "org", "team", &additional, 960),
            Err(StoreError::ControlAccessDenied)
        ));
        (f, child)
    };
    let db = Store::open(&path).unwrap();
    assert!(allows(&db, &child, 960));
    assert_eq!(
        db.derive_execution_grant("alice", "org", "team", &f.request, 960)
            .unwrap(),
        child
    );
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    let task = run.tasks.get_mut(&TaskId::new("parent-task")).unwrap();
    task.state = TaskState::Leased;
    task.binding.deadline = Some(1700);
    let attempt = run
        .attempts
        .get_mut(&AttemptId::new("parent-attempt"))
        .unwrap();
    attempt.state = AttemptState::Starting;
    attempt.execution_claimed = false;
    attempt.execution_quiesced = false;
    attempt.suspension = None;
    let lease = attempt.lease.as_mut().unwrap();
    lease.lease_epoch += 1;
    lease.holder = tetonic_domain::ExecutionTargetId::worker("replacement");
    lease.expires_at = 1260;
    db.persist_run_projection(&run).unwrap();
    assert!(
        allows(&db, &child, 961),
        "claiming the new executor must not revoke an existing child"
    );
    run.tasks
        .get_mut(&TaskId::new("parent-task"))
        .unwrap()
        .state = TaskState::Running;
    let attempt = run
        .attempts
        .get_mut(&AttemptId::new("parent-attempt"))
        .unwrap();
    attempt.state = AttemptState::Running;
    attempt.execution_claimed = true;
    db.persist_run_projection(&run).unwrap();
    assert!(allows(&db, &child, 962));
    assert!(
        !allows(&db, &child, 999),
        "resumption never extends authorization"
    );
    assert_eq!(
        child,
        db.derive_execution_grant("alice", "org", "team", &f.request, 962)
            .unwrap()
    );
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
    let derived: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM execution_grant_events WHERE action='derive'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(derived, 1);
}

#[test]
fn durable_permission_does_not_survive_stop_revoke_recovery_or_changed_parent_binding() {
    for change in 0..13 {
        let db = Store::open(":memory:").unwrap();
        let mut f = seed(&db, true);
        f.request.lifetime = DelegationLifetime::ParentWork;
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        let mut run = park(&db);
        assert!(allows(&db, &child, 201));
        match change {
            0 => {
                db.request_control_stop("alice", "org", "work", "root", "estop", "Stop")
                    .unwrap();
            }
            1 => {
                db.revoke_execution_grant("alice", "org", "parent-grant", 202)
                    .unwrap();
            }
            2 => {
                db.revoke_execution_grant("alice", "org", "child-grant", 202)
                    .unwrap();
            }
            3 => {
                db.conn
                    .execute(
                        "UPDATE control_principals SET enabled=0 WHERE principal_id='alice'",
                        [],
                    )
                    .unwrap();
            }
            _ => {
                let task = run.tasks.get_mut(&TaskId::new("parent-task")).unwrap();
                let attempt = run
                    .attempts
                    .get_mut(&AttemptId::new("parent-attempt"))
                    .unwrap();
                match change {
                    4 => run.state = RunState::RecoveryRequired,
                    5 => run.cancellation.run_canceled = true,
                    6 => task.binding.task_definition_version += 1,
                    7 => task.active_attempt = Some(AttemptId::new("new-attempt")),
                    8 => attempt.suspension = None,
                    9 => attempt.execution_quiesced = false,
                    10 => {
                        attempt.lease.as_mut().unwrap().holder =
                            tetonic_domain::ExecutionTargetId::worker("unfenced-replacement")
                    }
                    11 => {
                        task.binding.job_spec.as_mut().unwrap().definition_digest =
                            "other-revision".into()
                    }
                    _ => attempt.suspension.as_mut().unwrap().remaining_seconds += 1,
                }
                db.persist_run_projection(&run).unwrap();
            }
        }
        assert!(!allows(&db, &child, 203), "change {change}");
    }
}

#[test]
fn legacy_receipts_keep_exact_lease_semantics_and_lifetime_cannot_be_upgraded_in_place() {
    let db = Store::open(":memory:").unwrap();
    let mut f = seed(&db, true);
    let child = db
        .derive_execution_grant("alice", "org", "team", &f.request, 101)
        .unwrap();
    let request_json = serde_json::to_value(&f.request).unwrap();
    assert!(
        request_json.get("lifetime").is_none(),
        "old retry fingerprints remain stable"
    );
    assert_eq!(
        serde_json::from_value::<DelegatedGrantRequest>(request_json)
            .unwrap()
            .lifetime,
        DelegationLifetime::ParentLease
    );
    f.request.lifetime = DelegationLifetime::ParentWork;
    assert!(matches!(
        db.derive_execution_grant("alice", "org", "team", &f.request, 102),
        Err(StoreError::ControlResourceConflict)
    ));
    park(&db);
    assert!(!allows(&db, &child, 201));
    let mut bad = serde_json::to_value(&f.request).unwrap();
    bad["lifetime"] = "unrestricted".into();
    assert!(serde_json::from_value::<DelegatedGrantRequest>(bad).is_err());
}

#[test]
fn schema_64_upgrade_preserves_legacy_permission_and_retry_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema-64.db");
    let (f, child) = {
        let db = Store::open(&path).unwrap();
        let f = seed(&db, true);
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        db.conn
            .execute("DELETE FROM schema_versions WHERE version=65", [])
            .unwrap();
        (f, child)
    };
    let db = Store::open(&path).unwrap();
    assert_eq!(
        crate::backup::schema_version(&db.conn).unwrap(),
        crate::SCHEMA_TARGET_VERSION
    );
    assert!(crate::pre_migrate_backup_directory(&path).is_dir());
    assert_eq!(
        child,
        db.derive_execution_grant("alice", "org", "team", &f.request, 102)
            .unwrap()
    );
    assert!(allows(&db, &child, 102));
    park(&db);
    assert!(
        !allows(&db, &child, 201),
        "migration never broadens an existing permission"
    );
}

#[test]
fn clearing_a_stop_cannot_revive_old_work_permission_and_unrelated_stops_do_not_interfere() {
    for agent in ["lead", "worker"] {
        let db = Store::open(":memory:").unwrap();
        let mut f = seed(&db, true);
        f.request.lifetime = DelegationLifetime::ParentWork;
        let child = db
            .derive_execution_grant("alice", "org", "team", &f.request, 101)
            .unwrap();
        db.request_control_stop(
            "alice",
            "org",
            "agent",
            "unrelated-agent",
            "pause",
            "Other work",
        )
        .unwrap();
        assert!(allows(&db, &child, 102));
        db.request_control_stop("alice", "org", "agent", agent, "cancel", "Stop")
            .unwrap();
        assert!(!allows(&db, &child, 103));
        db.clear_control_stop("alice", "org", "agent", agent)
            .unwrap();
        // Agent stops do not rewrite the parent work binding; the saved stop
        // generation, rather than a changed task state, must reject this grant.
        assert_eq!(
            db.get_team_work_item("org", "team", "root")
                .unwrap()
                .unwrap()
                .status,
            "running"
        );
        assert!(!allows(&db, &child, 104));
        assert!(db
            .derive_execution_grant("alice", "org", "team", &f.request, 104)
            .is_err());
    }
}
