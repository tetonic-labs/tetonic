use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

impl Store {
    /// Trusted execution host: create a fresh audit history in an authorized
    /// information context. This record is not a live session or run status.
    pub fn create_execution_audit_history(
        &self,
        actor: &str,
        context: &str,
        session: &str,
    ) -> Result<()> {
        validate_id(session)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
            [session],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES(?1,'','execution-audit','','audit',?2,?3)", params![session,crate::util::now(),context])?;
        tx.commit()?;
        Ok(())
    }

    /// Immutable content boundary for trusted audit writes. Recording terminal
    /// evidence after revocation must remain possible; readers check membership.
    pub fn require_execution_audit_history(&self, context: &str, session: &str) -> Result<()> {
        let valid = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id=?2 AND mode='execution-audit')", params![session,context], |r| r.get::<_, bool>(0))?;
        if valid {
            Ok(())
        } else {
            Err(StoreError::ControlAccessDenied)
        }
    }

    /// Status of a discussion in this context. Missing and foreign ids are both
    /// `None` for an authorized principal; other principals are denied.
    pub fn context_discussion_status(
        &self,
        actor: &str,
        context: &str,
        session: &str,
    ) -> Result<Option<String>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let status = self
            .conn
            .query_row(
                "SELECT status FROM sessions WHERE id=?1 AND context_id=?2 AND mode='discussion'",
                params![session, context],
                |r| r.get(0),
            )
            .optional()?;
        tx.commit()?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(status)
    }

    /// Close a discussion without deleting its history or stopping any execution.
    pub fn close_context_history(&self, actor: &str, context: &str, session: &str) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        let status: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM sessions WHERE id=?1 AND context_id=?2 AND mode='discussion'",
                params![session, context],
                |r| r.get(0),
            )
            .optional()?;
        match status.as_deref() {
            Some("open") => {
                self.conn.execute(
                    "UPDATE sessions SET status='closed',ended_at=?2 WHERE id=?1",
                    params![session, crate::util::now()],
                )?;
            }
            Some("closed") => {}
            _ => return Err(StoreError::ControlAccessDenied),
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn migrate_context_messages_v36(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version>=36)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch("ALTER TABLE messages ADD COLUMN author_principal_id TEXT REFERENCES control_principals(principal_id);
        ALTER TABLE messages ADD COLUMN client_message_id TEXT;
        CREATE UNIQUE INDEX idx_messages_client_id ON messages(session_id,client_message_id) WHERE client_message_id IS NOT NULL;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(36,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Reopen an existing discussion. A missing id and an id owned by another
    /// context are both denied, and neither creates a row.
    pub fn open_context_history(&self, actor: &str, context: &str, session: &str) -> Result<()> {
        validate_id(session)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        let open: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id=?2 AND mode='discussion' AND status='open')",
            params![session, context],
            |r| r.get(0),
        )?;
        if !open {
            return Err(StoreError::ControlAccessDenied);
        }
        tx.commit()?;
        Ok(())
    }

    /// Trusted fixture writer. A caller-chosen id can reveal that the id is
    /// already taken, so the employee door uses [`Self::create_context_history`].
    pub fn insert_open_discussion(&self, actor: &str, context: &str, session: &str) -> Result<()> {
        validate_id(session)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        let existing: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT context_id,mode,status FROM sessions WHERE id=?1",
                [session],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((bound, mode, status)) = existing {
            if bound != context || mode != "discussion" || status != "open" {
                return Err(StoreError::ControlAccessDenied);
            }
        } else {
            self.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES(?1,'','discussion','','open',?2,?3)",params![session,crate::util::now(),context])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Employee create. The id is chosen here, not by the caller.
    pub fn create_context_history(&self, actor: &str, context: &str) -> Result<String> {
        let id = crate::new_id("disc");
        self.insert_open_discussion(actor, context, &id)?;
        Ok(id)
    }

    /// Only human user messages. Principal attribution is separate from agent ID.
    /// Same session/request ID is an idempotent retry only for identical author/content.
    pub fn append_context_message(
        &self,
        actor: &str,
        context: &str,
        session: &str,
        request: &str,
        content: &str,
    ) -> Result<i64> {
        validate_id(request)?;
        if content.is_empty() || content.len() > 65_536 {
            return Err(StoreError::InvalidControlResource("message".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        let allowed: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id=?2 AND mode='discussion' AND status='open')",params![session,context],|r|r.get(0))?;
        if !allowed {
            return Err(StoreError::ControlAccessDenied);
        }
        let existing: Option<(i64,String,String)> = self.conn.query_row("SELECT seq,author_principal_id,content FROM messages WHERE session_id=?1 AND client_message_id=?2",params![session,request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let seq = if let Some((seq, author, text)) = existing {
            if author != actor || text != content {
                return Err(StoreError::ControlResourceConflict);
            }
            seq
        } else {
            let row = self.append_message(session, "user", "", content, None)?;
            self.conn.execute(
                "UPDATE messages SET author_principal_id=?1,client_message_id=?2 WHERE rowid=?3",
                params![actor, request, row],
            )?;
            self.conn
                .query_row("SELECT seq FROM messages WHERE rowid=?1", [row], |r| {
                    r.get(0)
                })?
        };
        tx.commit()?;
        Ok(seq)
    }

    pub(crate) fn require_context_access(&self, actor: &str, context: &str) -> Result<()> {
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.trim().is_empty() || id.contains('\0') || id.len() > 256 {
        return Err(StoreError::InvalidControlResource("history_id".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwner, OrganizationRole};
    #[test]
    fn closing_requires_current_access_and_preserves_history() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        db.register_control_principal("alice").unwrap();
        db.set_organization_member("org", "alice", OrganizationRole::Member)
            .unwrap();
        db.create_information_context(
            "alice",
            "private",
            &ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("alice", "private", "session")
            .unwrap();
        db.append_context_message("alice", "private", "session", "request", "retained")
            .unwrap();
        assert!(db.end_session("session", "ok", None).is_err());
        assert!(db
            .close_context_history("admin", "private", "session")
            .is_err());
        assert!(db
            .close_context_history("alice", "private", "unknown")
            .is_err());
        assert_eq!(
            db.context_discussion_status("alice", "private", "session")
                .unwrap()
                .as_deref(),
            Some("open")
        );
        assert!(db
            .context_discussion_status("alice", "private", "missing")
            .unwrap()
            .is_none());
        assert!(db.session_status("session").unwrap().is_none());
        db.close_context_history("alice", "private", "session")
            .unwrap();
        db.close_context_history("alice", "private", "session")
            .unwrap();
        assert_eq!(
            db.scoped_transcript("alice", "private", "session", 10)
                .unwrap()[0]
                .2,
            "retained"
        );
        assert!(db
            .open_context_history("alice", "private", "session")
            .is_err());
        assert!(db
            .append_context_message("alice", "private", "session", "next", "no")
            .is_err());
        db.remove_organization_member("org", "alice").unwrap();
        assert!(db
            .context_discussion_status("alice", "private", "session")
            .is_err());
        assert!(db
            .close_context_history("alice", "private", "session")
            .is_err());
    }

    #[test]
    fn guessed_discussion_id_does_not_create_or_reveal_another_context() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.create_team(&crate::TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
        db.create_information_context(
            "alice",
            "private",
            &ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.create_information_context(
            "alice",
            "shared",
            &ContextOwner::Team {
                org_id: "org".into(),
                team_id: "team".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("alice", "private", "taken").unwrap();
        assert!(db.open_context_history("alice", "shared", "taken").is_err());
        assert!(db
            .open_context_history("alice", "shared", "brand-new")
            .is_err());
        let created: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id='brand-new'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(created, 0);
        let id = db.create_context_history("alice", "shared").unwrap();
        assert!(db.open_context_history("alice", "shared", &id).is_ok());
        assert_ne!(id, "taken");
    }

    #[test]
    fn discussion_persists_attribution_and_rechecks_access_on_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("alice", "org", "Org").unwrap();
            db.create_information_context(
                "alice",
                "private",
                &ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .unwrap();
            db.insert_open_discussion("alice", "private", "session")
                .unwrap();
            db.open_context_history("alice", "private", "session")
                .unwrap();
            assert_eq!(
                db.append_context_message("alice", "private", "session", "one", "hello")
                    .unwrap(),
                1
            );
            assert_eq!(
                db.append_context_message("alice", "private", "session", "one", "hello")
                    .unwrap(),
                1
            );
            assert!(db
                .append_context_message("alice", "private", "session", "one", "changed")
                .is_err());
            let row: (String,String,String) = db.conn.query_row("SELECT author_principal_id,agent_id,role FROM messages WHERE session_id='session'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(row, ("alice".into(), "".into(), "user".into()));
            db.remove_organization_member("org", "alice").unwrap();
        }
        let db = Store::open(path).unwrap();
        assert!(db
            .append_context_message("alice", "private", "session", "one", "hello")
            .is_err());
        assert!(db
            .scoped_transcript("alice", "private", "session", 20)
            .is_err());
        db.set_organization_member("org", "alice", OrganizationRole::Member)
            .unwrap();
        assert_eq!(
            db.scoped_transcript("alice", "private", "session", 20)
                .unwrap()
                .len(),
            1
        );
        assert!(db.transcript("session").is_err());
    }

    #[test]
    fn failed_attribution_rolls_back_message_and_recall_index() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.create_information_context(
            "alice",
            "private",
            &ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("alice", "private", "session")
            .unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_author BEFORE UPDATE OF author_principal_id ON messages BEGIN SELECT RAISE(ABORT,'failure'); END;").unwrap();
        assert!(db
            .append_context_message("alice", "private", "session", "one", "canary")
            .is_err());
        assert!(db
            .scoped_transcript("alice", "private", "session", 20)
            .unwrap()
            .is_empty());
        let count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM recall_fts WHERE session_id='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn execution_audit_is_scoped_fresh_and_not_a_discussion() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        db.register_control_principal("peer").unwrap();
        db.set_organization_member("org", "peer", crate::OrganizationRole::Member)
            .unwrap();
        db.create_information_context(
            "admin",
            "private",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.create_information_context(
            "admin",
            "other",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        assert!(db
            .create_execution_audit_history("peer", "private", "denied")
            .is_err());
        db.create_execution_audit_history("admin", "private", "audit")
            .unwrap();
        assert!(db
            .create_execution_audit_history("admin", "other", "audit")
            .is_err());
        assert!(db
            .create_execution_audit_history("admin", "private", "audit")
            .is_err());
        assert!(db
            .require_execution_audit_history("private", "audit")
            .is_ok());
        assert!(db
            .require_execution_audit_history("other", "audit")
            .is_err());
        assert!(db.require_legacy_session("audit").is_err());
        assert!(db
            .open_context_history("admin", "private", "audit")
            .is_err());
        assert!(db
            .append_context_message(
                "admin",
                "private",
                "audit",
                "request",
                "forged human message"
            )
            .is_err());
        db.append_message("audit", "assistant", "agent", "EXECUTIONCANARY", None)
            .unwrap();
        assert!(db
            .scoped_transcript("peer", "private", "audit", 10)
            .is_err());
        assert!(db.scoped_transcript("admin", "other", "audit", 10).is_err());
        assert_eq!(
            db.scoped_transcript("admin", "private", "audit", 10)
                .unwrap()[0]
                .2,
            "EXECUTIONCANARY"
        );
    }
}
