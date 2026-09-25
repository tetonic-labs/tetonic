//! Schema migrations for `lokai.db` (v1–v36).

use rusqlite::params;

use crate::util::{now, workspace_storage_key_str};
use crate::{Result, Store};

impl Store {
    pub(crate) fn migrate(&self) -> Result<()> {
        // Serialize cooperating upgrade/backup callers, including the interval
        // before SQLite can begin the migration transaction (VACUUM cannot run
        // inside that transaction). SQLite still owns schema/write atomicity.
        let _owner = crate::backup::upgrade_lock(&self.conn)?;
        let applied = crate::backup::schema_version(&self.conn)?;
        if applied > crate::SCHEMA_TARGET_VERSION {
            return Err(crate::StoreError::FutureSchema {
                found: applied,
                supported: crate::SCHEMA_TARGET_VERSION,
            });
        }
        if applied == crate::SCHEMA_TARGET_VERSION {
            return Ok(());
        }
        if applied > 0 && self.conn.path().is_some_and(|p| !p.is_empty()) {
            crate::backup::backup_connection(&self.conn, applied)?;
        }
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        // Re-read under SQLite write ownership. An older/noncooperating client
        // must not silently change the version between backup and upgrade.
        if crate::backup::schema_version(&self.conn)? != applied {
            return Err(crate::StoreError::Io(std::io::Error::other(
                "database schema changed during migration preparation; retry open",
            )));
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_versions (
                version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL
            );",
        )?;
        self.apply_schema_upgrades(applied)?;
        transaction.commit()?;
        Ok(())
    }

    fn apply_schema_upgrades(&self, applied: i64) -> Result<()> {
        if applied < 1 {
            self.conn.execute_batch(
                "CREATE TABLE sessions (\n\
                     id             TEXT PRIMARY KEY,\n\
                     workspace_root TEXT NOT NULL,\n\
                     mode           TEXT NOT NULL,\n\
                     model          TEXT NOT NULL,\n\
                     status         TEXT NOT NULL,\n\
                     started_at     TEXT NOT NULL,\n\
                     ended_at       TEXT,\n\
                     error          TEXT NOT NULL DEFAULT ''\n\
                 );\n\
                 CREATE TABLE messages (\n\
                     id              INTEGER PRIMARY KEY,\n\
                     session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     seq             INTEGER NOT NULL,\n\
                     role            TEXT NOT NULL,\n\
                     agent_id        TEXT NOT NULL DEFAULT '',\n\
                     content         TEXT NOT NULL,\n\
                     tool_calls_json TEXT,\n\
                     created_at      TEXT NOT NULL,\n\
                     UNIQUE(session_id, seq)\n\
                 );\n\
                 CREATE TABLE tool_calls (\n\
                     id             TEXT PRIMARY KEY,\n\
                     session_id     TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     tool           TEXT NOT NULL,\n\
                     args_json      TEXT NOT NULL,\n\
                     status         TEXT NOT NULL,\n\
                     result_summary TEXT NOT NULL DEFAULT '',\n\
                     error_kind     TEXT,\n\
                     created_at     TEXT NOT NULL,\n\
                     settled_at     TEXT\n\
                 );\n\
                 CREATE TABLE events (\n\
                     id         INTEGER PRIMARY KEY,\n\
                     session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     seq        INTEGER NOT NULL,\n\
                     kind       TEXT NOT NULL,\n\
                     actor      TEXT NOT NULL,\n\
                     payload    TEXT NOT NULL DEFAULT '{}',\n\
                     created_at TEXT NOT NULL,\n\
                     UNIQUE(session_id, seq)\n\
                 );\n\
                 CREATE TABLE egress_log (\n\
                     id           INTEGER PRIMARY KEY,\n\
                     session_id   TEXT,\n\
                     ts           TEXT NOT NULL,\n\
                     initiator    TEXT NOT NULL,\n\
                     host         TEXT NOT NULL,\n\
                     resolved_ip  TEXT,\n\
                     port         INTEGER NOT NULL,\n\
                     decision     TEXT NOT NULL,\n\
                     matched_rule TEXT\n\
                 );\n\
                 CREATE INDEX idx_messages_session_seq   ON messages(session_id, seq);\n\
                 CREATE INDEX idx_events_session_seq      ON events(session_id, seq);\n\
                 CREATE INDEX idx_tool_calls_session      ON tool_calls(session_id);\n\
                 CREATE INDEX idx_egress_session          ON egress_log(session_id);",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (1, ?1)",
                params![now()],
            )?;
        }

        if applied < 2 {
            self.conn.execute_batch(
                "CREATE TABLE file_changes (\n\
                     id            INTEGER PRIMARY KEY,\n\
                     session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     tool_call_id  TEXT NOT NULL,\n\
                     path          TEXT NOT NULL,\n\
                     change_kind   TEXT NOT NULL,\n\
                     blob_encoding TEXT NOT NULL,\n\
                     before_blob   BLOB,\n\
                     after_blob    BLOB,\n\
                     applied_at    TEXT NOT NULL\n\
                 );\n\
                 CREATE INDEX idx_file_changes_session ON file_changes(session_id);",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (2, ?1)",
                params![now()],
            )?;
        }

        if applied < 3 {
            self.conn.execute_batch(
                "CREATE TABLE checkpoints (\n\
                     id             TEXT PRIMARY KEY,\n\
                     workspace_root TEXT NOT NULL,\n\
                     label          TEXT NOT NULL,\n\
                     mark           INTEGER NOT NULL,\n\
                     kind           TEXT NOT NULL DEFAULT 'manual',\n\
                     session_id     TEXT,\n\
                     created_at     TEXT NOT NULL\n\
                 );\n\
                 CREATE INDEX idx_checkpoints_ws ON checkpoints(workspace_root, mark);\n\
                 CREATE TABLE workspace_head (\n\
                     workspace_root TEXT PRIMARY KEY,\n\
                     head_mark      INTEGER NOT NULL,\n\
                     redo_mark      INTEGER,\n\
                     updated_at     TEXT NOT NULL\n\
                 );\n\
                 CREATE TABLE restores (\n\
                     id             INTEGER PRIMARY KEY,\n\
                     workspace_root TEXT NOT NULL,\n\
                     from_mark      INTEGER NOT NULL,\n\
                     to_mark        INTEGER NOT NULL,\n\
                     reason         TEXT NOT NULL,\n\
                     applied        INTEGER NOT NULL,\n\
                     created_at     TEXT NOT NULL\n\
                 );\n\
                 CREATE INDEX idx_restores_ws ON restores(workspace_root, id);",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (3, ?1)",
                params![now()],
            )?;
        }
        self.migrate_estate_v4()?;
        self.migrate_estate_v5()?;
        self.migrate_policy_v6()?;
        self.migrate_projects_v7()?;
        self.migrate_recall_v8()?;
        self.migrate_trust_v9()?;
        self.migrate_capacity_v10()?;
        self.migrate_capacity_v11()?;
        self.migrate_spawn_v12()?;
        self.migrate_turn_ops_v13()?;
        self.migrate_workspace_v14()?;
        self.migrate_classification_v15()?;
        self.migrate_runs_v16()?;
        self.migrate_runs_v17()?;
        self.migrate_runs_v18()?;
        self.migrate_estate_v19()?;
        self.migrate_result_disposition_v20()?;
        self.migrate_compute_reservations_v21()?;
        self.migrate_scheduler_decisions_v22()?;
        self.migrate_message_tool_linkage_v23()?;
        self.migrate_capabilities_v24()?;
        self.migrate_secret_overrides_v25()?;
        self.migrate_run_replay_floor_v26()?;
        self.migrate_run_session_nullable_v27()?;
        self.migrate_agent_identities_v28()?;
        self.migrate_team_resources_v29()?;
        self.migrate_memberships_v30()?;
        self.migrate_control_credentials_v31()?;
        self.migrate_control_bootstrap_v32()?;
        self.migrate_team_admin_v33()?;
        self.migrate_identity_revisions_v34()?;
        self.migrate_context_scope_v35()?;
        self.migrate_context_messages_v36()?;
        self.migrate_context_artifacts_v37()?;
        Ok(())
    }

    fn migrate_run_replay_floor_v26(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 26 {
            return Ok(());
        }
        let has_floor: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('run_projections') WHERE name = 'replay_floor'",
            [],
            |r| r.get(0),
        )?;
        if has_floor == 0 {
            self.conn.execute_batch(
                "ALTER TABLE run_projections ADD COLUMN replay_floor INTEGER NOT NULL DEFAULT 0;\n\
                 ALTER TABLE run_projections ADD COLUMN recovery_snapshot_json TEXT;",
            )?;
        }
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (26, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_run_session_nullable_v27(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 27 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE run_projections_v27 (
                 run_id TEXT PRIMARY KEY,
                 session_id TEXT,
                 sequence INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 projection_json TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 replay_floor INTEGER NOT NULL DEFAULT 0,
                 recovery_snapshot_json TEXT
             );
             INSERT INTO run_projections_v27 (
                 run_id, session_id, sequence, state, projection_json, updated_at,
                 replay_floor, recovery_snapshot_json
             )
             SELECT run_id, session_id, sequence, state, projection_json, updated_at,
                    COALESCE(replay_floor, 0), recovery_snapshot_json
             FROM run_projections;
             DROP TABLE run_projections;
             ALTER TABLE run_projections_v27 RENAME TO run_projections;
             CREATE INDEX IF NOT EXISTS idx_run_projections_session ON run_projections(session_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (27, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_agent_identities_v28(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 28 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_identities (\n\
                 identity_id TEXT PRIMARY KEY,\n\
                 owning_application TEXT NOT NULL,\n\
                 bound_definition_digest TEXT NOT NULL,\n\
                 privilege_class TEXT NOT NULL,\n\
                 toolset_subscriptions_json TEXT NOT NULL,\n\
                 context_bindings_json TEXT NOT NULL,\n\
                 recovery_id TEXT NOT NULL,\n\
                 created_at TEXT NOT NULL,\n\
                 updated_at TEXT NOT NULL\n\
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (28, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_message_tool_linkage_v23(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 23 {
            return Ok(());
        }
        let has_col: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'tool_name'",
            [],
            |r| r.get(0),
        )?;
        if has_col == 0 {
            self.conn.execute_batch(
                "ALTER TABLE messages ADD COLUMN tool_name TEXT;\n\
                 ALTER TABLE messages ADD COLUMN tool_call_id TEXT;",
            )?;
        }
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (23, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_runs_v18(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 18 {
            return Ok(());
        }

        // Backup before major migration? The ticket says "Create a backup before a major migration"
        // We will do a simple atomic transaction here, which in SQLite provides backup-like safety if it rolls back.
        // For actual safe mode, the caller handles it if migration fails.
        self.conn.execute_batch(
            "ALTER TABLE run_events RENAME TO legacy_run_events;
             CREATE TABLE IF NOT EXISTS run_events (
                 event_id       TEXT PRIMARY KEY,
                 run_id         TEXT NOT NULL,
                 sequence       INTEGER NOT NULL,
                 event_type     TEXT NOT NULL,
                 schema_version INTEGER NOT NULL,
                 command_id     TEXT,
                 causation_id   TEXT,
                 correlation_id TEXT,
                 actor_json     TEXT NOT NULL,
                 occurred_at    TEXT NOT NULL,
                 recorded_at    TEXT NOT NULL,
                 data_class     TEXT NOT NULL,
                 payload_digest TEXT NOT NULL,
                 payload_json   TEXT NOT NULL,
                 UNIQUE (run_id, sequence)
             );
             CREATE INDEX IF NOT EXISTS idx_run_events_run ON run_events(run_id, sequence);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (18, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_runs_v17(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 17 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS run_command_dedup (
                 run_id       TEXT NOT NULL,
                 command_id   TEXT NOT NULL,
                 result_json  TEXT NOT NULL,
                 created_at   TEXT NOT NULL,
                 PRIMARY KEY (run_id, command_id)
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (17, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_runs_v16(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 16 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS run_projections (
                 run_id           TEXT PRIMARY KEY,
                 session_id       TEXT NOT NULL,
                 sequence         INTEGER NOT NULL,
                 state            TEXT NOT NULL,
                 projection_json  TEXT NOT NULL,
                 updated_at       TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS run_events (
                 run_id       TEXT NOT NULL,
                 sequence     INTEGER NOT NULL,
                 event_type   TEXT NOT NULL,
                 command_id   TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at   TEXT NOT NULL,
                 PRIMARY KEY (run_id, sequence)
             );
             CREATE TABLE IF NOT EXISTS run_idempotency (
                 run_id          TEXT NOT NULL,
                 idempotency_key TEXT NOT NULL,
                 result_json     TEXT NOT NULL,
                 PRIMARY KEY (run_id, idempotency_key)
             );
             CREATE INDEX IF NOT EXISTS idx_run_events_run ON run_events(run_id, sequence);
             CREATE INDEX IF NOT EXISTS idx_run_projections_session ON run_projections(session_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (16, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_classification_v15(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 15 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS classification_audit (
                 id              INTEGER PRIMARY KEY,
                 session_id      TEXT NOT NULL,
                 previous_class  TEXT,
                 new_class       TEXT NOT NULL,
                 reason          TEXT NOT NULL,
                 source          TEXT NOT NULL,
                 recorded_at     TEXT NOT NULL
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (15, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn migrate_spawn_v12(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied < 12 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS spawn_rollbacks (\n\
                     session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     agent_id   TEXT NOT NULL,\n\
                     rolled_at  TEXT NOT NULL,\n\
                     PRIMARY KEY (session_id, agent_id)\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (12, ?1)",
                params![now()],
            )?;
        }
        Ok(())
    }

    fn migrate_turn_ops_v13(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied < 13 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS turn_operations (\n\
                     session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,\n\
                     turn_id      TEXT NOT NULL,\n\
                     state        TEXT NOT NULL,\n\
                     payload_json TEXT NOT NULL DEFAULT '{}',\n\
                     updated_at   TEXT NOT NULL,\n\
                     PRIMARY KEY (session_id)\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (13, ?1)",
                params![now()],
            )?;
        }
        Ok(())
    }

    /// Normalize workspace keys across sessions, timeline, projects, and recall FTS.
    fn migrate_workspace_v14(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 14 {
            return Ok(());
        }

        let mut roots: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT workspace_root FROM sessions
                 UNION SELECT workspace_root FROM checkpoints
                 UNION SELECT workspace_root FROM workspace_head
                 UNION SELECT workspace_root FROM restores
                 UNION SELECT root FROM projects",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };

        roots.sort();
        roots.dedup();

        for old in roots {
            let new = workspace_storage_key_str(&old);
            if new == old {
                continue;
            }
            self.remap_workspace_root(&old, &new)?;
        }

        self.conn.execute("DELETE FROM recall_fts", [])?;
        self.backfill_recall_fts()?;

        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (14, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn remap_workspace_root(&self, old: &str, new: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET workspace_root = ?1 WHERE workspace_root = ?2",
            params![new, old],
        )?;
        self.conn.execute(
            "UPDATE checkpoints SET workspace_root = ?1 WHERE workspace_root = ?2",
            params![new, old],
        )?;
        self.conn.execute(
            "UPDATE restores SET workspace_root = ?1 WHERE workspace_root = ?2",
            params![new, old],
        )?;
        self.conn.execute(
            "UPDATE projects SET root = ?1 WHERE root = ?2",
            params![new, old],
        )?;

        let head_exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workspace_head WHERE workspace_root = ?1",
            params![new],
            |r| r.get(0),
        )?;
        if head_exists > 0 {
            self.conn.execute(
                "DELETE FROM workspace_head WHERE workspace_root = ?1",
                params![old],
            )?;
        } else {
            self.conn.execute(
                "UPDATE workspace_head SET workspace_root = ?1 WHERE workspace_root = ?2",
                params![new, old],
            )?;
        }
        Ok(())
    }

    /// Normalize a workspace path/string to the storage key used in SQL.
    pub fn normalize_workspace(&self, workspace_root: &str) -> String {
        workspace_storage_key_str(workspace_root)
    }

    pub fn normalize_workspace_path(&self, workspace_root: &std::path::Path) -> String {
        crate::util::workspace_storage_key(workspace_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migration_v14_unifies_workspace_aliases() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("lokai.db");
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let canonical = crate::util::workspace_storage_key(&ws);
        let alias = ws.to_string_lossy().to_string();
        if alias == canonical {
            return;
        }

        {
            let store = Store::open(&db).unwrap();
            store.remove_context_schema_for_test();
            store
                .conn
                .execute_batch("DROP TABLE agent_identity_revisions; DROP TABLE control_admin_events; DELETE FROM schema_versions WHERE version >= 14;")
                .unwrap();
            store
                .conn
                .execute("DROP TABLE IF EXISTS legacy_run_events", [])
                .unwrap();
            store
                .conn
                .execute("DROP TABLE IF EXISTS run_events", [])
                .unwrap();
            // Recreate the old run_events table so the migration can rename it
            store
                .conn
                .execute("CREATE TABLE run_events (run_id TEXT, sequence INTEGER, PRIMARY KEY(run_id, sequence))", [])
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO sessions(id, workspace_root, mode, model, status, started_at)
                     VALUES ('sess_alias', ?1, 'm', 'm', 'ok', 't')",
                    params![alias],
                )
                .unwrap();
        }

        let store = Store::open(&db).unwrap();
        let stored: String = store
            .conn
            .query_row(
                "SELECT workspace_root FROM sessions WHERE id = 'sess_alias'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, canonical);

        let version: i64 = store
            .conn
            .query_row("SELECT MAX(version) FROM schema_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::SCHEMA_TARGET_VERSION);
    }

    #[test]
    fn fresh_open_is_schema_target_version() {
        let store = Store::open(":memory:").unwrap();
        let version: i64 = store
            .conn
            .query_row("SELECT MAX(version) FROM schema_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::SCHEMA_TARGET_VERSION);
    }
}
