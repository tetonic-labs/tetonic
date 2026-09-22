use super::*;
use notify_debouncer_full::DebouncedEvent;

struct Fixture {
    dir: PathBuf,
    root: PathBuf,
    index: Index,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lokai-watcher-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = dir.join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let index = Index::open(dir.join("index.db")).unwrap();
        Self { dir, root, index }
    }

    fn snapshot(&self, index: &Index) -> Vec<(String, String)> {
        let mut stmt = index
            .conn
            .prepare("SELECT rel, content_hash FROM files ORDER BY rel")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    fn assert_rebuilt(&self) {
        let rebuilt = Index::open(self.dir.join("rebuilt.db")).unwrap();
        rebuilt.index_workspace(&self.root).unwrap();
        assert_eq!(self.snapshot(&self.index), self.snapshot(&rebuilt));
        let ws = crate::workspace_storage_key(&self.root);
        for symbol in ["after", "renamed", "obsolete"] {
            let actual = self.index.find_definition(&ws, symbol).unwrap();
            let expected = rebuilt.find_definition(&ws, symbol).unwrap();
            assert_eq!(actual.len(), expected.len(), "symbol {symbol}");
            let actual = self.index.search(&ws, symbol, 200).unwrap();
            let expected = rebuilt.search(&ws, symbol, 200).unwrap();
            assert_eq!(actual.len(), expected.len(), "search {symbol}");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SQLite handles may still be open on Windows; cleanup is best effort.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn burst_drains_without_another_event_and_matches_rebuild() {
    let f = Fixture::new();
    let mut pending = PendingUpdates::default();
    for i in 0..200 {
        let path = f.root.join(format!("file{i:03}.rs"));
        std::fs::write(&path, "pub fn before() {}\n").unwrap();
        pending.push(path);
    }
    f.index.index_workspace(&f.root).unwrap();
    for path in &pending.paths {
        std::fs::write(path, "pub fn after() {}\n").unwrap();
    }
    // Force matching metadata to verify explicit events still hash file content.
    f.index
        .conn
        .execute(
            "UPDATE files SET size = ?1, mtime = ?2",
            rusqlite::params![
                "pub fn after() {}\n".len(),
                std::fs::metadata(pending.paths.first().unwrap())
                    .unwrap()
                    .modified()
                    .unwrap()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            ],
        )
        .unwrap();
    let mut batches = 0;
    while !pending.paths.is_empty() {
        let batch = pending.take();
        assert!(batch.paths.len() <= MAX_WATCHER_PATHS_PER_FLUSH);
        batch.apply(&f.index, &f.root).unwrap();
        batches += 1;
    }
    assert_eq!(batches, 4);
    f.assert_rebuilt();
}

#[test]
fn overflow_and_watcher_errors_reconcile_renames_and_deleted_directories() {
    for error in [false, true] {
        let f = Fixture::new();
        let old = f.root.join("old");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("obsolete.rs"), "pub fn obsolete() {}\n").unwrap();
        std::fs::write(f.root.join("before.rs"), "pub fn renamed() {}\n").unwrap();
        f.index.index_workspace(&f.root).unwrap();
        std::fs::remove_dir_all(old).unwrap();
        std::fs::rename(f.root.join("before.rs"), f.root.join("after.rs")).unwrap();
        let mut pending = PendingUpdates::default();
        if error {
            pending.receive(
                Err(vec![notify::Error::generic("lost notifications")]),
                &f.root,
            );
        } else {
            for i in 0..MAX_PENDING_PATHS + 20 {
                pending.push(f.root.join(format!("event{i}.rs")));
            }
        }
        assert!(pending.dirty);
        assert!(pending.paths.is_empty());
        let batch = pending.take();
        // Events arriving during a scan survive completion of that scan.
        pending.push(f.root.join("after.rs"));
        batch.apply(&f.index, &f.root).unwrap();
        assert_eq!(pending.paths.len(), 1);
        f.assert_rebuilt();
    }
}

#[test]
fn failed_reconciliation_preserves_existing_rows_and_can_retry() {
    let f = Fixture::new();
    std::fs::write(f.root.join("keep.rs"), "pub fn after() {}\n").unwrap();
    f.index.index_workspace(&f.root).unwrap();
    let before = f.snapshot(&f.index);
    let moved = f.dir.join("temporarily-unavailable");
    std::fs::rename(&f.root, &moved).unwrap();
    let mut pending = PendingUpdates::default();
    pending.reconcile();
    assert!(pending.take().apply(&f.index, &f.root).is_err());
    assert_eq!(f.snapshot(&f.index), before);
    pending.reconcile();
    std::fs::rename(moved, &f.root).unwrap();
    pending.take().apply(&f.index, &f.root).unwrap();
    f.assert_rebuilt();
}

#[test]
fn removed_directory_and_rescan_notifications_schedule_reconciliation() {
    let f = Fixture::new();
    let mut pending = PendingUpdates::default();
    let event = notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder))
        .add_path(f.root.join("deleted"));
    pending.receive(
        Ok(vec![DebouncedEvent::new(event, std::time::Instant::now())]),
        &f.root,
    );
    assert!(pending.take().dirty);
    let event = notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan);
    pending.receive(
        Ok(vec![DebouncedEvent::new(event, std::time::Instant::now())]),
        &f.root,
    );
    assert!(pending.dirty);
    assert!(!path_under_workspace(&f.root.join("../outside"), &f.root));
    assert!(!path_under_workspace(Path::new("relative.rs"), &f.root));
}

#[test]
fn new_paths_follow_ignore_rules_and_database_notifications_are_excluded() {
    let f = Fixture::new();
    std::fs::write(f.root.join(".gitignore"), "ignored.rs\n").unwrap();
    std::fs::write(f.root.join("ignored.rs"), "pub fn obsolete() {}\n").unwrap();
    std::fs::write(f.root.join("visible.rs"), "pub fn after() {}\n").unwrap();
    let mut pending = PendingUpdates::default();
    pending.push(f.root.join("ignored.rs"));
    pending.push(f.root.join("visible.rs"));
    pending.take().apply(&f.index, &f.root).unwrap();
    f.assert_rebuilt();
    assert_eq!(f.snapshot(&f.index).len(), 1);
    let db = f.root.join("index.db");
    for suffix in ["", "-wal", "-shm", "-journal"] {
        assert!(is_index_database_path(
            &f.root.join(format!("index.db{suffix}")),
            &db
        ));
    }
    assert!(!is_index_database_path(&f.root.join("index.db.rs"), &db));
}

#[test]
fn native_watcher_converges_after_a_large_burst_and_directory_deletion() {
    let f = Fixture::new();
    let old = f.root.join("old");
    std::fs::create_dir(&old).unwrap();
    std::fs::write(old.join("obsolete.rs"), "pub fn obsolete() {}\n").unwrap();
    f.index.index_workspace(&f.root).unwrap();
    let mut watcher = IndexWatcher::spawn(f.dir.join("index.db"), f.root.clone()).unwrap();
    thread::sleep(Duration::from_millis(800));
    for i in 0..140 {
        std::fs::write(f.root.join(format!("new{i}.rs")), "pub fn after() {}\n").unwrap();
    }
    std::fs::remove_dir_all(old).unwrap();
    std::fs::rename(f.root.join("new0.rs"), f.root.join("renamed.rs")).unwrap();
    let rebuilt = Index::open(f.dir.join("expected.db")).unwrap();
    rebuilt.index_workspace(&f.root).unwrap();
    let expected = f.snapshot(&rebuilt);
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while f.snapshot(&f.index) != expected && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    watcher.stop();
    assert_eq!(f.snapshot(&f.index), expected);
    f.assert_rebuilt();
}
