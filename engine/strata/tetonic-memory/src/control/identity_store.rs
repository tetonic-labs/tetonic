//! Latest identity projection (v28) and immutable revision snapshots (v34).
//! Storage only; lifecycle writer is `tetonic-run`.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use crate::util::now;
use crate::{Result, Store, StoreError};

impl Store {
    pub fn put_agent_identity(&self, row: &AgentIdentityRow) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.put_agent_identity_in_transaction(row)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn put_agent_identity_in_transaction(&self, row: &AgentIdentityRow) -> Result<()> {
        if let Some(existing) =
            self.get_agent_identity_revision(&row.identity_id, &row.bound_definition_digest)?
        {
            if existing != *row {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            self.conn.execute(
                "INSERT INTO agent_identity_revisions VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![
                    row.identity_id,
                    row.owning_application,
                    row.bound_definition_digest,
                    row.privilege_class,
                    row.toolset_subscriptions_json,
                    row.context_bindings_json,
                    row.recovery_id
                ],
            )?;
        }
        let ts = now();
        self.conn.execute(
            "INSERT INTO agent_identities(\n\
                 identity_id, owning_application, bound_definition_digest, privilege_class,\n\
                 toolset_subscriptions_json, context_bindings_json, recovery_id,\n\
                 created_at, updated_at\n\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)\n\
             ON CONFLICT(identity_id) DO UPDATE SET\n\
                 owning_application = excluded.owning_application,\n\
                 bound_definition_digest = excluded.bound_definition_digest,\n\
                 privilege_class = excluded.privilege_class,\n\
                 toolset_subscriptions_json = excluded.toolset_subscriptions_json,\n\
                 context_bindings_json = excluded.context_bindings_json,\n\
                 recovery_id = excluded.recovery_id,\n\
                 updated_at = excluded.updated_at",
            rusqlite::params![
                row.identity_id,
                row.owning_application,
                row.bound_definition_digest,
                row.privilege_class,
                row.toolset_subscriptions_json,
                row.context_bindings_json,
                row.recovery_id,
                ts,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn migrate_identity_revisions_v34(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=34)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch("CREATE TABLE agent_identity_revisions (
            identity_id TEXT NOT NULL, owning_application TEXT NOT NULL,
            bound_definition_digest TEXT NOT NULL, privilege_class TEXT NOT NULL,
            toolset_subscriptions_json TEXT NOT NULL, context_bindings_json TEXT NOT NULL,
            recovery_id TEXT NOT NULL, PRIMARY KEY(identity_id,bound_definition_digest));
            INSERT INTO agent_identity_revisions SELECT identity_id,owning_application,bound_definition_digest,privilege_class,toolset_subscriptions_json,context_bindings_json,recovery_id FROM agent_identities;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(34,?1)",
            [now()],
        )?;
        Ok(())
    }

    pub fn get_agent_identity_revision(
        &self,
        identity_id: &str,
        digest: &str,
    ) -> Result<Option<AgentIdentityRow>> {
        self.conn.query_row("SELECT identity_id,owning_application,bound_definition_digest,privilege_class,toolset_subscriptions_json,context_bindings_json,recovery_id FROM agent_identity_revisions WHERE identity_id=?1 AND bound_definition_digest=?2", [identity_id,digest], |r| Ok(AgentIdentityRow {
            identity_id:r.get(0)?, owning_application:r.get(1)?,bound_definition_digest:r.get(2)?,privilege_class:r.get(3)?,toolset_subscriptions_json:r.get(4)?,context_bindings_json:r.get(5)?,recovery_id:r.get(6)?
        })).optional().map_err(Into::into)
    }

    pub fn get_agent_identity(&self, identity_id: &str) -> Result<Option<AgentIdentityRow>> {
        self.conn
            .query_row(
                "SELECT identity_id, owning_application, bound_definition_digest, privilege_class,\n\
                     toolset_subscriptions_json, context_bindings_json, recovery_id\n\
                 FROM agent_identities WHERE identity_id = ?1",
                [identity_id],
                |r| {
                    Ok(AgentIdentityRow {
                        identity_id: r.get(0)?,
                        owning_application: r.get(1)?,
                        bound_definition_digest: r.get(2)?,
                        privilege_class: r.get(3)?,
                        toolset_subscriptions_json: r.get(4)?,
                        context_bindings_json: r.get(5)?,
                        recovery_id: r.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIdentityRow {
    pub identity_id: String,
    pub owning_application: String,
    pub bound_definition_digest: String,
    pub privilege_class: String,
    pub toolset_subscriptions_json: String,
    pub context_bindings_json: String,
    pub recovery_id: String,
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn revisions_survive_updates_reopen_and_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("revisions.db");
        let original = crate::AgentIdentityRow {
            identity_id: "agent".into(),
            owning_application: "app".into(),
            bound_definition_digest: "v1".into(),
            privilege_class: "standard".into(),
            toolset_subscriptions_json: "[]".into(),
            context_bindings_json: "[]".into(),
            recovery_id: "agent".into(),
        };
        {
            let db = Store::open(&path).unwrap();
            db.put_agent_identity(&original).unwrap();
            db.remove_context_schema_for_test();
            db.conn.execute_batch("DROP TABLE agent_identity_revisions; DELETE FROM schema_versions WHERE version>=34;").unwrap();
        }
        {
            let db = Store::open(&path).unwrap();
            assert_eq!(
                db.get_agent_identity_revision("agent", "v1").unwrap(),
                Some(original.clone())
            );
            let mut updated = original.clone();
            updated.bound_definition_digest = "v2".into();
            updated.context_bindings_json = "[\"team\"]".into();
            db.put_agent_identity(&updated).unwrap();
            db.put_agent_identity(&updated).unwrap();
            let mut collision = original.clone();
            collision.privilege_class = "elevated".into();
            assert!(matches!(
                db.put_agent_identity(&collision),
                Err(crate::StoreError::ControlResourceConflict)
            ));
            assert_eq!(db.get_agent_identity("agent").unwrap(), Some(updated));
        }
        let db = Store::open(path).unwrap();
        assert_eq!(
            db.get_agent_identity_revision("agent", "v1").unwrap(),
            Some(original)
        );
        assert!(db
            .get_agent_identity_revision("agent", "unknown")
            .unwrap()
            .is_none());
    }

    #[test]
    fn put_get_agent_identity_round_trip() {
        let store = Store::open(":memory:").unwrap();
        store
            .put_agent_identity(&crate::AgentIdentityRow {
                identity_id: "id_coding_production".into(),
                owning_application: "coding".into(),
                bound_definition_digest: "digest".into(),
                privilege_class: "default".into(),
                toolset_subscriptions_json: r#"["planner"]"#.into(),
                context_bindings_json: r#"["memory"]"#.into(),
                recovery_id: "id_coding_production".into(),
            })
            .unwrap();
        let row = store
            .get_agent_identity("id_coding_production")
            .unwrap()
            .expect("row");
        assert_eq!(row.owning_application, "coding");
        assert_eq!(row.bound_definition_digest, "digest");
        assert_eq!(row.recovery_id, "id_coding_production");
    }
}
