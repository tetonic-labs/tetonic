//! Local audit/memory store — the precious `lokai.db` (Phase A slice).
//!
//! This is the on-disk, **local-only** record of what the agent did: sessions,
//! the full conversation, every tool call, structured events, and a mirror of
//! the egress log. It exists so the user can *see and trust* (and later undo)
//! the agent's actions — see contracts/memory-store-v2.md.
//!
//! By construction this crate has **no network path**: it never depends on
//! `lokai-egress` or any HTTP client. It holds project content, so it stays on
//! the machine.
//!
//! This slice ships the audit spine (sessions, messages, tool_calls, events,
//! egress_log), `file_changes` (undo), time-travel (`checkpoints`/`workspace_head`/`restores`),
//! trust/approvals (v9), project memory, recall FTS, estate, and capacity profiles.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

mod backup;
mod blob;
mod capability;
mod capacity;
mod capacity_tables;
mod compute_reservation;
#[cfg(test)]
mod durability_tests;
mod estate;
mod identity_store;
mod control_credentials;
mod control_bootstrap;
mod membership_store;
mod membership_admin;
mod team_store;
#[cfg(test)]
mod migration_tests;
pub mod payload_digest;
mod policy;
mod projects;
mod recall;
mod result_disposition;
mod run_store;
mod scheduler_decision;
mod schema;
mod secret_overrides;
mod sync_lock;
mod trust;
mod util;
mod worker_store;

pub use backup::{
    is_ephemeral_db_path, pre_migrate_backup_directory, pre_migrate_backup_path,
    pre_migration_backup, SCHEMA_TARGET_VERSION,
};
pub use sync_lock::{mutex_lock, RecoverMutex};
pub use util::{new_id, workspace_storage_key, workspace_storage_key_str};

pub use identity_store::AgentIdentityRow;
pub use control_credentials::ControlCredentialRow;
pub use membership_store::{ControlPermission, OrganizationRole};
pub use team_store::{OrganizationRow, TeamRow};
pub use recall::RecallHit;
pub use trust::{ApprovalRow, EgressAllowRow};

pub use capacity::RuntimeProfileRow;
pub use compute_reservation::ComputeReservationRow;
pub use estate::{OwnerIdentityRow, WorkerEnrollmentRow};
pub use result_disposition::DurableDispositionRecord;
pub use run_store::CompactReport;
pub use scheduler_decision::SchedulerDecisionRow;
pub use secret_overrides::{SecretOverrideAuditRow, SecretOverrideRow, SecretOverrideScope};
pub use worker_store::{CoordinatorPinRow, WorkerStore, WorkerStoreError};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database schema {found} is newer than supported schema {supported}; use a compatible binary")]
    FutureSchema { found: i64, supported: i64 },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid policy mode: {0}")]
    InvalidPolicyMode(String),
    #[error("invalid data class: {0}")]
    InvalidDataClass(String),
    /// A stored or supplied `payload_digest` does not describe its payload
    /// (M6, INV-RUN-003).
    ///
    /// Named for what it detects. This is corruption-detection, not
    /// tamper-evidence: `payload_json` and `payload_digest` are adjacent
    /// plaintext columns, so an actor who can write the database can rewrite
    /// both consistently. Calling it an integrity violation would claim more.
    #[error("payload digest mismatch: {0}")]
    DigestMismatch(String),
    #[error("capability already consumed")]
    CapabilityAlreadyConsumed,
    #[error("capability revoked")]
    CapabilityRevoked,
    #[error("capability expired")]
    CapabilityExpired,
    #[error("invalid control resource: {0}")]
    InvalidControlResource(String),
    #[error("control resource already exists with different attributes")]
    ControlResourceConflict,
    #[error("control access denied")]
    ControlAccessDenied,
    #[error("organization must retain an enabled administrator")]
    LastOrganizationAdministrator,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Map bare `:memory:` to a shared-cache URI so writer + read-pool connections
/// see the same database (plain `:memory:` is per-connection in SQLite).
fn resolve_shared_store_path(path: &Path) -> Result<PathBuf> {
    if path == Path::new(":memory:") {
        static MEM_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = MEM_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let uri = format!(
            "file:lokai-shared-mem-{}-{}?mode=memory&cache=shared",
            std::process::id(),
            n
        );
        return Ok(PathBuf::from(uri));
    }
    Ok(path.to_path_buf())
}

fn open_flags_for(path: &Path, readonly: bool) -> rusqlite::OpenFlags {
    let mut flags = if readonly {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
    };
    if path
        .to_str()
        .is_some_and(|s| s.starts_with("file:") || s.contains("mode=memory"))
    {
        flags |= rusqlite::OpenFlags::SQLITE_OPEN_URI;
    }
    flags
}

/// A file change with before/after content decompressed (for undo).
#[derive(Debug, Clone)]
pub struct FileChangeRow {
    pub id: i64,
    pub tool_call_id: String,
    /// Workspace-relative path.
    pub path: String,
    pub change_kind: String,
    /// Prior content (`None` if the file was created in this change).
    pub before: Option<String>,
    /// New content (`None` if the file was deleted in this change).
    pub after: Option<String>,
}

/// A named checkpoint: a saved position on the file-change timeline.
#[derive(Debug, Clone)]
pub struct CheckpointRow {
    pub id: String,
    pub label: String,
    /// `file_changes.id` high-water at creation (0 = empty workspace).
    pub mark: i64,
    pub kind: String,
    pub created_at: String,
}

/// A session summary row for the history view.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub status: String,
    pub model: String,
    pub workspace_root: String,
    pub started_at: String,
    pub messages: i64,
    pub tool_calls: i64,
    pub file_changes: i64,
}

use blob::{decode_blob, decode_change_tuples, encode_blob};
use util::now;

/// The audit/memory store. Owns a single SQLite connection to `lokai.db`.
///
/// `rusqlite::Connection` is `Send` but not `Sync`; the store is therefore meant
/// to be owned by the engine task that drives a session (it is not shared across
/// threads). All methods take `&self` (SQLite uses interior locking).
pub struct Store {
    pub(crate) conn: Connection,
    path: PathBuf,
}

pub type WriteCommand = Box<dyn FnOnce(&mut Store) + Send + 'static>;

/// Thread-safe handle to `lokai.db` (H2-2).
///
/// **Write path:** a dedicated OS thread owns the writable connection. Callers
/// enqueue work on an `mpsc` channel and await a oneshot; they never hold a
/// `std::sync::Mutex` across SQLite I/O. The single writer serializes all
/// commits, so the run journal stays append-ordered globally (and therefore
/// per-run) without callers coordinating.
///
/// **Read path:** a pool of `SQLITE_OPEN_READ_ONLY` connections (WAL) so
/// analytical queries (recall FTS, capacity doctor) do not sit on the writer
/// lock. Async reads run inside `spawn_blocking`.
///
/// Prefer [`SharedStore::write`] / [`SharedStore::read`] from `async fn`. Use
/// `*_sync` only from sync startup / CLI helpers — never from an async body
/// (enforced by `lokai-arch-gate` `async_sync_calls`).
#[derive(Clone)]
pub struct SharedStore {
    writer_tx: std::sync::mpsc::Sender<WriteCommand>,
    read_pool: std::sync::Arc<std::sync::Mutex<Vec<Store>>>,
    path: PathBuf,
}

#[cfg(test)]
mod read_failure_tests {
    use super::*;

    #[tokio::test]
    async fn read_open_failure_is_returned_and_panicked_reader_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let store = SharedStore::open(dir.path().join("test.db"), 1).unwrap();
        assert!(store
            .read(|_| -> () { panic!("reader failed") })
            .await
            .is_err());
        assert_eq!(store.read(|_| 42).await.unwrap(), 42);
        assert_eq!(store.write(|_| 43).await.unwrap(), 43);
        let mut unavailable = store.clone();
        unavailable.read_pool = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        unavailable.path = dir.path().join("missing").join("absent.db");
        assert!(unavailable.read(|_| 0).await.is_err());
    }

    #[test]
    fn synchronous_read_open_failure_is_returned() {
        let dir = tempfile::tempdir().unwrap();
        let mut unavailable = SharedStore::open(dir.path().join("test.db"), 0).unwrap();
        unavailable.path = dir.path().join("missing").join("absent.db");
        assert!(unavailable.read_sync(|_| 0).is_err());
    }
}

impl SharedStore {
    pub fn open(path: impl AsRef<Path>, pool_size: usize) -> Result<Self> {
        let path = resolve_shared_store_path(path.as_ref())?;
        let writer_store = Store::open(&path)?;

        let (tx, rx) = std::sync::mpsc::channel::<WriteCommand>();

        std::thread::spawn(move || {
            let mut store = writer_store;
            while let Ok(cmd) = rx.recv() {
                cmd(&mut store);
            }
        });

        let mut readers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            readers.push(Store::open_readonly(&path)?);
        }

        Ok(Self {
            writer_tx: tx,
            read_pool: std::sync::Arc::new(std::sync::Mutex::new(readers)),
            path,
        })
    }

    /// On-disk path for this shared store (for migration backup / diagnostics).
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn record_wait(op: &'static str, wait_ms: u64) {
        tracing::trace!(target: "tetonic_memory", store_wait_ms = wait_ms, op, "store-wait");
        tetonic_telemetry::record_store_wait(op, wait_ms);
    }

    pub async fn write<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Store) -> R + Send + 'static,
        R: Send + 'static,
    {
        let enqueued_at = std::time::Instant::now();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cmd = Box::new(move |store: &mut Store| {
            Self::record_wait("write", enqueued_at.elapsed().as_millis() as u64);
            let res = f(store);
            let _ = tx.send(res);
        });
        self.writer_tx
            .send(cmd)
            .map_err(|_| StoreError::Io(std::io::Error::other("writer actor closed")))?;
        rx.await
            .map_err(|_| StoreError::Io(std::io::Error::other("writer actor dropped response")))
    }

    pub fn write_sync<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Store) -> R + Send + 'static,
        R: Send + 'static,
    {
        let enqueued_at = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd = Box::new(move |store: &mut Store| {
            Self::record_wait("write", enqueued_at.elapsed().as_millis() as u64);
            let res = f(store);
            let _ = tx.send(res);
        });
        self.writer_tx
            .send(cmd)
            .map_err(|_| StoreError::Io(std::io::Error::other("writer actor closed")))?;
        rx.recv()
            .map_err(|_| StoreError::Io(std::io::Error::other("writer actor dropped response")))
    }

    pub async fn read<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Store) -> R + Send + 'static,
        R: Send + 'static,
    {
        let enqueued_at = std::time::Instant::now();
        let read_pool = self.read_pool.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> Result<R> {
            let cached = read_pool
                .lock()
                .map_err(|_| StoreError::Io(std::io::Error::other("read pool poisoned")))?
                .pop();
            let store = match cached {
                Some(store) => store,
                None => Store::open_readonly(&path)?,
            };
            Self::record_wait("read", enqueued_at.elapsed().as_millis() as u64);
            // On panic, Drop closes this connection. Do not recycle a connection
            // whose callback may have left SQLite state partially configured.
            let res = f(&store);
            read_pool
                .lock()
                .map_err(|_| StoreError::Io(std::io::Error::other("read pool poisoned")))?
                .push(store);
            Ok(res)
        })
        .await
        .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?
    }

    pub fn read_sync<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Store) -> R,
    {
        let enqueued_at = std::time::Instant::now();
        let cached = self
            .read_pool
            .lock()
            .map_err(|_| StoreError::Io(std::io::Error::other("read pool poisoned")))?
            .pop();
        let store = match cached {
            Some(store) => store,
            None => Store::open_readonly(&self.path)?,
        };
        Self::record_wait("read", enqueued_at.elapsed().as_millis() as u64);
        let res = f(&store);
        self.read_pool
            .lock()
            .map_err(|_| StoreError::Io(std::io::Error::other("read pool poisoned")))?
            .push(store);
        Ok(res)
    }
}

impl Store {
    /// Open (creating if needed) the audit DB at `path`, applying pragmas and
    /// migrations. The parent directory is created if missing.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !path
                .to_str()
                .is_some_and(|s| s.starts_with("file:") || s == ":memory:")
            {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = if path == Path::new(":memory:") {
            Connection::open(&path)?
        } else {
            Connection::open_with_flags(&path, open_flags_for(&path, false))?
        };
        backup::ensure_supported_schema(&conn)?;
        // Acknowledged writes must request a WAL sync at each commit. NORMAL
        // allows acknowledged transactions to disappear after power loss.
        conn.execute_batch(
            "PRAGMA busy_timeout=5000;\n\
             PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=FULL;\n\
             PRAGMA fullfsync=ON;\n\
             PRAGMA checkpoint_fullfsync=ON;\n\
             PRAGMA foreign_keys=ON;",
        )?;
        let store = Self { conn, path };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open_with_flags(&path, open_flags_for(&path, true))?;
        backup::ensure_supported_schema(&conn)?;
        conn.execute_batch(
            "PRAGMA busy_timeout=5000;\n\
             PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=FULL;\n\
             PRAGMA fullfsync=ON;\n\
             PRAGMA checkpoint_fullfsync=ON;\n\
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(Self { conn, path })
    }

    /// Where this store lives on disk (for user-facing "your data is here").
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current SQLite journal mode (H2-2 / WAL recovery checks).
    pub fn journal_mode(&self) -> Result<String> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(Into::into)
    }

    /// Reclaims unused disk and memory pages via SQLite vacuum (OPT-702 memory hygiene).
    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute("VACUUM", [])?;
        Ok(())
    }

    /// Optimizes database query planner statistics and frees cached memory.
    pub fn optimize(&self) -> Result<()> {
        self.conn.execute("PRAGMA optimize", [])?;
        self.conn.execute("PRAGMA shrink_memory", [])?;
        Ok(())
    }

    /// Open a new session row; returns its id.
    pub fn start_session(&self, workspace_root: &str, mode: &str, model: &str) -> Result<String> {
        let ws = self.normalize_workspace(workspace_root);
        let id = new_id("sess");
        self.conn.execute(
            "INSERT INTO sessions(id, workspace_root, mode, model, status, started_at)\n\
             VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
            params![id, ws, mode, model, now()],
        )?;
        Ok(id)
    }

    /// Close a session with a terminal status (`ok` | `error` | `canceled`).
    pub fn end_session(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET status = ?2, ended_at = ?3, error = ?4 WHERE id = ?1",
            params![id, status, now(), error.unwrap_or("")],
        )?;
        Ok(())
    }

    /// Append a conversation message; `seq` is assigned monotonically per session.
    pub fn append_message(
        &self,
        session_id: &str,
        role: &str,
        agent_id: &str,
        content: &str,
        tool_calls_json: Option<&str>,
    ) -> Result<i64> {
        self.append_message_with(
            session_id,
            role,
            agent_id,
            content,
            tool_calls_json,
            None,
            None,
        )
    }

    /// Append a message including tool-result linkage (H3-1).
    #[allow(clippy::too_many_arguments)]
    pub fn append_message_with(
        &self,
        session_id: &str,
        role: &str,
        agent_id: &str,
        content: &str,
        tool_calls_json: Option<&str>,
        tool_name: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> Result<i64> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO messages(session_id, seq, role, agent_id, content, tool_calls_json, tool_name, tool_call_id, created_at)\n\
             VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?1), ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        stmt.execute(params![
            session_id,
            role,
            agent_id,
            content,
            tool_calls_json,
            tool_name,
            tool_call_id,
            now()
        ])?;
        let row_id = self.conn.last_insert_rowid();
        recall::after_message_insert(self, session_id, role, content);
        Ok(row_id)
    }

    /// Record a settled tool call (this slice records the final state in one shot;
    /// the proposed→approved→settled transitions land with approvals in Phase D).
    #[allow(clippy::too_many_arguments)]
    pub fn record_tool_call(
        &self,
        id: &str,
        session_id: &str,
        tool: &str,
        args_json: &str,
        ok: bool,
        result_summary: &str,
        error_kind: Option<&str>,
    ) -> Result<()> {
        // `denied` (user blocked it) is recorded distinctly from genuine errors.
        let status = if ok {
            "ok"
        } else if error_kind == Some("denied") {
            "denied"
        } else {
            "error"
        };
        let ts = now();
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO tool_calls(id, session_id, tool, args_json, status, result_summary, error_kind, created_at, settled_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(id) DO UPDATE SET
               status = excluded.status,
               result_summary = excluded.result_summary,
               error_kind = excluded.error_kind,
               settled_at = excluded.settled_at",
        )?;
        stmt.execute(params![
            id,
            session_id,
            tool,
            args_json,
            status,
            result_summary,
            error_kind,
            ts
        ])?;
        if ok {
            let body = if result_summary.trim().is_empty() {
                args_json.to_string()
            } else {
                result_summary.to_string()
            };
            recall::after_tool_insert(self, session_id, tool, &body);
        }
        Ok(())
    }

    /// Persist a file mutation with zstd-compressed before/after blobs (so the
    /// change can later be rendered and reversed). `before`/`after` are full
    /// content; `None` means "no such side" (create has no before; delete no
    /// after). Diff-encoding for edits is a future size optimization.
    pub fn record_file_change(
        &self,
        tool_call_id: &str,
        session_id: &str,
        path: &str,
        change_kind: &str,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<()> {
        let compress = |s: Option<&str>| -> Result<Option<Vec<u8>>> {
            match s {
                Some(text) => Ok(Some(encode_blob(text)?)),
                None => Ok(None),
            }
        };
        let before_blob = compress(before)?;
        let after_blob = compress(after)?;
        let ts = now();
        self.conn.execute("BEGIN IMMEDIATE", [])?;
        let result = (|| -> Result<()> {
            self.conn.execute(
                "INSERT INTO file_changes(session_id, tool_call_id, path, change_kind, blob_encoding, before_blob, after_blob, applied_at)\n\
                 VALUES (?1, ?2, ?3, ?4, 'zstd-full', ?5, ?6, ?7)",
                params![session_id, tool_call_id, path, change_kind, before_blob, after_blob, ts],
            )?;
            let new_id = self.conn.last_insert_rowid();
            self.conn.execute(
                "UPDATE workspace_head SET head_mark = ?1, redo_mark = NULL, updated_at = ?2\n\
                 WHERE workspace_root = (SELECT workspace_root FROM sessions WHERE id = ?3)",
                params![new_id, ts, session_id],
            )?;
            Ok(())
        })();
        if result.is_ok() {
            self.conn.execute("COMMIT", [])?;
        } else {
            let _ = self.conn.execute("ROLLBACK", []);
        }
        result
    }

    /// Highest `file_changes.id` for this session, or 0 if none (H3-2 turn mark).
    pub fn session_file_change_highwater(&self, session_id: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM file_changes WHERE session_id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// The ordered file changes of a session (oldest first), with before/after
    /// content decompressed — everything needed to reverse the session's writes.
    pub fn session_file_changes(&self, session_id: &str) -> Result<Vec<FileChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, tool_call_id, path, change_kind, before_blob, after_blob\n\
             FROM file_changes WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![session_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?,
                    r.get::<_, Option<Vec<u8>>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        decode_change_tuples(rows)
    }

    /// File changes for a *workspace* whose id falls in `(low_excl, high_incl]`,
    /// oldest first. This is the core query for time-travel: it spans every
    /// session that ran in the workspace, ordered by the global change id.
    pub fn workspace_changes_in_range(
        &self,
        workspace_root: &str,
        low_excl: i64,
        high_incl: i64,
    ) -> Result<Vec<FileChangeRow>> {
        let ws = self.normalize_workspace(workspace_root);
        let mut stmt = self.conn.prepare(
            "SELECT fc.id, fc.tool_call_id, fc.path, fc.change_kind, fc.before_blob, fc.after_blob\n\
             FROM file_changes fc JOIN sessions s ON fc.session_id = s.id\n\
             WHERE s.workspace_root = ?1 AND fc.id > ?2 AND fc.id <= ?3\n\
             ORDER BY fc.id",
        )?;
        let rows = stmt
            .query_map(params![ws, low_excl, high_incl], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?,
                    r.get::<_, Option<Vec<u8>>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        decode_change_tuples(rows)
    }

    /// The canonical workspace root recorded for a session (where its relative
    /// file paths resolve). `None` if the session doesn't exist.
    pub fn session_workspace_root(&self, session_id: &str) -> Result<Option<String>> {
        let root: Option<String> = self
            .conn
            .query_row(
                "SELECT workspace_root FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(root)
    }

    /// Read back and decompress the `after` blob of a file change (for undo /
    /// verification). Returns `None` if the row or blob is absent.
    pub fn file_change_after(&self, id: i64) -> Result<Option<String>> {
        let blob: Option<Vec<u8>> = self.conn.query_row(
            "SELECT after_blob FROM file_changes WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        decode_blob(blob)
    }

    // ---- Time-travel: head cursor, checkpoints, boundaries, restore log -------

    /// The highest change id ever recorded for a workspace (the natural "tip").
    pub fn workspace_max_mark(&self, workspace_root: &str) -> Result<i64> {
        let ws = self.normalize_workspace(workspace_root);
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(fc.id), 0) FROM file_changes fc\n\
             JOIN sessions s ON fc.session_id = s.id WHERE s.workspace_root = ?1",
            params![ws],
            |r| r.get(0),
        )?)
    }

    /// The cursor row for a workspace: `(head_mark, redo_mark)`. `None` if this
    /// workspace has never time-travelled (head is then implicitly the tip).
    pub fn head_state(&self, workspace_root: &str) -> Result<Option<(i64, Option<i64>)>> {
        let ws = self.normalize_workspace(workspace_root);
        Ok(self
            .conn
            .query_row(
                "SELECT head_mark, redo_mark FROM workspace_head WHERE workspace_root = ?1",
                params![ws],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?)
    }

    /// The change id currently applied to the workspace: the cursor if present,
    /// else the tip (everything applied).
    pub fn current_head(&self, workspace_root: &str) -> Result<i64> {
        match self.head_state(workspace_root)? {
            Some((head, _)) => Ok(head),
            None => self.workspace_max_mark(workspace_root),
        }
    }

    /// Move the cursor (upsert). `redo_mark = None` clears any pending redo.
    pub fn set_head(
        &self,
        workspace_root: &str,
        head_mark: i64,
        redo_mark: Option<i64>,
    ) -> Result<()> {
        let ws = self.normalize_workspace(workspace_root);
        self.conn.execute(
            "INSERT INTO workspace_head(workspace_root, head_mark, redo_mark, updated_at)\n\
             VALUES (?1, ?2, ?3, ?4)\n\
             ON CONFLICT(workspace_root) DO UPDATE SET\n\
                 head_mark = excluded.head_mark,\n\
                 redo_mark = excluded.redo_mark,\n\
                 updated_at = excluded.updated_at",
            params![ws, head_mark, redo_mark, now()],
        )?;
        Ok(())
    }

    /// Create a named checkpoint at the current head; returns `(id, mark)`.
    pub fn create_checkpoint(
        &self,
        workspace_root: &str,
        label: &str,
        kind: &str,
        session_id: Option<&str>,
    ) -> Result<(String, i64)> {
        let ws = self.normalize_workspace(workspace_root);
        let mark = self.current_head(&ws)?;
        let id = new_id("ckpt");
        self.conn.execute(
            "INSERT INTO checkpoints(id, workspace_root, label, mark, kind, session_id, created_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, ws, label, mark, kind, session_id, now()],
        )?;
        Ok((id, mark))
    }

    /// Checkpoints for a workspace, newest position first.
    pub fn list_checkpoints(&self, workspace_root: &str) -> Result<Vec<CheckpointRow>> {
        let ws = self.normalize_workspace(workspace_root);
        let mut stmt = self.conn.prepare(
            "SELECT id, label, mark, kind, created_at FROM checkpoints\n\
             WHERE workspace_root = ?1 ORDER BY mark DESC, created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![ws], |r| {
                Ok(CheckpointRow {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    mark: r.get(2)?,
                    kind: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Resolve a checkpoint reference (exact id first, else most recent matching
    /// label) within a workspace.
    pub fn find_checkpoint(
        &self,
        workspace_root: &str,
        reference: &str,
    ) -> Result<Option<CheckpointRow>> {
        let ws = self.normalize_workspace(workspace_root);
        let map = |r: &rusqlite::Row| {
            Ok(CheckpointRow {
                id: r.get(0)?,
                label: r.get(1)?,
                mark: r.get(2)?,
                kind: r.get(3)?,
                created_at: r.get(4)?,
            })
        };
        let by_id = self
            .conn
            .query_row(
                "SELECT id, label, mark, kind, created_at FROM checkpoints\n\
                 WHERE workspace_root = ?1 AND id = ?2",
                params![ws, reference],
                map,
            )
            .optional()?;
        if by_id.is_some() {
            return Ok(by_id);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT id, label, mark, kind, created_at FROM checkpoints\n\
                 WHERE workspace_root = ?1 AND label = ?2 ORDER BY created_at DESC LIMIT 1",
                params![ws, reference],
                map,
            )
            .optional()?)
    }

    /// The greatest "boundary" strictly below `cur` to step undo back to: either
    /// a checkpoint mark or a session-start mark (the change just before a
    /// session's first write). `0` when there's nothing earlier — i.e. base.
    pub fn previous_boundary(&self, workspace_root: &str, cur: i64) -> Result<i64> {
        let ws = self.normalize_workspace(workspace_root);
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(m), 0) FROM (\n\
                 SELECT mark AS m FROM checkpoints\n\
                     WHERE workspace_root = ?1 AND mark < ?2\n\
                 UNION\n\
                 SELECT MIN(fc.id) - 1 AS m FROM file_changes fc\n\
                     JOIN sessions s ON fc.session_id = s.id\n\
                     WHERE s.workspace_root = ?1 GROUP BY fc.session_id\n\
             ) WHERE m < ?2",
            params![ws, cur],
            |r| r.get(0),
        )?)
    }

    /// Log a restore move (undo/redo/restore) for the audit trail.
    pub fn record_restore(
        &self,
        workspace_root: &str,
        from_mark: i64,
        to_mark: i64,
        reason: &str,
        applied: i64,
    ) -> Result<()> {
        let ws = self.normalize_workspace(workspace_root);
        self.conn.execute(
            "INSERT INTO restores(workspace_root, from_mark, to_mark, reason, applied, created_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ws, from_mark, to_mark, reason, applied, now()],
        )?;
        Ok(())
    }

    /// Append a structured event (`note`, `session_status`, ...). `payload` is JSON.
    pub fn append_event(
        &self,
        session_id: &str,
        kind: &str,
        actor: &str,
        payload: &str,
    ) -> Result<i64> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO events(session_id, seq, kind, actor, payload, created_at)\n\
             VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE session_id = ?1), ?2, ?3, ?4, ?5)",
        )?;
        stmt.execute(params![session_id, kind, actor, payload, now()])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Count events of `kind` for a session (H1-1 redaction audit tests).
    pub fn event_count(&self, session_id: &str, kind: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND kind = ?2",
                params![session_id, kind],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// Mirror one egress decision into the local log (history for the Network
    /// Activity Panel and post-hoc privacy audit).
    #[allow(clippy::too_many_arguments)]
    pub fn record_egress(
        &self,
        session_id: Option<&str>,
        ts: &str,
        initiator: &str,
        host: &str,
        resolved_ip: Option<&str>,
        port: u16,
        decision: &str,
        matched_rule: Option<&str>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO egress_log(session_id, ts, initiator, host, resolved_ip, port, decision, matched_rule)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        stmt.execute(params![
            session_id,
            ts,
            initiator,
            host,
            resolved_ip,
            port as i64,
            decision,
            matched_rule
        ])?;
        Ok(())
    }

    /// List the most recent sessions (newest first) with rollup counts, for the
    /// `lokai sessions` history view.
    pub fn list_recent_sessions(&self, limit: u32) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.status, s.model, s.workspace_root, s.started_at,\n\
                    (SELECT COUNT(*) FROM messages   m WHERE m.session_id = s.id),\n\
                    (SELECT COUNT(*) FROM tool_calls  t WHERE t.session_id = s.id),\n\
                    (SELECT COUNT(*) FROM file_changes f WHERE f.session_id = s.id)\n\
             FROM sessions s ORDER BY s.started_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(SessionRow {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    model: r.get(2)?,
                    workspace_root: r.get(3)?,
                    started_at: r.get(4)?,
                    messages: r.get(5)?,
                    tool_calls: r.get(6)?,
                    file_changes: r.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The ordered transcript of a session: `(seq, role, content)`.
    pub fn transcript(&self, session_id: &str) -> Result<Vec<(i64, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, role, content FROM messages WHERE session_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map(params![session_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Count rows in a session (used by tests / the CLI's audit receipt).
    pub fn message_count(&self, session_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?)
    }

    /// Most recent session in a workspace that has at least one message (AR1-1 resume).
    pub fn find_latest_session_for_workspace(
        &self,
        workspace_root: &str,
    ) -> Result<Option<String>> {
        let ws = self.normalize_workspace(workspace_root);
        self.conn
            .query_row(
                "SELECT s.id FROM sessions s
                 WHERE s.workspace_root = ?1
                   AND EXISTS (SELECT 1 FROM messages m WHERE m.session_id = s.id)
                 ORDER BY s.started_at DESC LIMIT 1",
                params![ws],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Mark a prior session active again (resume path — no new session row).
    pub fn reopen_session(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET status = 'running', ended_at = NULL, error = '' WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Current session status (`running`, `ok`, `error`, `canceled`).
    pub fn session_status(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT status FROM sessions WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![session_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Persist in-flight turn operational state (AC2-6).
    pub fn upsert_turn_operation(
        &self,
        session_id: &str,
        turn_id: &str,
        state: &str,
        payload_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO turn_operations(session_id, turn_id, state, payload_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id) DO UPDATE SET
               turn_id = excluded.turn_id,
               state = excluded.state,
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
            params![session_id, turn_id, state, payload_json, now()],
        )?;
        Ok(())
    }

    /// Load operational turn row for crash recovery.
    pub fn get_turn_operation(&self, session_id: &str) -> Result<Option<TurnOperationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT turn_id, state, payload_json, updated_at FROM turn_operations WHERE session_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_id], |r| {
            Ok(TurnOperationRow {
                turn_id: r.get(0)?,
                state: r.get(1)?,
                payload_json: r.get(2)?,
                updated_at: r.get(3)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Clear operational row when a turn completes cleanly.
    pub fn clear_turn_operation(&self, session_id: &str, turn_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM turn_operations WHERE session_id = ?1 AND turn_id = ?2",
            params![session_id, turn_id],
        )?;
        Ok(())
    }

    /// Ordered audit messages for session resume (newest capped at `max` from the start).
    /// Excludes messages from spawn branches that were rolled back (SEC2-E2-012).
    pub fn list_messages_for_resume(
        &self,
        session_id: &str,
        max: u32,
    ) -> Result<Vec<StoredMessageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.role, m.content, m.tool_calls_json, m.tool_name, m.tool_call_id FROM messages m
             WHERE m.session_id = ?1
               AND NOT EXISTS (
                 SELECT 1 FROM spawn_rollbacks r
                 WHERE r.session_id = m.session_id AND r.agent_id = m.agent_id
               )
             ORDER BY m.seq DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(params![session_id, max], |r| {
                Ok(StoredMessageRow {
                    role: r.get(0)?,
                    content: r.get(1)?,
                    tool_calls_json: r.get(2)?,
                    tool_name: r.get(3)?,
                    tool_call_id: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Eligible resume-message count (same filter as `list_messages_for_resume`).
    pub fn count_messages_for_resume(&self, session_id: &str) -> Result<u32> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages m
             WHERE m.session_id = ?1
               AND NOT EXISTS (
                 SELECT 1 FROM spawn_rollbacks r
                 WHERE r.session_id = m.session_id AND r.agent_id = m.agent_id
               )",
            params![session_id],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// Mark a spawned agent branch as rolled back so resume omits its audit rows.
    pub fn record_spawn_rollback(&self, session_id: &str, agent_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO spawn_rollbacks(session_id, agent_id, rolled_at)
             VALUES (?1, ?2, ?3)",
            params![session_id, agent_id, now()],
        )?;
        Ok(())
    }
}

/// In-flight turn operational row (AC2-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOperationRow {
    pub turn_id: String,
    pub state: String,
    pub payload_json: String,
    pub updated_at: String,
}

/// One row from `messages` for rebuilding agent context (AR1-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessageRow {
    pub role: String,
    pub content: String,
    pub tool_calls_json: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_and_persist_roundtrip() {
        let store = Store::open(":memory:").expect("open in-memory db");
        let sid = store
            .start_session("/tmp/proj", "single-agent", "qwen3.5:latest")
            .unwrap();

        store
            .append_message(&sid, "system", "", "you are lokai", None)
            .unwrap();
        store
            .append_message(&sid, "user", "", "fix the bug", None)
            .unwrap();
        store
            .append_message(
                &sid,
                "assistant",
                "",
                "",
                Some("[{\"function\":{\"name\":\"read_file\"}}]"),
            )
            .unwrap();
        store
            .append_message(&sid, "tool", "", "file contents...", None)
            .unwrap();

        let tc_id = new_id("tc");
        store
            .record_tool_call(
                &tc_id,
                &sid,
                "read_file",
                "{\"path\":\"calc.py\"}",
                true,
                "read calc.py",
                None,
            )
            .unwrap();
        // A denied tool call is recorded with status `denied`, not `error`.
        store
            .record_tool_call(
                &new_id("tc"),
                &sid,
                "run_shell",
                "{\"command\":\"rm -rf /\"}",
                false,
                "not approved",
                Some("denied"),
            )
            .unwrap();
        store
            .append_event(
                &sid,
                "note",
                "system",
                "{\"text\":\"auto-compacted 2 messages\"}",
            )
            .unwrap();

        // A file change persists compressed before/after and round-trips.
        let edit_tc = new_id("tc");
        store
            .record_tool_call(
                &edit_tc,
                &sid,
                "edit_file",
                "{}",
                true,
                "edited calc.py",
                None,
            )
            .unwrap();
        store
            .record_file_change(
                &edit_tc,
                &sid,
                "calc.py",
                "edit",
                Some("return a - b"),
                Some("return a + b"),
            )
            .unwrap();
        let fc_id: i64 = store
            .conn
            .query_row(
                "SELECT id FROM file_changes WHERE tool_call_id = ?1",
                params![edit_tc],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            store.file_change_after(fc_id).unwrap().as_deref(),
            Some("return a + b")
        );
        store
            .record_egress(
                Some(&sid),
                &now(),
                "inference:ollama:chat",
                "localhost",
                Some("127.0.0.1"),
                11434,
                "allow",
                Some("builtin loopback"),
            )
            .unwrap();
        store.end_session(&sid, "ok", None).unwrap();

        assert_eq!(store.message_count(&sid).unwrap(), 4);

        // seq is monotonic and unique per session.
        let max_seq: i64 = store
            .conn
            .query_row(
                "SELECT MAX(seq) FROM messages WHERE session_id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(max_seq, 4);

        let status: String = store
            .conn
            .query_row(
                "SELECT status FROM sessions WHERE id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "ok");

        let denied: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE status = 'denied'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }

    #[test]
    fn zstd_rejects_oversized_file_change_input() {
        let store = Store::open(":memory:").unwrap();
        let sid = store.start_session("/tmp", "m", "m").unwrap();
        let tc = new_id("tc");
        let huge = "x".repeat(blob::MAX_BLOB_INPUT_BYTES + 1);
        let err = store
            .record_file_change(&tc, &sid, "big.txt", "edit", Some(&huge), None)
            .unwrap_err();
        assert!(err.to_string().contains("max input size"));
    }

    #[test]
    fn start_session_canonicalizes_workspace() {
        use crate::util::workspace_storage_key;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let store = Store::open(":memory:").unwrap();
        let sid = store
            .start_session(&dir.path().to_string_lossy(), "single-agent", "m")
            .unwrap();
        let stored = store.session_workspace_root(&sid).unwrap().unwrap();
        assert_eq!(stored, workspace_storage_key(dir.path()));
    }

    #[test]
    fn checkpoint_undo_redo_timeline() {
        let store = Store::open(":memory:").unwrap();
        let ws = "/tmp/proj";
        let sid = store.start_session(ws, "single-agent", "m").unwrap();

        // Two sequential edits to the same file: v0 -> v1 -> v2.
        let tc1 = new_id("tc");
        store
            .record_file_change(&tc1, &sid, "a.txt", "edit", Some("v0"), Some("v1"))
            .unwrap();
        // A named checkpoint after the first edit.
        let (ckpt, mark) = store
            .create_checkpoint(ws, "after-v1", "manual", Some(&sid))
            .unwrap();
        let tc2 = new_id("tc");
        store
            .record_file_change(&tc2, &sid, "a.txt", "edit", Some("v1"), Some("v2"))
            .unwrap();

        // Head implicitly at the tip (2 changes).
        let tip = store.workspace_max_mark(ws).unwrap();
        assert_eq!(store.current_head(ws).unwrap(), tip);
        assert_eq!(mark, tip - 1, "checkpoint sits between the two edits");

        // The checkpoint's mark is the boundary undo should fall back to first.
        assert_eq!(store.previous_boundary(ws, tip).unwrap(), mark);

        // Undo to the checkpoint: only the second edit is in range.
        let to_undo = store.workspace_changes_in_range(ws, mark, tip).unwrap();
        assert_eq!(to_undo.len(), 1);
        assert_eq!(to_undo[0].before.as_deref(), Some("v1"));
        store.set_head(ws, mark, Some(tip)).unwrap();
        assert_eq!(store.current_head(ws).unwrap(), mark);
        assert_eq!(store.head_state(ws).unwrap(), Some((mark, Some(tip))));

        // Redo forward: re-apply the second edit (after = "v2").
        let to_redo = store.workspace_changes_in_range(ws, mark, tip).unwrap();
        assert_eq!(to_redo[0].after.as_deref(), Some("v2"));
        store.set_head(ws, tip, None).unwrap();
        assert_eq!(store.head_state(ws).unwrap(), Some((tip, None)));

        // A new write while time-travelling advances the head and clears redo.
        store.set_head(ws, mark, Some(tip)).unwrap();
        let tc3 = new_id("tc");
        store
            .record_file_change(&tc3, &sid, "a.txt", "edit", Some("v2"), Some("v3"))
            .unwrap();
        let (h, r) = store.head_state(ws).unwrap().unwrap();
        assert_eq!(h, store.workspace_max_mark(ws).unwrap());
        assert_eq!(r, None, "new history invalidates redo");

        // Checkpoint resolves by id and by label; restore log records moves.
        assert!(store.find_checkpoint(ws, &ckpt).unwrap().is_some());
        assert_eq!(
            store.find_checkpoint(ws, "after-v1").unwrap().unwrap().mark,
            mark
        );
        store.record_restore(ws, tip, mark, "undo", 1).unwrap();
        let restores: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM restores WHERE workspace_root = ?1",
                params![ws],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(restores, 1);
    }

    #[test]
    fn resume_returns_newest_messages_in_chronological_order() {
        let store = Store::open(":memory:").unwrap();
        let sid = store
            .start_session("/tmp/ws", "single-agent", "mock")
            .unwrap();
        for i in 0..15 {
            store
                .append_message(&sid, "user", "", &format!("msg-{i}"), None)
                .unwrap();
        }
        let rows = store.list_messages_for_resume(&sid, 5).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].content, "msg-10");
        assert_eq!(rows[4].content, "msg-14");
    }

    #[test]
    fn resume_rows_include_tool_linkage() {
        let store = Store::open(":memory:").unwrap();
        let sid = store
            .start_session("/tmp/ws", "single-agent", "mock")
            .unwrap();
        store
            .append_message_with(
                &sid,
                "tool",
                "",
                "file contents",
                None,
                Some("read_file"),
                Some("tc_1"),
            )
            .unwrap();
        let rows = store.list_messages_for_resume(&sid, 5).unwrap();
        assert_eq!(rows[0].tool_name.as_deref(), Some("read_file"));
        assert_eq!(rows[0].tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(store.count_messages_for_resume(&sid).unwrap(), 1);
    }

    #[test]
    fn resume_messages_and_latest_session() {
        let store = Store::open(":memory:").unwrap();
        let ws = "/tmp/resume-ws";
        let sid = store.start_session(ws, "single-agent", "mock").unwrap();
        store
            .append_message(&sid, "system", "", "sys", None)
            .unwrap();
        store
            .append_message(&sid, "user", "", "hello", None)
            .unwrap();
        store.end_session(&sid, "ok", None).unwrap();

        assert_eq!(
            store
                .find_latest_session_for_workspace(ws)
                .unwrap()
                .as_deref(),
            Some(sid.as_str())
        );
        store.reopen_session(&sid).unwrap();
        let rows = store.list_messages_for_resume(&sid, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, "system");
        assert_eq!(rows[1].content, "hello");
    }

    #[test]
    fn resume_omits_rolled_back_spawn_agent() {
        let store = Store::open(":memory:").unwrap();
        let sid = store
            .start_session("/tmp/ws", "single-agent", "mock")
            .unwrap();
        store
            .append_message(&sid, "user", "a0", "root", None)
            .unwrap();
        store
            .append_message(&sid, "assistant", "a0_s0", "spawn child", None)
            .unwrap();
        store.record_spawn_rollback(&sid, "a0_s0").unwrap();
        let rows = store.list_messages_for_resume(&sid, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "root");
    }

    #[test]
    fn turn_operation_persist_and_clear() {
        let store = Store::open(":memory:").unwrap();
        let sid = store
            .start_session("/tmp/ws", "single-agent", "mock")
            .unwrap();
        store
            .upsert_turn_operation(&sid, "turn_1", "executing", r#"{"tool":"run_shell"}"#)
            .unwrap();
        let row = store.get_turn_operation(&sid).unwrap().unwrap();
        assert_eq!(row.state, "executing");
        assert_eq!(row.turn_id, "turn_1");
        store.clear_turn_operation(&sid, "turn_1").unwrap();
        assert!(store.get_turn_operation(&sid).unwrap().is_none());
    }

    /// R05: upgrading an existing on-disk DB creates a non-empty backup first.
    #[test]
    fn pre_migration_backup_created_before_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("lokai.db");
        {
            let store = Store::open(&db).expect("initial open");
            store
                .conn
                .execute_batch("DROP TABLE control_admin_events;
                    DELETE FROM schema_versions WHERE version = 32;")
                .unwrap();
        }
        let backups = pre_migrate_backup_directory(&db);
        assert!(!backups.exists());
        let _upgraded = Store::open(&db).expect("upgrade open");
        let bak = std::fs::read_dir(&backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(bak.is_file(), "backup file missing at {}", bak.display());
        assert!(
            std::fs::metadata(&bak).unwrap().len() > 0,
            "backup must be non-empty"
        );
    }

    /// R05: backup failure aborts migrate (fail-closed).
    #[test]
    fn pre_migration_backup_failure_aborts_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("lokai.db");
        {
            let store = Store::open(&db).expect("initial open");
            store
                .conn
                .execute_batch("DROP TABLE control_admin_events;
                    DELETE FROM schema_versions WHERE version = 32;")
                .unwrap();
        }
        let bak = pre_migrate_backup_directory(&db);
        std::fs::write(&bak, b"blocked backup directory").unwrap();
        let err = Store::open(&db);
        assert!(
            err.is_err(),
            "migrate must not continue when backup blocked"
        );
    }

    #[test]
    fn pre_migration_backup_missing_db_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.db");
        let err = pre_migration_backup(&missing);
        assert!(err.is_err());
    }
}

mod worker_tls;
pub use worker_tls::{WorkerTlsIdentity, WorkerTlsKey};
