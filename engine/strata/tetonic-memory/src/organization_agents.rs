//! Organization-owned agent registration. Definition data is configuration, not
//! authority to execute a harness or access its requested tools.
use crate::{AgentIdentityRow, ControlPermission, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredAgent {
    pub identity: AgentIdentityRow,
    pub definition_json: String,
}

impl Store {
    pub(crate) fn migrate_org_agents_v38(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE organization_agents (
            org_id TEXT NOT NULL REFERENCES organizations(org_id),
            agent_key TEXT NOT NULL, identity_id TEXT NOT NULL UNIQUE,
            definition_digest TEXT NOT NULL, definition_json TEXT NOT NULL,
            created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
            created_at TEXT NOT NULL, PRIMARY KEY(org_id,agent_key),
            FOREIGN KEY(identity_id,definition_digest) REFERENCES agent_identity_revisions(identity_id,bound_definition_digest)
        );
        CREATE TRIGGER organization_agent_immutable BEFORE UPDATE ON organization_agents
        BEGIN SELECT RAISE(ABORT,'registered agent revision is immutable'); END;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(38,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn register_organization_agent(
        &self,
        actor: &str,
        org: &str,
        key: &str,
        harness: &str,
        configuration: &serde_json::Value,
    ) -> Result<RegisteredAgent> {
        if [key, harness]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256 || s.contains('\0'))
            || !configuration.is_object()
        {
            return Err(StoreError::InvalidControlResource(
                "agent definition".into(),
            ));
        }
        let definition_json =
            serde_json::json!({"schema_version":1,"harness":harness,"configuration":configuration})
                .to_string();
        if definition_json.len() > 65536 {
            return Err(StoreError::InvalidControlResource(
                "agent definition size".into(),
            ));
        }
        let digest = format!("sha256:{:x}", Sha256::digest(definition_json.as_bytes()));
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(existing) = self.registered_agent_unchecked(org, key)? {
            if existing.definition_json != definition_json {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        let id = crate::new_id("agent");
        let identity = AgentIdentityRow {
            identity_id: id.clone(),
            owning_application: harness.into(),
            bound_definition_digest: digest.clone(),
            privilege_class: "unconfigured".into(),
            toolset_subscriptions_json: "[]".into(),
            context_bindings_json: "[]".into(),
            recovery_id: id,
        };
        self.put_agent_identity_in_transaction(&identity)?;
        self.conn.execute(
            "INSERT INTO organization_agents VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                key,
                identity.identity_id,
                digest,
                definition_json,
                actor,
                crate::util::now()
            ],
        )?;
        tx.commit()?;
        Ok(RegisteredAgent {
            identity,
            definition_json,
        })
    }

    pub fn get_organization_agent(
        &self,
        actor: &str,
        org: &str,
        key: &str,
    ) -> Result<Option<RegisteredAgent>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.control_access(actor, ControlPermission::ReadOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let result = self.registered_agent_unchecked(org, key)?;
        tx.commit()?;
        Ok(result)
    }

    fn registered_agent_unchecked(&self, org: &str, key: &str) -> Result<Option<RegisteredAgent>> {
        let row: Option<(String,String,String)> = self.conn.query_row(
            "SELECT identity_id,definition_digest,definition_json FROM organization_agents WHERE org_id=?1 AND agent_key=?2",
            params![org,key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        row.map(|(id, digest, definition_json)| {
            let identity = self
                .get_agent_identity_revision(&id, &digest)?
                .ok_or(StoreError::ControlResourceConflict)?;
            Ok(RegisteredAgent {
                identity,
                definition_json,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_atomic_scoped_immutable_and_restart_safe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.db");
        let config = serde_json::json!({"instructions":"Maintain this project","requested_tools":["read_file"]});
        let original;
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("alice", "org", "Org").unwrap();
            db.register_control_principal("bob").unwrap();
            db.set_organization_member("org", "bob", crate::OrganizationRole::Member)
                .unwrap();
            assert!(db
                .register_organization_agent("bob", "org", "maintainer", "coding", &config)
                .is_err());
            original = db
                .register_organization_agent("alice", "org", "maintainer", "coding", &config)
                .unwrap();
            assert_eq!(
                db.register_organization_agent("alice", "org", "maintainer", "coding", &config)
                    .unwrap(),
                original
            );
            assert_eq!(original.identity.privilege_class, "unconfigured");
            assert_eq!(original.identity.toolset_subscriptions_json, "[]");
            assert!(db
                .register_organization_agent("alice", "org", "maintainer", "other", &config)
                .is_err());
            assert!(db
                .get_organization_agent("alice", "other", "maintainer")
                .is_err());
            assert_eq!(
                db.get_organization_agent("bob", "org", "maintainer")
                    .unwrap()
                    .unwrap(),
                original
            );
            db.conn.execute_batch("CREATE TRIGGER reject_agent BEFORE INSERT ON organization_agents BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
            let before: i64 = db
                .conn
                .query_row("SELECT count(*) FROM agent_identities", [], |r| r.get(0))
                .unwrap();
            assert!(db
                .register_organization_agent("alice", "org", "failed", "coding", &config)
                .is_err());
            let after: i64 = db
                .conn
                .query_row("SELECT count(*) FROM agent_identities", [], |r| r.get(0))
                .unwrap();
            assert_eq!(before, after);
            db.remove_organization_member("org", "bob").unwrap();
            assert!(db
                .get_organization_agent("bob", "org", "maintainer")
                .is_err());
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(
            db.get_organization_agent("alice", "org", "maintainer")
                .unwrap()
                .unwrap(),
            original
        );
    }
}
