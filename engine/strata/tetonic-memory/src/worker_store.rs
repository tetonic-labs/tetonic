//! Worker-local persistence (`worker.db`) — coordinator pins after enrollment.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use crate::capacity::RuntimeProfileRow;
use crate::capacity_tables::{
    CREATE_CAPACITY_BINDINGS, CREATE_RUNTIME_PROFILES, INSERT_RUNTIME_PROFILE,
    LIST_RUNTIME_PROFILES, LIST_RUNTIME_PROFILES_FOR_NODE, SELECT_CAPACITY_BINDING,
    SELECT_RUNTIME_PROFILE, UPSERT_CAPACITY_BINDING,
};
use crate::{now, StoreError};

#[derive(Debug, Error)]
pub enum WorkerStoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("worker TLS identity: {0}")]
    InvalidIdentity(&'static str),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type WorkerResult<T> = std::result::Result<T, WorkerStoreError>;

#[derive(Debug, Clone)]
pub struct CoordinatorPinRow {
    pub estate_id: String,
    pub coordinator_pubkey: Vec<u8>,
    pub label: String,
    pub enrolled_at: String,
    pub epoch: u64,
}

pub struct WorkerStore {
    pub(crate) conn: Connection,
    path: std::path::PathBuf,
}

impl WorkerStore {
    pub fn open(path: impl AsRef<Path>) -> WorkerResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=FULL;\n\
             PRAGMA fullfsync=ON;\n\
             PRAGMA checkpoint_fullfsync=ON;\n\
             PRAGMA secure_delete=ON;\n\
             PRAGMA foreign_keys=ON;\n\
             PRAGMA busy_timeout=5000;",
        )?;
        let store = Self { conn, path };
        let tx = store.conn.unchecked_transaction()?;
        store.migrate()?;
        tx.commit()?;
        Ok(store)
    }

    pub fn db_path(&self) -> WorkerResult<&Path> {
        Ok(&self.path)
    }

    fn migrate(&self) -> WorkerResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_versions (\n\
                 version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);\n\
             CREATE TABLE IF NOT EXISTS coordinator_pins (\n\
                 estate_id           TEXT NOT NULL,\n\
                 coordinator_pubkey  BLOB NOT NULL,\n\
                 label               TEXT NOT NULL,\n\
                 enrolled_at         TEXT NOT NULL,\n\
                 epoch               INTEGER NOT NULL DEFAULT 0,\n\
                 PRIMARY KEY (estate_id, coordinator_pubkey)\n\
             );",
        )?;
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied > 7 {
            return Err(WorkerStoreError::InvalidIdentity(
                "worker schema is newer than this binary",
            ));
        }
        if applied < 1 {
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (1, ?1)",
                params![now()],
            )?;
        }
        if applied < 2 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS worker_tls (\n\
                     id INTEGER PRIMARY KEY CHECK (id = 1),\n\
                     cert_der BLOB NOT NULL,\n\
                     key_der  BLOB NOT NULL\n\
                 );\n\
                 CREATE TABLE IF NOT EXISTS ingress_log (\n\
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,\n\
                     ts          TEXT NOT NULL,\n\
                     peer_id     TEXT,\n\
                     remote_addr TEXT NOT NULL,\n\
                     decision    TEXT NOT NULL,\n\
                     reason      TEXT NOT NULL,\n\
                     route       TEXT\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (2, ?1)",
                params![now()],
            )?;
        }
        if applied < 3 {
            self.conn.execute_batch(&format!(
                "{CREATE_RUNTIME_PROFILES}\n{CREATE_CAPACITY_BINDINGS}"
            ))?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (3, ?1)",
                params![now()],
            )?;
        }
        if applied < 4 {
            // ingress_log was never wired; in-memory IngressLog covers worker audit (AR1-4).
            self.conn
                .execute_batch("DROP TABLE IF EXISTS ingress_log;")?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (4, ?1)",
                params![now()],
            )?;
        }
        if applied < 5 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS ingress_log (\n\
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,\n\
                     ts          TEXT NOT NULL,\n\
                     peer_id     TEXT,\n\
                     remote_addr TEXT NOT NULL,\n\
                     decision    TEXT NOT NULL,\n\
                     reason      TEXT NOT NULL,\n\
                     route       TEXT\n\
                 );\n\
                 CREATE INDEX IF NOT EXISTS idx_ingress_log_ts ON ingress_log(ts);",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (5, ?1)",
                params![now()],
            )?;
        }
        if applied < 6 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS fabric_ingress_records (\n\
                     delivery_key TEXT PRIMARY KEY,\n\
                     record_json  TEXT NOT NULL\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (6, ?1)",
                params![now()],
            )?;
        }
        if applied < 7 {
            self.conn.execute_batch(
                "ALTER TABLE worker_tls ADD COLUMN key_ref TEXT;
                 ALTER TABLE worker_tls ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE worker_tls ADD COLUMN revoked INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE worker_tls ADD COLUMN scrub_pending INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (7, ?1)",
                params![now()],
            )?;
        }
        Ok(())
    }

    /// Load persisted fabric job ingress records (M5-1 worker dedup).
    pub fn load_fabric_ingress_records(&self) -> WorkerResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT record_json FROM fabric_ingress_records")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<Result<Vec<String>, _>>()
            .map_err(WorkerStoreError::from)
    }

    /// Upsert one fabric ingress record.
    pub fn upsert_fabric_ingress_record(
        &self,
        delivery_key: &str,
        record_json: &str,
    ) -> WorkerResult<()> {
        self.conn.execute(
            "INSERT INTO fabric_ingress_records(delivery_key, record_json) VALUES (?1, ?2)
             ON CONFLICT(delivery_key) DO UPDATE SET record_json = excluded.record_json",
            params![delivery_key, record_json],
        )?;
        Ok(())
    }

    /// Append one ingress audit row (fabric listener).
    pub fn append_ingress_event(
        &self,
        ts: &str,
        peer_id: Option<&str>,
        remote_addr: &str,
        decision: &str,
        reason: &str,
        route: Option<&str>,
    ) -> WorkerResult<()> {
        self.conn.execute(
            "INSERT INTO ingress_log(ts, peer_id, remote_addr, decision, reason, route)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ts, peer_id, remote_addr, decision, reason, route],
        )?;
        Ok(())
    }

    /// Drop oldest rows beyond `keep` (newest retained).
    pub fn trim_ingress_log(&self, keep: i64) -> WorkerResult<()> {
        self.conn.execute(
            "DELETE FROM ingress_log WHERE id NOT IN (
                 SELECT id FROM ingress_log ORDER BY id DESC LIMIT ?1
             )",
            params![keep],
        )?;
        Ok(())
    }

    pub fn ingress_event_count(&self) -> WorkerResult<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM ingress_log", [], |r| r.get(0))?)
    }

    pub fn upsert_coordinator_pin(
        &self,
        estate_id: &str,
        coordinator_pubkey: &[u8],
        label: &str,
    ) -> WorkerResult<()> {
        self.conn.execute(
            "INSERT INTO coordinator_pins(estate_id, coordinator_pubkey, label, enrolled_at, epoch)\n\
             VALUES (?1, ?2, ?3, ?4, 0)\n\
             ON CONFLICT(estate_id, coordinator_pubkey) DO UPDATE SET\n\
                 label=excluded.label, enrolled_at=excluded.enrolled_at",
            params![estate_id, coordinator_pubkey, label, now()],
        )?;
        Ok(())
    }

    pub fn list_coordinator_pins(&self) -> WorkerResult<Vec<CoordinatorPinRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT estate_id, coordinator_pubkey, label, enrolled_at, epoch FROM coordinator_pins",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(CoordinatorPinRow {
                    estate_id: r.get(0)?,
                    coordinator_pubkey: r.get(1)?,
                    label: r.get(2)?,
                    enrolled_at: r.get(3)?,
                    epoch: r.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn remove_coordinator_pin(
        &self,
        estate_id: &str,
        coordinator_pubkey: &[u8],
    ) -> WorkerResult<()> {
        self.conn.execute(
            "DELETE FROM coordinator_pins WHERE estate_id = ?1 AND coordinator_pubkey = ?2",
            params![estate_id, coordinator_pubkey],
        )?;
        Ok(())
    }

    pub fn bump_coordinator_epoch(
        &self,
        estate_id: &str,
        coordinator_pubkey: &[u8],
        epoch: u64,
    ) -> WorkerResult<()> {
        self.conn.execute(
            "UPDATE coordinator_pins SET epoch = MAX(epoch, ?3) WHERE estate_id = ?1 AND coordinator_pubkey = ?2",
            params![estate_id, coordinator_pubkey, epoch as i64],
        )?;
        Ok(())
    }

    pub fn coordinator_pin_epoch(
        &self,
        estate_id: &str,
        coordinator_pubkey: &[u8],
    ) -> WorkerResult<Option<u64>> {
        self.conn
            .query_row(
                "SELECT epoch FROM coordinator_pins WHERE estate_id = ?1 AND coordinator_pubkey = ?2",
                params![estate_id, coordinator_pubkey],
                |r| r.get::<_, i64>(0).map(|e| e as u64),
            )
            .optional()
            .map_err(Into::into)
    }

    // ---- capacity profiles (ES5-4, worker-local) ----------------------------

    pub fn insert_runtime_profile(&self, row: &RuntimeProfileRow) -> WorkerResult<()> {
        self.conn.execute(
            INSERT_RUNTIME_PROFILE,
            params![
                row.id,
                row.node_id,
                row.role,
                row.fingerprint,
                row.created_at,
                i32::from(row.gates_passed),
                row.json,
            ],
        )?;
        Ok(())
    }

    pub fn get_runtime_profile(&self, id: &str) -> WorkerResult<Option<RuntimeProfileRow>> {
        self.conn
            .query_row(SELECT_RUNTIME_PROFILE, params![id], |r| {
                Ok(RuntimeProfileRow {
                    id: r.get(0)?,
                    node_id: r.get(1)?,
                    role: r.get(2)?,
                    fingerprint: r.get(3)?,
                    created_at: r.get(4)?,
                    gates_passed: r.get::<_, i32>(5)? != 0,
                    json: r.get(6)?,
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn list_runtime_profiles(
        &self,
        node_id: Option<&str>,
    ) -> WorkerResult<Vec<RuntimeProfileRow>> {
        let mut out = Vec::new();
        match node_id {
            Some(nid) => {
                let mut stmt = self.conn.prepare(LIST_RUNTIME_PROFILES_FOR_NODE)?;
                let rows = stmt.query_map(params![nid], worker_profile_row)?;
                for r in rows {
                    out.push(r?);
                }
            }
            None => {
                let mut stmt = self.conn.prepare(LIST_RUNTIME_PROFILES)?;
                let rows = stmt.query_map([], worker_profile_row)?;
                for r in rows {
                    out.push(r?);
                }
            }
        }
        Ok(out)
    }

    pub fn set_capacity_binding(
        &self,
        node_id: &str,
        role: &str,
        profile_id: Option<&str>,
    ) -> WorkerResult<()> {
        self.conn.execute(
            UPSERT_CAPACITY_BINDING,
            params![node_id, role, profile_id, now()],
        )?;
        Ok(())
    }

    pub fn get_capacity_binding(&self, node_id: &str, role: &str) -> WorkerResult<Option<String>> {
        self.conn
            .query_row(SELECT_CAPACITY_BINDING, params![node_id, role], |r| {
                r.get(0)
            })
            .optional()
            .map_err(Into::into)
    }
}

fn worker_profile_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeProfileRow> {
    Ok(RuntimeProfileRow {
        id: r.get(0)?,
        node_id: r.get(1)?,
        role: r.get(2)?,
        fingerprint: r.get(3)?,
        created_at: r.get(4)?,
        gates_passed: r.get::<_, i32>(5)? != 0,
        json: r.get(6)?,
    })
}

impl From<WorkerStoreError> for StoreError {
    fn from(e: WorkerStoreError) -> Self {
        match e {
            WorkerStoreError::Sqlite(e) => StoreError::Sqlite(e),
            WorkerStoreError::Io(e) => StoreError::Io(e),
            other => StoreError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_v4_drops_legacy_ingress_then_v5_restores() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("worker.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_versions (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
                 INSERT INTO schema_versions(version, applied_at) VALUES (2, 'test');
                 CREATE TABLE worker_tls (id INTEGER PRIMARY KEY, cert_der BLOB NOT NULL, key_der BLOB NOT NULL);
                 CREATE TABLE ingress_log (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     ts TEXT NOT NULL,
                     peer_id TEXT,
                     remote_addr TEXT NOT NULL,
                     decision TEXT NOT NULL,
                     reason TEXT NOT NULL,
                     route TEXT
                 );",
            )
            .unwrap();
        }

        let store = WorkerStore::open(&db).unwrap();
        drop(store);
        let conn = Connection::open(&db).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 7);

        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='ingress_log'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "v5 must recreate ingress_log after v4 drop");

        let fabric_ingress: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fabric_ingress_records'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fabric_ingress, 1, "v6 must create fabric_ingress_records");
    }

    #[test]
    fn migration_v5_restores_ingress_log() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("worker.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_versions (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
                 INSERT INTO schema_versions(version, applied_at) VALUES (4, 'test');
                 CREATE TABLE worker_tls (id INTEGER PRIMARY KEY, cert_der BLOB NOT NULL, key_der BLOB NOT NULL);",
            )
            .unwrap();
        }
        let store = WorkerStore::open(&db).unwrap();
        store
            .append_ingress_event(
                "t",
                Some("peer"),
                "127.0.0.1:1",
                "allow",
                "authorized",
                Some("/v1/health"),
            )
            .unwrap();
        assert_eq!(store.ingress_event_count().unwrap(), 1);
    }
}
