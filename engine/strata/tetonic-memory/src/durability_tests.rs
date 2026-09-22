use super::*;

fn assert_durable(conn: &Connection) {
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    for (pragma, expected) in [
        ("PRAGMA synchronous", 2),
        ("PRAGMA fullfsync", 1),
        ("PRAGMA checkpoint_fullfsync", 1),
    ] {
        let actual: i64 = conn.query_row(pragma, [], |r| r.get(0)).unwrap();
        assert_eq!(actual, expected, "{pragma}");
    }
}

#[test]
fn durable_policy_applies_to_every_connection_owner_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.db");
    for _ in 0..2 {
        let store = Store::open(&path).unwrap();
        assert_durable(&store.conn);
        let reader = Store::open_readonly(&path).unwrap();
        assert_durable(&reader.conn);
        let worker = WorkerStore::open(dir.path().join("worker.db")).unwrap();
        assert_durable(&worker.conn);
    }
    let shared = SharedStore::open(&path, 1).unwrap();
    shared
        .write_sync(|store| assert_durable(&store.conn))
        .unwrap();
    shared
        .read_sync(|store| assert_durable(&store.conn))
        .unwrap();
}

#[test]
fn acknowledged_commit_survives_process_exit_without_connection_cleanup() {
    const CHILD_PATH: &str = "LOKAI_DURABILITY_TEST_DB";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let store = Store::open(path).unwrap();
        assert_durable(&store.conn);
        store
            .conn
            .execute_batch(
                "BEGIN IMMEDIATE;
             CREATE TABLE durability_probe (value TEXT NOT NULL);
             INSERT INTO durability_probe VALUES ('acknowledged');
             COMMIT;",
            )
            .unwrap();
        // Skip Rust destructors and SQLite connection-close/checkpoint cleanup.
        // This is process-exit evidence, not a power-loss experiment.
        std::process::exit(23);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.db");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "durability_tests::acknowledged_commit_survives_process_exit_without_connection_cleanup"])
        .env(CHILD_PATH, &path)
        .status().unwrap();
    assert_eq!(status.code(), Some(23));
    let store = Store::open(path).unwrap();
    let value: String = store
        .conn
        .query_row("SELECT value FROM durability_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "acknowledged");
    assert_durable(&store.conn);
}
