use std::sync::Arc;
use tetonic_domain::{RunCommand, RunSupervisorError, StorageLimits};
use tetonic_run::{DurableRunSupervisor, RunSupervisor};

mod harness;

#[tokio::test]
async fn test_quota_limits_enter_safe_mode() {
    let db_path = std::env::temp_dir().join(format!("lokai_test_{}.db", uuid::Uuid::new_v4()));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let quotas = Arc::new(tetonic_run::quotas::StorageQuotaManager::new(
        StorageLimits {
            global_quota_bytes: 64,
            ..StorageLimits::default()
        },
    ));
    let sup = DurableRunSupervisor::new(Some(store)).with_quotas(quotas);
    let mut last = None;
    for i in 0..40 {
        let res = sup
            .handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
                envelope: harness::env(None, &format!("create_{i}")),
                session_id: Some(tetonic_domain::SessionId::new("s1")),
                run_id: tetonic_domain::RunId::new(format!("r{i}")),
                root_task_id: tetonic_domain::TaskId::new("t1"),
                root_binding: Default::default(),
                speculation: None,
                job_spec: None,
            }))
            .await;
        last = Some(res);
        if last.as_ref().unwrap().is_err() {
            break;
        }
    }
    let err = last.unwrap().expect_err("quota must trip");
    assert!(matches!(err, RunSupervisorError::StorageLimitExceeded(_)));
    assert!(sup.is_safe_mode(), "quota exceed must enter safe mode");
}

#[tokio::test]
async fn test_safe_mode_blocking() {
    let (sup, _store) = harness::db_supervisor();
    sup.migration.enter_safe_mode("Test forced safe mode");

    let res = sup
        .handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
            envelope: harness::env(None, "create_safe"),
            session_id: Some(tetonic_domain::SessionId::new("s1")),
            run_id: tetonic_domain::RunId::new("r1"),
            root_task_id: tetonic_domain::TaskId::new("t1"),
            root_binding: Default::default(),
            speculation: None,
            job_spec: None,
        }))
        .await;

    assert!(matches!(res, Err(RunSupervisorError::Conflict(_))));
}

/// R04: newly appended run events carry non-empty payload digests that change with payload.
#[tokio::test]
async fn test_run_event_payload_digest_populated() {
    let (sup, store) = harness::db_supervisor();
    let run_a = tetonic_domain::RunId::new("digest_a");
    let run_b = tetonic_domain::RunId::new("digest_b");
    for (run, cid) in [(&run_a, "create_a"), (&run_b, "create_b")] {
        sup.handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
            envelope: harness::env(None, cid),
            session_id: Some(tetonic_domain::SessionId::new("s_digest")),
            run_id: run.clone(),
            root_task_id: tetonic_domain::TaskId::new("t1"),
            root_binding: Default::default(),
            speculation: None,
            job_spec: None,
        }))
        .await
        .expect("create");
    }

    let events_a = store
        .read({
            let id = run_a.0.clone();
            move |db| db.list_run_events(&id).expect("list a")
        })
        .await
        .expect("read a");
    let events_b = store
        .read({
            let id = run_b.0.clone();
            move |db| db.list_run_events(&id).expect("list b")
        })
        .await
        .expect("read b");
    assert_eq!(events_a.len(), 1);
    assert_eq!(events_b.len(), 1);
    let da = &events_a[0].payload_digest.0;
    let db = &events_b[0].payload_digest.0;
    assert!(!da.is_empty(), "payload_digest must not be empty");
    assert!(
        da.starts_with("sha256:"),
        "expected sha256 digest, got {da}"
    );
    assert_ne!(da, db, "different create payloads must digest differently");

    let resumed = sup
        .resume_from_sequence(run_a.clone(), 0, 10)
        .await
        .expect("resume ok")
        .expect("no gap");
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].payload_digest.0, *da);
}

/// R25: compact persists replay floor; resume below floor returns ReplayGap.
#[tokio::test]
async fn compact_then_resume_below_floor_returns_gap() {
    let (sup, store) = harness::db_supervisor();
    let (run, _root, _sess) = harness::create_started_run(&sup).await;
    // CreateRun=1, StartRun=2 — add a few more transitions.
    for i in 0..3 {
        let seq = harness::current_seq(&sup, &run).await;
        let task = tetonic_domain::TaskId::new(format!("extra_{i}"));
        sup.handle(RunCommand::AddTask(tetonic_domain::AddTask {
            envelope: harness::env(Some(seq), &format!("add_{i}")),
            run_id: run.clone(),
            task_id: task,
            binding: Default::default(),
        }))
        .await
        .expect("add task");
    }
    let tip_before = sup.snapshot(run.clone()).await.expect("tip before");
    assert!(tip_before.sequence >= 5);

    let floor = 3u64;
    let report = store
        .write({
            let run_id = run.0.clone();
            move |db| {
                let events = db.list_run_events(&run_id).expect("list");
                let below: Vec<_> = events.into_iter().filter(|e| e.sequence < floor).collect();
                let session = tetonic_domain::SessionId::new("sess_1");
                let recovery = tetonic_run::replay_from_events(
                    &tetonic_run::empty_snapshot(
                        tetonic_domain::RunId::new(&run_id),
                        Some(session),
                    ),
                    &below,
                )
                .expect("recovery");
                db.compact_run_events_at_floor(&run_id, floor, &recovery)
                    .expect("compact")
            }
        })
        .await
        .expect("write");
    assert_eq!(report.replay_floor, floor);
    assert!(report.deleted >= 1);

    let gap = sup
        .resume_from_sequence(run.clone(), 1, 50)
        .await
        .expect("resume call")
        .expect_err("must be ReplayGap");
    assert_eq!(gap.requested_after, 1);
    assert_eq!(gap.earliest_available, floor);
    assert!(matches!(
        gap.reason,
        tetonic_domain::ReplayGapReason::OlderThanFloor
    ));

    let tip_after = sup.snapshot(run.clone()).await.expect("tip after");
    assert_eq!(tip_after.sequence, tip_before.sequence);
    assert_eq!(tip_after.state, tip_before.state);

    // Restart supervisor on same DB — tip projection matches.
    let sup2 = DurableRunSupervisor::new(Some(store.clone()));
    let tip_restart = sup2.snapshot(run.clone()).await.expect("restart tip");
    assert_eq!(tip_restart.sequence, tip_before.sequence);
    assert_eq!(tip_restart.state, tip_before.state);

    let rebuilt = sup2.replay_run(&run).expect("replay from floor");
    assert_eq!(rebuilt.sequence, tip_before.sequence);
}

#[test]
fn test_migration_manager_backup_and_safe_mode_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("lokai.db");
    tetonic_memory::Store::open(&db).expect("create db");
    let mgr = tetonic_run::migration::MigrationManager::new();
    mgr.set_db_path(db.clone());
    let dest = mgr.pre_migration_backup().expect("backup ok");
    assert!(dest.is_file());
    assert!(std::fs::metadata(&dest).unwrap().len() > 0);
    assert!(!mgr.is_safe_mode());

    let mgr2 = tetonic_run::migration::MigrationManager::new();
    mgr2.set_db_path(dir.path().join("does-not-exist.db"));
    let err = mgr2.pre_migration_backup();
    assert!(err.is_err());
    assert!(mgr2.is_safe_mode(), "backup failure must enter safe mode");
}
