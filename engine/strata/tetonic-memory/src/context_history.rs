use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

impl Store {
    pub(crate) fn migrate_context_messages_v36(&self) -> Result<()> {
        self.conn.execute_batch("ALTER TABLE messages ADD COLUMN author_principal_id TEXT REFERENCES control_principals(principal_id);
        ALTER TABLE messages ADD COLUMN client_message_id TEXT;
        CREATE UNIQUE INDEX idx_messages_client_id ON messages(session_id,client_message_id) WHERE client_message_id IS NOT NULL;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(36,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Opens durable discussion history, not an execution or a live model session.
    pub fn open_context_history(&self, actor: &str, context: &str, session: &str) -> Result<()> {
        validate_id(session)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, context)?;
        let existing: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT context_id,mode FROM sessions WHERE id=?1",
                [session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((bound, mode)) = existing {
            if bound != context || mode != "discussion" {
                return Err(StoreError::ControlAccessDenied);
            }
        } else {
            self.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES(?1,'','discussion','','open',?2,?3)",params![session,crate::util::now(),context])?;
        }
        tx.commit()?;
        Ok(())
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

    fn require_context_access(&self, actor: &str, context: &str) -> Result<()> {
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
            db.open_context_history("alice", "private", "session")
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
        db.open_context_history("alice", "private", "session")
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
}
