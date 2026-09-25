//! Immutable harness configuration revisions; registration retains its original
//! default. Publishing does not retarget admitted work or select a new default.
use crate::{AgentIdentityRow, ControlPermission, RegisteredAgent, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

impl Store {
    pub(crate) fn migrate_agent_definitions_v39(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE agent_definition_revisions (
            identity_id TEXT NOT NULL, definition_digest TEXT NOT NULL,
            definition_json TEXT NOT NULL,
            created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
            created_at TEXT NOT NULL, PRIMARY KEY(identity_id,definition_digest),
            FOREIGN KEY(identity_id,definition_digest) REFERENCES agent_identity_revisions(identity_id,bound_definition_digest)
        );
        INSERT INTO agent_definition_revisions SELECT identity_id,definition_digest,definition_json,created_by,created_at FROM organization_agents;
        ALTER TABLE organization_agents DROP COLUMN definition_json;
        CREATE TRIGGER agent_definition_immutable BEFORE UPDATE ON agent_definition_revisions
        BEGIN SELECT RAISE(ABORT,'agent definition is immutable'); END;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(39,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub(crate) fn insert_agent_definition(
        &self,
        identity: &AgentIdentityRow,
        json: &str,
        actor: &str,
    ) -> Result<()> {
        let existing: Option<String> = self.conn.query_row("SELECT definition_json FROM agent_definition_revisions WHERE identity_id=?1 AND definition_digest=?2", params![identity.identity_id,identity.bound_definition_digest], |r|r.get(0)).optional()?;
        if let Some(existing) = existing {
            if existing != json {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            self.conn.execute(
                "INSERT INTO agent_definition_revisions VALUES(?1,?2,?3,?4,?5)",
                params![
                    identity.identity_id,
                    identity.bound_definition_digest,
                    json,
                    actor,
                    crate::util::now()
                ],
            )?;
        }
        Ok(())
    }

    pub fn publish_organization_agent_revision(
        &self,
        actor: &str,
        org: &str,
        key: &str,
        harness: &str,
        configuration: &serde_json::Value,
    ) -> Result<RegisteredAgent> {
        let (definition_json, digest) =
            super::organization_agents::definition_payload(key, harness, configuration)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let original = self
            .registered_agent_unchecked(org, key)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let mut identity = original.identity;
        identity.bound_definition_digest = digest;
        identity.owning_application = harness.into();
        self.put_agent_identity_in_transaction(&identity)?;
        self.insert_agent_definition(&identity, &definition_json, actor)?;
        tx.commit()?;
        Ok(RegisteredAgent {
            identity,
            definition_json,
        })
    }

    pub fn get_organization_agent_revision(
        &self,
        actor: &str,
        org: &str,
        key: &str,
        digest: &str,
    ) -> Result<Option<RegisteredAgent>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.control_access(actor, ControlPermission::ReadOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let row: Option<(String,String)> = self.conn.query_row("SELECT a.identity_id,d.definition_json FROM organization_agents a JOIN agent_definition_revisions d ON d.identity_id=a.identity_id WHERE a.org_id=?1 AND a.agent_key=?2 AND d.definition_digest=?3",params![org,key,digest],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let result = row
            .map(|(id, definition_json)| {
                let identity = self
                    .get_agent_identity_revision(&id, digest)?
                    .ok_or(StoreError::ControlResourceConflict)?;
                Ok(RegisteredAgent {
                    identity,
                    definition_json,
                })
            })
            .transpose();
        tx.commit()?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publishing_preserves_original_and_exact_revisions_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("revision.db");
        let (first, second);
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("admin", "org", "Org").unwrap();
            db.register_control_principal("member").unwrap();
            db.set_organization_member("org", "member", crate::OrganizationRole::Member)
                .unwrap();
            first = db
                .register_organization_agent(
                    "admin",
                    "org",
                    "agent",
                    "general",
                    &serde_json::json!({"instructions":"first"}),
                )
                .unwrap();
            let next = serde_json::json!({"instructions":"second"});
            assert!(db
                .publish_organization_agent_revision("member", "org", "agent", "general", &next)
                .is_err());
            second = db
                .publish_organization_agent_revision("admin", "org", "agent", "general", &next)
                .unwrap();
            assert_eq!(first.identity.identity_id, second.identity.identity_id);
            assert_ne!(
                first.identity.bound_definition_digest,
                second.identity.bound_definition_digest
            );
            assert_eq!(
                second,
                db.publish_organization_agent_revision("admin", "org", "agent", "general", &next)
                    .unwrap()
            );
            assert_eq!(
                Some(first.clone()),
                db.get_organization_agent("admin", "org", "agent").unwrap()
            );
            assert!(db
                .get_organization_agent_revision(
                    "admin",
                    "foreign",
                    "agent",
                    &second.identity.bound_definition_digest
                )
                .is_err());
            db.conn.execute_batch("CREATE TRIGGER fail_definition BEFORE INSERT ON agent_definition_revisions BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
            assert!(db
                .publish_organization_agent_revision(
                    "admin",
                    "org",
                    "agent",
                    "general",
                    &serde_json::json!({"instructions":"failed"})
                )
                .is_err());
            assert_eq!(
                db.get_agent_identity(&second.identity.identity_id)
                    .unwrap()
                    .unwrap(),
                second.identity
            );
            let count: i64 = db
                .conn
                .query_row(
                    "SELECT count(*) FROM agent_identity_revisions WHERE identity_id=?1",
                    [&first.identity.identity_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        }
        let db = Store::open(&path).unwrap();
        for revision in [first, second] {
            assert_eq!(
                db.get_organization_agent_revision(
                    "member",
                    "org",
                    "agent",
                    &revision.identity.bound_definition_digest
                )
                .unwrap(),
                Some(revision)
            );
        }
        db.remove_organization_member("org", "member").unwrap();
        assert!(db
            .get_organization_agent_revision("member", "org", "agent", "unknown")
            .is_err());
    }

    #[test]
    fn v38_definition_moves_without_changing_identity_or_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
        let original;
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("admin", "org", "Org").unwrap();
            original = db
                .register_organization_agent(
                    "admin",
                    "org",
                    "agent",
                    "general",
                    &serde_json::json!({"instructions":"preserved"}),
                )
                .unwrap();
            db.conn.execute_batch("DROP TRIGGER organization_agent_immutable; ALTER TABLE organization_agents ADD COLUMN definition_json TEXT NOT NULL DEFAULT ''; UPDATE organization_agents SET definition_json=(SELECT definition_json FROM agent_definition_revisions d WHERE d.identity_id=organization_agents.identity_id AND d.definition_digest=organization_agents.definition_digest); CREATE TRIGGER organization_agent_immutable BEFORE UPDATE ON organization_agents BEGIN SELECT RAISE(ABORT,'immutable'); END; DROP TABLE agent_definition_revisions; DELETE FROM schema_versions WHERE version=39;").unwrap();
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(
            db.get_organization_agent("admin", "org", "agent").unwrap(),
            Some(original.clone())
        );
        assert_eq!(
            db.get_organization_agent_revision(
                "admin",
                "org",
                "agent",
                &original.identity.bound_definition_digest
            )
            .unwrap(),
            Some(original)
        );
    }
}
