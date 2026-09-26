//! Durable context identity. Scoped execution APIs are composed separately;
//! existing local history APIs may only consume explicitly legacy sessions.
use crate::{Result, Store, StoreError};

impl Store {
    pub(crate) fn migrate_context_scope_v35(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version>=35)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch("CREATE TABLE information_contexts (
            context_id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('legacy_local','private','team')),
            org_id TEXT REFERENCES organizations(org_id),
            owner_principal_id TEXT REFERENCES control_principals(principal_id),
            team_id TEXT,
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id),
            CHECK((kind='legacy_local' AND context_id='legacy-local' AND org_id IS NULL AND owner_principal_id IS NULL AND team_id IS NULL)
               OR (kind='private' AND org_id IS NOT NULL AND owner_principal_id IS NOT NULL AND team_id IS NULL)
               OR (kind='team' AND org_id IS NOT NULL AND team_id IS NOT NULL AND owner_principal_id IS NULL))
        );
        INSERT INTO information_contexts(context_id,kind) VALUES('legacy-local','legacy_local');
        ALTER TABLE sessions ADD COLUMN context_id TEXT NOT NULL DEFAULT 'legacy-local';
        CREATE TRIGGER session_context_exists BEFORE INSERT ON sessions
        WHEN NOT EXISTS(SELECT 1 FROM information_contexts WHERE context_id=NEW.context_id)
        BEGIN SELECT RAISE(ABORT,'unknown information context'); END;
        CREATE TRIGGER session_context_immutable BEFORE UPDATE OF context_id ON sessions
        WHEN NEW.context_id IS NOT OLD.context_id
        BEGIN SELECT RAISE(ABORT,'session context is immutable'); END;
        CREATE TRIGGER information_context_immutable BEFORE UPDATE ON information_contexts
        BEGIN SELECT RAISE(ABORT,'information context is immutable'); END;
        CREATE TRIGGER information_context_in_use BEFORE DELETE ON information_contexts
        WHEN OLD.context_id='legacy-local' OR EXISTS(SELECT 1 FROM sessions WHERE context_id=OLD.context_id)
        BEGIN SELECT RAISE(ABORT,'information context is in use'); END;
        CREATE INDEX idx_sessions_context ON sessions(context_id);")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(35,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Fail closed before a legacy caller can consume a scoped session by ID.
    /// Unknown IDs retain the same denial as inaccessible scoped IDs.
    pub fn require_legacy_session(&self, session: &str) -> Result<()> {
        let allowed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id='legacy-local')",
            [session],
            |r| r.get(0),
        )?;
        if !allowed {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }

    /// Legacy operator sessions and execution-audit histories can store tool
    /// evidence. A private or team discussion cannot.
    pub fn require_legacy_or_audit_session(&self, session: &str) -> Result<()> {
        let allowed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND (context_id='legacy-local' OR mode='execution-audit'))",
            [session],
            |row| row.get(0),
        )?;
        if !allowed {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn remove_context_schema_for_test(&self) {
        self.remove_run_capacity_schema_for_test();
        self.conn.execute_batch("DROP TABLE execution_grant_events; DROP TABLE execution_grants; DROP TABLE organization_agents; DROP TABLE agent_definition_revisions; DROP TABLE context_publications; DROP TABLE context_artifacts; DROP INDEX idx_messages_client_id; ALTER TABLE messages DROP COLUMN client_message_id; ALTER TABLE messages DROP COLUMN author_principal_id; DROP TRIGGER IF EXISTS project_memory_context_exists; DROP TRIGGER IF EXISTS project_memory_context_immutable; DROP TRIGGER IF EXISTS project_memory_blocks_context_delete; DROP INDEX IF EXISTS idx_project_memory_scope; ALTER TABLE project_memory DROP COLUMN context_id; DROP TRIGGER session_context_exists; DROP TRIGGER session_context_immutable; DROP TRIGGER information_context_immutable; DROP TRIGGER information_context_in_use; DROP INDEX idx_sessions_context; ALTER TABLE sessions DROP COLUMN context_id; DROP TABLE information_contexts;").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_readers_cannot_consume_private_history_or_derived_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = Store::open(dir.path().join("scope.db")).unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.conn
            .execute(
                "INSERT INTO information_contexts VALUES('private-a','private','org','alice',NULL)",
                [],
            )
            .unwrap();
        let workspace = db.normalize_workspace_path(dir.path());
        // Trusted fixture construction; there is deliberately no public scoped
        // activation API until all context consumers have been cut over.
        db.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES('private-session',?1,'test','test','ok','t','private-a')", [&workspace]).unwrap();
        db.append_message(
            "private-session",
            "user",
            "",
            "PRIVATECANARY health note",
            None,
        )
        .unwrap();
        assert!(db
            .record_tool_call(
                "finish-private",
                "private-session",
                "finish",
                "{}",
                true,
                "PRIVATECANARY summary",
                None,
            )
            .is_err());
        db.conn
            .execute(
                "INSERT INTO tool_calls(id, session_id, tool, args_json, status, result_summary, created_at, settled_at)
                 VALUES('finish-private','private-session','finish','{}','ok','PRIVATECANARY summary','t','t')",
                [],
            )
            .unwrap();
        assert!(db.session_workspace("private-session").unwrap().is_none());
        assert!(db
            .session_workspace_root("private-session")
            .unwrap()
            .is_none());
        assert!(db.session_status("private-session").unwrap().is_none());
        assert_eq!(db.message_count("private-session").unwrap(), 0);
        assert!(db
            .upsert_turn_operation("private-session", "turn", "executing", "{}")
            .is_err());
        assert!(db.transcript("private-session").is_err());
        assert!(db.list_messages_for_resume("private-session", 20).is_err());
        assert!(db.count_messages_for_resume("private-session").is_err());
        assert!(db.get_turn_operation("private-session").is_err());
        assert!(db.reopen_session("private-session").is_err());
        assert!(db.list_recent_sessions(20).unwrap().is_empty());
        assert!(db
            .find_latest_session_for_workspace(&workspace)
            .unwrap()
            .is_none());
        assert!(db
            .recall_history(dir.path(), "PRIVATECANARY", 20, None)
            .unwrap()
            .is_empty());
        assert!(db
            .recent_finish_outcomes(dir.path(), None, 5)
            .unwrap()
            .is_empty());
        assert!(db
            .record_file_change(
                "finish-private",
                "private-session",
                "secret/PRIVATECANARY.txt",
                "write",
                None,
                Some("PRIVATECANARY"),
            )
            .is_err());
        let private_blob = crate::blob::encode_blob("PRIVATECANARY").unwrap();
        db.conn
            .execute(
                "INSERT INTO file_changes(session_id, tool_call_id, path, change_kind, blob_encoding, before_blob, after_blob, applied_at)
                 VALUES('private-session','finish-private','secret/PRIVATECANARY.txt','write','zstd-full',NULL,?1,'t')",
                [private_blob],
            )
            .unwrap();
        let private_change: i64 = db
            .conn
            .query_row(
                "SELECT id FROM file_changes WHERE session_id='private-session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(db.session_file_changes("private-session").unwrap().is_empty());
        assert_eq!(
            db.session_file_change_highwater("private-session").unwrap(),
            0
        );
        assert!(db.file_change_after(private_change).unwrap().is_none());
        assert!(db
            .workspace_changes_in_range(&workspace, 0, i64::MAX)
            .unwrap()
            .is_empty());
        assert_eq!(db.workspace_max_mark(&workspace).unwrap(), 0);
        assert!(db
            .set_session_data_class("private-session", "circle_ok")
            .is_err());
        assert!(db.session_data_class("private-session").unwrap().is_none());
        assert_eq!(db.event_count("private-session", "note").unwrap(), 0);
        assert!(db
            .record_approval(
                "appr-private",
                "private-session",
                "run_shell",
                "PRIVATECANARY",
                "allow",
                true,
            )
            .is_err());
        db.conn
            .execute(
                "INSERT INTO approvals(id, session_id, kind, detail, decision, remembered, decided_at)
                 VALUES('appr-private','private-session','run_shell','PRIVATECANARY','allow',1,'t')",
                [],
            )
            .unwrap();
        assert!(db.get_approval("appr-private").unwrap().is_none());
        assert_eq!(db.approval_count("private-session").unwrap(), 0);
        assert!(!db
            .remember_session_approval_rule("private-session", "run_shell", "PRIVATECANARY")
            .unwrap());
        assert!(!db
            .approval_rule_matches("run_shell", "PRIVATECANARY")
            .unwrap());
        assert!(db
            .record_egress(
                Some("private-session"),
                "t",
                "agent",
                "secret.example",
                None,
                443,
                "deny",
                None,
            )
            .is_err());
        db.conn
            .execute(
                "INSERT INTO egress_log(session_id, ts, initiator, host, port, decision)
                 VALUES('private-session','t','agent','secret.example',443,'deny')",
                [],
            )
            .unwrap();
        assert_eq!(db.egress_count_for_session("private-session").unwrap(), 0);
        assert!(db
            .list_recent_sessions_for_workspace(dir.path(), "other", 5)
            .unwrap()
            .is_empty());
        assert!(db.recent_touched_paths(dir.path(), 12).unwrap().is_empty());
        let legacy = db.start_session(&workspace, "test", "test").unwrap();
        db.record_file_change(
            "finish-legacy",
            &legacy,
            "visible.txt",
            "write",
            None,
            Some("visible"),
        )
        .unwrap();
        assert_eq!(db.session_file_changes(&legacy).unwrap().len(), 1);
        assert_eq!(
            db.workspace_changes_in_range(&workspace, 0, i64::MAX)
                .unwrap()
                .len(),
            1
        );
        assert!(db.session_file_changes("private-session").unwrap().is_empty());
        assert!(db.consolidate_session_explicit("private-session").is_err());
        assert!(db.consolidate_session("private-session").is_err());
        assert!(db
            .conn
            .execute(
                "UPDATE sessions SET context_id='legacy-local' WHERE id='private-session'",
                []
            )
            .is_err());
        assert!(db
            .conn
            .execute(
                "UPDATE information_contexts SET kind='team' WHERE context_id='private-a'",
                []
            )
            .is_err());
        assert!(db
            .conn
            .execute(
                "DELETE FROM information_contexts WHERE context_id='private-a'",
                []
            )
            .is_err());
        assert!(db.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES('unknown','w','m','m','ok','t','missing')", []).is_err());
    }

    #[test]
    fn upgrading_preserves_history_in_explicit_legacy_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
        let id;
        {
            let db = Store::open(&path).unwrap();
            id = db.start_session("workspace", "test", "test").unwrap();
            db.append_message(&id, "user", "", "original history", None)
                .unwrap();
            db.remove_context_schema_for_test();
            db.conn
                .execute("DELETE FROM schema_versions WHERE version>=35", [])
                .unwrap();
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(db.transcript(&id).unwrap()[0].2, "original history");
        let scope: String = db
            .conn
            .query_row("SELECT context_id FROM sessions WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(scope, "legacy-local");
        let new = db.start_session("workspace", "test", "test").unwrap();
        db.require_legacy_session(&new).unwrap();
        drop(db);
        Store::open(&path)
            .unwrap()
            .require_legacy_session(&id)
            .unwrap();
    }
}
