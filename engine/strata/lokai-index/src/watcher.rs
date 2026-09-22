//! Debounced filesystem watcher → incremental re-index (D6 / AR1-3).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify_debouncer_full::{new_debouncer, DebounceEventResult};

use crate::{Index, IndexError, Result};

/// Max paths indexed per debounced watcher flush (SEC2-E2-030).
pub(crate) const MAX_WATCHER_PATHS_PER_FLUSH: usize = 64;

/// Retained paths are bounded; overflow becomes one full reconciliation request.
const MAX_PENDING_PATHS: usize = 1024;

#[derive(Default)]
struct PendingUpdates {
    paths: BTreeSet<PathBuf>,
    dirty: bool,
}

impl PendingUpdates {
    fn reconcile(&mut self) {
        self.paths.clear();
        self.dirty = true;
    }

    fn push(&mut self, path: PathBuf) {
        if self.dirty {
            return;
        }
        self.paths.insert(path);
        if self.paths.len() > MAX_PENDING_PATHS {
            self.reconcile();
        }
    }

    fn receive(&mut self, result: DebounceEventResult, root: &Path) {
        let events = match result {
            Ok(events) => events,
            Err(errors) => {
                tracing::warn!(
                    count = errors.len(),
                    "index watcher error; scheduling reconciliation"
                );
                self.reconcile();
                return;
            }
        };
        for event in events {
            if matches!(event.kind, notify::EventKind::Access(_)) {
                continue;
            }
            // Missing paths can be deleted directories; ignore-policy changes
            // and incomplete notifications require a workspace-wide inventory.
            if event.need_rescan() || event.paths.is_empty() {
                self.reconcile();
            }
            for path in &event.paths {
                if !path_under_workspace(path, root) {
                    continue;
                }
                if !path.is_file()
                    || matches!(
                        path.file_name().and_then(|n| n.to_str()),
                        Some(".gitignore" | ".ignore")
                    )
                {
                    self.reconcile();
                } else {
                    self.push(path.clone());
                }
            }
        }
    }

    fn take(&mut self) -> Self {
        if self.dirty {
            return std::mem::take(self);
        }
        let mut batch = Self::default();
        for _ in 0..MAX_WATCHER_PATHS_PER_FLUSH {
            let Some(path) = self.paths.pop_first() else {
                break;
            };
            batch.paths.insert(path);
        }
        batch
    }

    fn apply(&self, index: &Index, root: &Path) -> Result<()> {
        if self.dirty {
            index.reconcile_workspace(root)?;
        } else if !self.paths.is_empty() {
            index.index_watcher_paths(root, &self.paths.iter().cloned().collect::<Vec<_>>())?;
        }
        Ok(())
    }
}

/// Handle to a background index watcher thread.
pub struct IndexWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl IndexWatcher {
    /// Spawn a debounced watcher (300ms). Returns `None` if the filesystem does
    /// not support notify (graceful disable per D6).
    pub fn spawn(index_db: PathBuf, workspace: PathBuf) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let thread = thread::Builder::new()
            .name("lokai-index-watcher".into())
            .spawn(move || {
                if let Err(e) = run_watcher(index_db, workspace, stop2) {
                    tracing::warn!("index watcher disabled: {e}");
                }
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for IndexWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_watcher(index_db: PathBuf, workspace: PathBuf, stop: Arc<AtomicBool>) -> Result<()> {
    let idx = Index::open(&index_db)?;
    // Preserve the caller's workspace key, including Windows short-path aliases.
    let workspace = if workspace.is_absolute() {
        workspace
    } else {
        std::env::current_dir()?.join(workspace)
    };
    let database = if index_db.is_absolute() {
        index_db
    } else {
        std::env::current_dir()?.join(index_db)
    };
    let pending = Arc::new(Mutex::new(PendingUpdates::default()));
    let pending_cb = pending.clone();
    let ws_root = workspace.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |mut res: DebounceEventResult| {
            if let Ok(events) = &mut res {
                events.retain_mut(|event| {
                    if event.need_rescan() || event.paths.is_empty() {
                        return true;
                    }
                    event
                        .paths
                        .retain(|path| !is_index_database_path(path, &database));
                    !event.paths.is_empty()
                });
            }
            pending_cb
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .receive(res, &ws_root);
        },
    )
    .map_err(|e| IndexError::Io(std::io::Error::other(e.to_string())))?;

    debouncer
        .watch(&workspace, notify::RecursiveMode::Recursive)
        .map_err(|e| IndexError::Io(std::io::Error::other(e.to_string())))?;

    tracing::info!(root = %workspace.display(), "live index watcher started");
    // Register first, then reconcile so startup mutations cannot fall in a gap.
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .reconcile();
    while !stop.load(Ordering::Relaxed) {
        // Do not hold the queue mutex during disk/SQLite work. Events arriving
        // during reconciliation remain queued for the next pass.
        let batch = pending.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Err(error) = batch.apply(&idx, &workspace) {
            tracing::warn!(%error, "index update failed; reconciliation will retry");
            pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reconcile();
            thread::sleep(Duration::from_secs(1));
        } else {
            thread::sleep(Duration::from_millis(250));
        }
    }
    Ok(())
}

fn is_index_database_path(path: &Path, database: &Path) -> bool {
    let path = crate::workspace_storage_key(path);
    let database = crate::workspace_storage_key(database);
    path == database
        || ["-wal", "-shm", "-journal"]
            .iter()
            .any(|suffix| path == format!("{database}{suffix}"))
}

fn path_under_workspace(path: &Path, workspace: &Path) -> bool {
    let root = PathBuf::from(crate::workspace_storage_key(workspace));
    let path = PathBuf::from(crate::workspace_storage_key(path));
    path.is_absolute()
        && path.starts_with(root)
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Block until the process exits (Ctrl+C). For `lokai --watch-index`.
pub fn watch_index_blocking(index_db: &Path, workspace: &Path) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let index_db = index_db.to_path_buf();
    let workspace = workspace.to_path_buf();
    run_watcher(index_db, workspace, stop)
}

#[cfg(test)]
#[path = "watcher_tests.rs"]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    #[test]
    fn mutex_index_is_safe_across_threads() {
        let dir = std::env::temp_dir().join(format!("lokai-mutex-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "pub fn a() {}\n").unwrap();
        let db = std::env::temp_dir().join(format!("lokai-mutex-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let idx = Arc::new(Mutex::new(Index::open(&db).unwrap()));
        idx.lock().unwrap().index_workspace(&dir).unwrap();
        let ws = dir.to_string_lossy().to_string();

        let h1 = {
            let idx = idx.clone();
            let ws = ws.clone();
            thread::spawn(move || idx.lock().unwrap().status(&ws).unwrap().files)
        };
        let h2 = {
            let idx = idx.clone();
            let ws = ws.clone();
            thread::spawn(move || idx.lock().unwrap().find_definition(&ws, "a").unwrap().len())
        };
        assert_eq!(h1.join().unwrap(), 1);
        assert_eq!(h2.join().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn watcher_reindexes_on_file_change() {
        let dir = std::env::temp_dir().join(format!("lokai-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("live.rs"), "pub fn live() {}\n").unwrap();

        let db = std::env::temp_dir().join(format!("lokai-watch-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        {
            let idx = Index::open(&db).unwrap();
            idx.index_workspace(&dir).unwrap();
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let db_bg = db.clone();
        let dir_bg = dir.clone();
        let handle = thread::spawn(move || run_watcher(db_bg, dir_bg, stop_bg));

        // Allow the watcher thread to register before we mutate files.
        thread::sleep(Duration::from_millis(800));
        std::fs::write(
            dir.join("live.rs"),
            "pub fn live() { 1 }\npub fn extra() {}\n",
        )
        .unwrap();

        let ws = dir.to_string_lossy().to_string();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut saw_extra = false;
        while Instant::now() < deadline {
            if Index::open(&db)
                .unwrap()
                .find_definition(&ws, "extra")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            {
                saw_extra = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();

        assert!(saw_extra, "watcher should pick up edited file within 15s");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db);
    }
}
