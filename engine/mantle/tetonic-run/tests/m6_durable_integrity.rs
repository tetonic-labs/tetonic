//! M6 — durable run-state integrity (INV-RUN-003).
//!
//! These cover the four fail-opens in `recover_at_startup`, the detector that
//! observes it, and the premise the durability argument rests on.
//!
//! Non-vacuity: each test was run against the pre-M6 tree and fails there. The
//! recorded failures are in `docs/architecture/v3/packages/M6.md` → IMPLEMENT.

use std::collections::BTreeMap;

use tetonic_domain::{RunId, RunSnapshot, RunState, SessionId, TaskId, TaskRecord, TaskState};
use tetonic_memory::SharedStore;
use tetonic_run::{detect_recovery_required, DurableRunSupervisor, RunSupervisor};

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lokai_m6_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Break (or repair) the journal from a second connection.
///
/// Deliberately not a test hook inside `lokai-memory`: the point is that the
/// real `commit_run_command` fails, so the fault has to be real too.
fn ddl(path: &std::path::Path, sql: &str) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    conn.execute_batch(sql).unwrap();
}

/// Make the projection write fail while every read still succeeds.
///
/// Recovery persists the projection, so this is the injection that exercises the
/// write path specifically — as opposed to dropping the table, which breaks the
/// run-list read first and would test a different fail-open.
fn block_projection_writes(path: &std::path::Path) {
    ddl(
        path,
        "CREATE TRIGGER m6_block_projection BEFORE INSERT ON run_projections
         BEGIN SELECT RAISE(ABORT, 'injected projection write failure'); END;",
    );
}

fn unblock_projection_writes(path: &std::path::Path) {
    ddl(path, "DROP TRIGGER m6_block_projection;");
}

/// A snapshot whose recovery trigger is an orphaned task: `Leased` with no
/// active attempt (`recovery.rs:28`).
///
/// The trigger matters. An expired-lease fixture would be vacuous, because
/// `recover_expired_leases` runs *before* detection and clears exactly that
/// predicate, leaving the task `Ready`/`Failed` — so the flag would never be
/// set and the test would assert nothing. This trigger, and the lease-less
/// running attempt at `recovery.rs:22`, are the two that survive the pass, and
/// neither reads the clock.
fn orphaned_task_snapshot(run: &str) -> RunSnapshot {
    let task_id = TaskId::new("task_orphan");
    let mut tasks = BTreeMap::new();
    tasks.insert(
        task_id.clone(),
        TaskRecord {
            task_id,
            state: TaskState::Leased,
            binding: Default::default(),
            accepted_artifact: None,
            active_attempt: None,
            winning_attempt: None,
            finalization_claim: None,
            completed_version: None,
            retry: Default::default(),
            side_effect_keys: Vec::new(),
        },
    );
    RunSnapshot {
        run_id: RunId::new(run),
        session_id: Some(SessionId::new("sess_m6")),
        state: RunState::Active,
        sequence: 1,
        workspace_version: None,
        tasks,
        attempts: Default::default(),
        dependencies: Default::default(),
        events: Vec::new(),
        delivery_index: Default::default(),
        side_effect_commits: Default::default(),
        deadlines: Default::default(),
        cancellation: Default::default(),
        speculation: Default::default(),
        next_lease_epoch: 0,
        job_spec: None,
    }
}

/// Persist a snapshot without going through the supervisor, so the run exists
/// on disk before any recovery pass sees it.
fn seed(store: &SharedStore, snap: &RunSnapshot) {
    let payload = serde_json::json!({"seed": true});
    let event = tetonic_domain::RunEventEnvelope {
        event_id: tetonic_domain::ids::EventId::new("ev_seed"),
        run_id: snap.run_id.clone(),
        sequence: snap.sequence,
        event_type: tetonic_domain::EventType::Other("seed".into()),
        schema_version: 1,
        command_id: None,
        causation_id: None,
        correlation_id: None,
        actor: tetonic_domain::EventActor {
            name: "test".into(),
        },
        occurred_at: chrono::Utc::now(),
        recorded_at: chrono::Utc::now(),
        data_class: Default::default(),
        payload_digest: tetonic_run::payload_digest::digest_event_payload(&payload).unwrap(),
        payload,
    };
    store
        .write_sync({
            let snap = snap.clone();
            move |db| db.commit_run_command(&snap, &event, None)
        })
        .unwrap()
        .unwrap();
}

/// VM-1 — a recovery write that fails must fail closed.
///
/// Pre-M6 the write at `service.rs:226` was `let _ = …`, so this started
/// normally and reported nothing to recover.
#[test]
fn recovery_write_failure_enters_safe_mode() {
    let path = temp_db();
    let store = SharedStore::open(&path, 1).unwrap();
    let snap = orphaned_task_snapshot("run_write_fails");
    seed(&store, &snap);

    block_projection_writes(&path);

    let sup = DurableRunSupervisor::new(Some(store.clone()));
    assert!(
        sup.is_safe_mode(),
        "a dropped recovery write must fail closed into Safe Mode"
    );
}

/// VM-2 — a run-list read that fails must fail closed.
///
/// Pre-M6 this was `.unwrap_or_else(|_| Ok(vec![])).unwrap_or_default()`, which
/// turned "cannot read the run list" into "there are no runs".
#[test]
fn recovery_read_failure_enters_safe_mode() {
    let path = temp_db();
    let store = SharedStore::open(&path, 1).unwrap();
    let snap = orphaned_task_snapshot("run_read_fails");
    seed(&store, &snap);

    ddl(&path, "DROP TABLE run_projections");

    let sup = DurableRunSupervisor::new(Some(store.clone()));
    assert!(
        sup.is_safe_mode(),
        "an unreadable run list must fail closed, not report nothing to recover"
    );
}

/// VM-3 — the detector must not answer "no" when it means "cannot tell".
///
/// This is the link that made the dropped write self-concealing: production
/// consults `recovery_required()` to decide whether to enter Safe Mode, and
/// pre-M6 a store it could not read returned `false`.
#[test]
fn recovery_required_fails_closed_when_the_store_cannot_be_read() {
    let path = temp_db();
    let store = SharedStore::open(&path, 1).unwrap();
    let snap = orphaned_task_snapshot("run_detector");
    seed(&store, &snap);

    let sup = DurableRunSupervisor::new(Some(store.clone()));
    ddl(&path, "DROP TABLE run_projections");

    assert!(
        sup.recovery_required(),
        "an unreadable store must be reported as needing recovery"
    );
    let reason = sup
        .recovery_required_reason()
        .expect("fail-closed detector must name why");
    assert!(
        reason.contains("not self-clearing"),
        "Safe Mode reason must tell the operator the state will not resolve itself, got {reason}"
    );
}

/// VM-11 — the premise D-1 rests on, and the reason M6 needs no durable flag.
///
/// `RecoveryRequired` is a memo of conditions held in the snapshot. Because
/// `commit_run_command` is one transaction, a failed recovery write leaves those
/// conditions untouched on disk, so the next start derives the same answer. This
/// asserts the whole loop: the write fails, nothing persists, and a later start
/// against a working store still reaches `RecoveryRequired`.
///
/// It exists because D-1 replaced a mechanism with an argument, and an argument
/// with no test is a comment.
#[test]
fn recovery_state_is_rederived_after_a_failed_persist() {
    let path = temp_db();
    let store = SharedStore::open(&path, 1).unwrap();
    let run = "run_rederive";
    let snap = orphaned_task_snapshot(run);
    seed(&store, &snap);

    // The condition is detectable before anything is recovered.
    assert!(
        detect_recovery_required(&snap, 0),
        "fixture must trigger detection, or the rest of this test is vacuous"
    );

    // First start: the write cannot land.
    block_projection_writes(&path);
    let first = DurableRunSupervisor::new(Some(store.clone()));
    assert!(first.is_safe_mode(), "the failed persist must fail closed");

    // Nothing was persisted, so the trigger is still on disk.
    let reloaded = store
        .read_sync(move |db| db.load_run_snapshot(run))
        .unwrap()
        .unwrap()
        .expect("snapshot must still exist");
    assert_ne!(
        reloaded.state,
        RunState::RecoveryRequired,
        "the failed write must not have persisted the flag"
    );
    assert!(
        detect_recovery_required(&reloaded, 0),
        "the triggering condition must survive the failed write — this is what \
         makes RecoveryRequired durable without a durable flag"
    );

    // Second start against a working store: the flag is re-derived and persisted.
    unblock_projection_writes(&path);
    let second = DurableRunSupervisor::new(Some(store.clone()));
    assert!(
        second.recovery_required(),
        "recovery must be re-derived and persisted on the next start"
    );
    let reason = second
        .recovery_required_reason()
        .expect("persisted RecoveryRequired must name the run");
    assert!(
        reason.contains(run) && reason.contains("not self-clearing"),
        "Safe Mode reason must name run {run} and say it is not self-clearing, got {reason}"
    );
    let persisted = store
        .read_sync(move |db| db.load_run_snapshot(run))
        .unwrap()
        .unwrap()
        .expect("snapshot must exist");
    assert_eq!(
        persisted.state,
        RunState::RecoveryRequired,
        "the re-derived flag must be durable, so a later start sees it directly"
    );
}
