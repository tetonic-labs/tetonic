//! Durable `agent_identities` rows (schema v28). Storage only; lifecycle writer is `lokai-run`.

use rusqlite::OptionalExtension;

use crate::util::now;
use crate::{Result, Store};

impl Store {
    pub fn put_agent_identity(&self, row: &AgentIdentityRow) -> Result<()> {
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
