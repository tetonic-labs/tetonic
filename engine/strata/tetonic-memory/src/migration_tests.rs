//! Crash/rollback fixtures use only temporary databases and self-spawned test processes.
use super::*;
use rusqlite::functions::FunctionFlags;

fn raw_store(path: &Path) -> Store {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )
    .unwrap();
    Store {
        conn,
        path: path.to_path_buf(),
    }
}

fn seed_unversioned(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_versions(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
        CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('keep');",
    )
    .unwrap();
}

fn install_fault(conn: &Connection, version: i64, phase: &str, crash: bool) {
    assert!(matches!(phase, "BEFORE" | "AFTER"));
    if crash {
        conn.create_scalar_function(
            "migration_crash",
            0,
            FunctionFlags::SQLITE_UTF8,
            |_| -> rusqlite::Result<i64> {
                std::process::exit(71);
            },
        )
        .unwrap();
    }
    let fault = if crash {
        "SELECT migration_crash();"
    } else {
        "SELECT RAISE(ABORT, 'migration fault');"
    };
    conn.execute_batch(&format!(
        "CREATE TEMP TRIGGER fail_migration {phase} INSERT ON schema_versions
        WHEN NEW.version = {version} BEGIN {fault} END;"
    ))
    .unwrap();
}

fn assert_old_empty_schema(conn: &Connection) {
    assert_eq!(backup::schema_version(conn).unwrap(), 0);
    let tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 2, "a failed migration left schema changes behind");
    let value: String = conn
        .query_row("SELECT value FROM sentinel", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "keep");
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn sql_errors_roll_back_every_migration_marker() {
    for version in 1..=SCHEMA_TARGET_VERSION {
        for phase in ["BEFORE", "AFTER"] {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("lokai.db");
            seed_unversioned(&db);
            let store = raw_store(&db);
            install_fault(&store.conn, version, phase, false);
            assert!(store.migrate().is_err(), "{version} {phase}");
            assert!(store.conn.is_autocommit());
            assert_old_empty_schema(&store.conn);
        }
    }
}

fn crash_upgrade(path: &Path, version: i64, phase: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "migration_tests::migration_child_process",
            "--nocapture",
        ])
        .env("LOKAI_MIGRATION_CRASH_DB", path)
        .env("LOKAI_MIGRATION_CRASH_VERSION", version.to_string())
        .env("LOKAI_MIGRATION_CRASH_PHASE", phase)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(71),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn migration_child_process() {
    let Some(path) = std::env::var_os("LOKAI_MIGRATION_CRASH_DB") else {
        return;
    };
    let version = std::env::var("LOKAI_MIGRATION_CRASH_VERSION")
        .unwrap()
        .parse()
        .unwrap();
    let phase = std::env::var("LOKAI_MIGRATION_CRASH_PHASE").unwrap();
    let store = raw_store(Path::new(&path));
    install_fault(&store.conn, version, &phase, true);
    store.migrate().unwrap();
    panic!("crash hook was not executed");
}

#[test]
fn abrupt_exit_at_every_marker_is_recoverable() {
    for version in 1..=SCHEMA_TARGET_VERSION {
        for phase in ["BEFORE", "AFTER"] {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("lokai.db");
            seed_unversioned(&db);
            crash_upgrade(&db, version, phase);
            let conn = Connection::open(&db).unwrap();
            assert_old_empty_schema(&conn);
            drop(conn);
            let upgraded = Store::open(&db).unwrap();
            assert_eq!(
                backup::schema_version(&upgraded.conn).unwrap(),
                SCHEMA_TARGET_VERSION
            );
        }
    }
}

fn seed_v26(path: &Path) {
    let store = Store::open(path).unwrap();
    store.remove_context_schema_for_test();
    store.conn.execute_batch("DROP TABLE agent_identity_revisions; DROP TABLE control_admin_events; DROP TABLE agent_identities;
        DELETE FROM schema_versions WHERE version > 26;
        DROP TABLE run_projections;
        CREATE TABLE run_projections (
            run_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, sequence INTEGER NOT NULL,
            state TEXT NOT NULL, projection_json TEXT NOT NULL, updated_at TEXT NOT NULL,
            replay_floor INTEGER NOT NULL DEFAULT 0, recovery_snapshot_json TEXT
        );
        INSERT INTO run_projections VALUES ('run-original','session-original',42,'Active','{}','now',7,'{}');").unwrap();
}

fn assert_projection(conn: &Connection) {
    let values: (String, i64, i64) = conn.query_row(
        "SELECT session_id,sequence,replay_floor FROM run_projections WHERE run_id='run-original'",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).unwrap();
    assert_eq!(values, ("session-original".into(), 42, 7));
}

fn snapshots(db: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(pre_migrate_backup_directory(db))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sqlite"))
        .collect()
}

#[test]
fn v26_rebuild_crash_retry_and_restore_preserve_rows_and_backups() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("lokai.db");
    seed_v26(&db);
    // v27 has dropped/rebuilt the original table, but has not committed.
    crash_upgrade(&db, 27, "AFTER");
    let conn = Connection::open(&db).unwrap();
    assert_eq!(backup::schema_version(&conn).unwrap(), 26);
    assert_projection(&conn);
    drop(conn);
    let first = snapshots(&db).pop().unwrap();
    let bytes_before = std::fs::read(&first).unwrap();
    let store = Store::open(&db).unwrap();
    assert_projection(&store.conn);
    assert_eq!(
        backup::schema_version(&store.conn).unwrap(),
        SCHEMA_TARGET_VERSION
    );
    assert_eq!(snapshots(&db).len(), 2);
    assert_eq!(std::fs::read(&first).unwrap(), bytes_before);
    drop(store);
    // Operator recovery uses a fresh destination, avoiding stale WAL/SHM files.
    let restored = dir.path().join("restored.db");
    std::fs::copy(first, &restored).unwrap();
    let old = Connection::open(&restored).unwrap();
    assert_eq!(backup::schema_version(&old).unwrap(), 26);
    assert_projection(&old);
    drop(old);
    assert_projection(&Store::open(&restored).unwrap().conn);
}

#[test]
fn backup_retry_preserves_legacy_and_prior_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("lokai.db");
    let store = Store::open(&db).unwrap();
    let legacy = pre_migrate_backup_path(&db);
    std::fs::write(&legacy, b"legacy backup").unwrap();
    let first = store.create_pre_migration_backup().unwrap();
    let original = std::fs::read(&first).unwrap();
    let second = pre_migration_backup(&db).unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(first).unwrap(), original);
    assert_eq!(std::fs::read(legacy).unwrap(), b"legacy backup");
}

#[test]
fn future_schema_is_refused_without_migration_or_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("future.db");
    let store = Store::open(&db).unwrap();
    store
        .conn
        .execute(
            "INSERT INTO schema_versions VALUES (?1,'future')",
            [SCHEMA_TARGET_VERSION + 1],
        )
        .unwrap();
    drop(store);
    assert!(matches!(
        Store::open(&db),
        Err(StoreError::FutureSchema { .. })
    ));
    assert!(matches!(
        Store::open_readonly(&db),
        Err(StoreError::FutureSchema { .. })
    ));
    assert!(!pre_migrate_backup_directory(&db).exists());
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        backup::schema_version(&conn).unwrap(),
        SCHEMA_TARGET_VERSION + 1
    );
}

#[test]
fn simultaneous_upgrade_openers_serialize_and_preserve_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("lokai.db");
    seed_v26(&db);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let store = Store::open(db).unwrap();
                assert_projection(&store.conn);
                assert_eq!(
                    backup::schema_version(&store.conn).unwrap(),
                    SCHEMA_TARGET_VERSION
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(snapshots(&db).len(), 1);
}

#[test]
fn backup_restore_keeps_team_resources_and_private_history() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("lokai.db");
    let backup = {
        let store = Store::open(&db).unwrap();
        store.bootstrap_control("alice", "org", "Org").unwrap();
        store.register_control_principal("bob").unwrap();
        store
            .set_organization_member("org", "bob", OrganizationRole::Member)
            .unwrap();
        store
            .create_team(&TeamRow {
                org_id: "org".into(),
                team_id: "team".into(),
                name: "Team".into(),
                owner_principal_id: "alice".into(),
            })
            .unwrap();
        store.add_team_member("org", "team", "bob").unwrap();
        store
            .create_information_context(
                "alice",
                "private",
                &ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .unwrap();
        store
            .create_information_context(
                "alice",
                "shared",
                &ContextOwner::Team {
                    org_id: "org".into(),
                    team_id: "team".into(),
                },
            )
            .unwrap();
        let private_session = store.create_context_history("alice", "private").unwrap();
        store
            .append_context_message(
                "alice",
                "private",
                &private_session,
                "private-1",
                "PRIVATECANARY",
            )
            .unwrap();
        let team_session = store.create_context_history("alice", "shared").unwrap();
        store
            .append_context_message("alice", "shared", &team_session, "team-1", "TEAMVISIBLE")
            .unwrap();
        let backup = store.create_pre_migration_backup().unwrap();
        drop(store);
        backup
    };
    let restored = Store::open(&backup).unwrap();
    let private = restored
        .scoped_transcript("alice", "private", 
            &restored
                .conn
                .query_row(
                    "SELECT id FROM sessions WHERE context_id='private'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            10,
        )
        .unwrap();
    assert!(private.iter().any(|(_, _, text)| text == "PRIVATECANARY"));
    let denied = restored.scoped_transcript(
        "bob",
        "private",
        &restored
            .conn
            .query_row(
                "SELECT id FROM sessions WHERE context_id='private'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        10,
    );
    match denied {
        Err(StoreError::ControlAccessDenied) => {}
        other => panic!("restored private history must stay denied, got {other:?}"),
    }
    let team_session = restored
        .conn
        .query_row(
            "SELECT id FROM sessions WHERE context_id='shared'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let shared = restored
        .scoped_transcript("bob", "shared", &team_session, 10)
        .unwrap();
    assert!(shared.iter().any(|(_, _, text)| text == "TEAMVISIBLE"));
    assert_eq!(
        restored.get_team("org", "team").unwrap().unwrap().name,
        "Team"
    );
    let original = Store::open(&db).unwrap();
    assert!(original
        .scoped_transcript(
            "alice",
            "private",
            &original
                .conn
                .query_row(
                    "SELECT id FROM sessions WHERE context_id='private'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            10,
        )
        .unwrap()
        .iter()
        .any(|(_, _, text)| text == "PRIVATECANARY"));
}
