//! Artifact ownership for trusted storage adapters. Never expose binding as an
//! employee endpoint: only a newly created artifact may be registered here.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

impl Store {
    pub(crate) fn migrate_context_artifacts_v37(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version>=37)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE context_artifacts (
            artifact_id TEXT PRIMARY KEY NOT NULL,
            context_id TEXT NOT NULL REFERENCES information_contexts(context_id),
            created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
            created_at TEXT NOT NULL
        );
        CREATE INDEX idx_context_artifacts_context ON context_artifacts(context_id);
        CREATE TRIGGER context_artifact_immutable BEFORE UPDATE ON context_artifacts
        BEGIN SELECT RAISE(ABORT,'artifact context is immutable'); END;",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(37,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Trusted writer-only operation. The caller must have created this artifact
    /// in its storage namespace; possession of an ID is not proof of ownership.
    pub fn bind_new_context_artifact(
        &self,
        actor: &str,
        context: &str,
        artifact: &str,
    ) -> Result<()> {
        if artifact.is_empty()
            || artifact.len() > 200
            || !artifact
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(StoreError::InvalidControlResource("artifact_id".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let previous: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT context_id,created_by FROM context_artifacts WHERE artifact_id=?1",
                [artifact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match previous {
            Some((bound, author)) if bound == context && author == actor => {}
            Some(_) => return Err(StoreError::ControlAccessDenied),
            None => {
                self.conn.execute(
                    "INSERT INTO context_artifacts VALUES(?1,?2,?3,?4)",
                    params![artifact, context, actor, crate::util::now()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Current membership and exact artifact ownership share one read snapshot.
    pub fn context_artifact_access(
        &self,
        actor: &str,
        context: &str,
        artifact: &str,
    ) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let allowed = self.context_access(actor, context)? && self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM context_artifacts WHERE artifact_id=?1 AND context_id=?2)",
            params![artifact, context], |r| r.get::<_, bool>(0))?;
        tx.commit()?;
        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwner, OrganizationRole};

    #[test]
    fn upgrade_from_v36_preserves_context_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
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
            db.append_context_message("alice", "private", "session", "request", "preserved")
                .unwrap();
            db.remove_run_capacity_schema_for_test();
            db.conn
                .execute_batch(
                    "DROP TABLE execution_grant_events; DROP TABLE execution_grants; DROP TABLE organization_agents; DROP TABLE agent_definition_revisions; DROP TABLE context_artifacts; DELETE FROM schema_versions WHERE version>=37;",
                )
                .unwrap();
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(
            db.scoped_transcript("alice", "private", "session", 10)
                .unwrap()[0]
                .2,
            "preserved"
        );
        db.bind_new_context_artifact("alice", "private", "new-artifact")
            .unwrap();
        assert!(db
            .context_artifact_access("alice", "private", "new-artifact")
            .unwrap());
    }

    #[test]
    fn artifact_ownership_persists_and_cannot_be_rebound_or_inherit_admin_access() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifacts.db");
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("admin", "org", "Org").unwrap();
            db.register_control_principal("alice").unwrap();
            db.set_organization_member("org", "alice", OrganizationRole::Member)
                .unwrap();
            for context in ["private", "other"] {
                db.create_information_context(
                    "alice",
                    context,
                    &ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .unwrap();
            }
            db.bind_new_context_artifact("alice", "private", "artifact-1")
                .unwrap();
            db.bind_new_context_artifact("alice", "private", "artifact-1")
                .unwrap();
            assert!(db
                .bind_new_context_artifact("alice", "other", "artifact-1")
                .is_err());
            assert!(db
                .conn
                .execute("UPDATE context_artifacts SET context_id='other'", [])
                .is_err());
            assert!(!db
                .context_artifact_access("admin", "private", "artifact-1")
                .unwrap());
            assert!(!db
                .context_artifact_access("alice", "other", "artifact-1")
                .unwrap());
            assert!(!db
                .context_artifact_access("alice", "private", "unknown")
                .unwrap());
        }
        let db = Store::open(&path).unwrap();
        assert!(db
            .context_artifact_access("alice", "private", "artifact-1")
            .unwrap());
        db.remove_organization_member("org", "alice").unwrap();
        assert!(!db
            .context_artifact_access("alice", "private", "artifact-1")
            .unwrap());
        assert!(db
            .bind_new_context_artifact("alice", "private", "artifact-1")
            .is_err());
    }
}
