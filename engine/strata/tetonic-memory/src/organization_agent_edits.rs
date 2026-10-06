//! Select a new default while keeping registered identities and admitted revisions intact.
use crate::{ControlPermission, RegisteredAgent, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

pub struct AgentEdit<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub key: &'a str,
    pub request: &'a str,
    pub expected: &'a str,
    pub harness: &'a str,
    pub configuration: &'a serde_json::Value,
}

impl Store {
    #[cfg(test)]
    pub(crate) fn remove_agent_edits_schema_for_test(&self) {
        self.conn.execute_batch("DROP TRIGGER IF EXISTS agent_edit_immutable; DROP TABLE IF EXISTS organization_agent_edits; DROP TABLE IF EXISTS organization_agent_heads;").unwrap();
    }

    pub(crate) fn migrate_agent_edits_v62(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=62)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch("CREATE TABLE organization_agent_heads (
            org_id TEXT NOT NULL, agent_key TEXT NOT NULL, identity_id TEXT NOT NULL,
            definition_digest TEXT NOT NULL,
            PRIMARY KEY(org_id,agent_key),
            FOREIGN KEY(org_id,agent_key) REFERENCES organization_agents(org_id,agent_key),
            FOREIGN KEY(identity_id,definition_digest) REFERENCES agent_definition_revisions(identity_id,definition_digest)
        );
        CREATE TABLE organization_agent_edits (
            org_id TEXT NOT NULL, agent_key TEXT NOT NULL, request_id TEXT NOT NULL,
            expected_digest TEXT NOT NULL, definition_digest TEXT NOT NULL,
            actor TEXT NOT NULL REFERENCES control_principals(principal_id), created_at TEXT NOT NULL,
            PRIMARY KEY(org_id,agent_key,request_id),
            FOREIGN KEY(org_id,agent_key) REFERENCES organization_agents(org_id,agent_key)
        );
        CREATE TRIGGER agent_edit_immutable BEFORE UPDATE ON organization_agent_edits
        BEGIN SELECT RAISE(ABORT,'agent edit receipt is immutable'); END;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(62,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn edit_organization_agent(&self, edit: AgentEdit<'_>) -> Result<RegisteredAgent> {
        let AgentEdit {
            actor,
            org,
            key,
            request,
            expected,
            harness,
            configuration,
        } = edit;
        if request.is_empty() || request.len() > 256 || expected.is_empty() {
            return Err(StoreError::InvalidControlResource("agent edit".into()));
        }
        let (definition_json, digest) =
            super::organization_agents::definition_payload(key, harness, configuration)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let current = self
            .registered_agent_unchecked(org, key)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let receipt: Option<(String, String)> = self.conn.query_row(
            "SELECT expected_digest,definition_digest FROM organization_agent_edits WHERE org_id=?1 AND agent_key=?2 AND request_id=?3",
            params![org,key,request], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((old_expected, old_digest)) = receipt {
            if old_expected != expected || old_digest != digest {
                return Err(StoreError::ControlResourceConflict);
            }
            let identity = self
                .get_agent_identity_revision(&current.identity.identity_id, &digest)?
                .ok_or(StoreError::ControlResourceConflict)?;
            tx.commit()?;
            return Ok(RegisteredAgent {
                identity,
                definition_json,
            });
        }
        if current.identity.bound_definition_digest != expected {
            return Err(StoreError::ControlResourceConflict);
        }
        let mut identity = current.identity;
        identity.bound_definition_digest = digest.clone();
        identity.owning_application = harness.into();
        self.put_agent_identity_in_transaction(&identity)?;
        self.insert_agent_definition(&identity, &definition_json, actor)?;
        self.conn.execute("INSERT INTO organization_agent_heads VALUES(?1,?2,?3,?4)
            ON CONFLICT(org_id,agent_key) DO UPDATE SET definition_digest=excluded.definition_digest",
            params![org,key,identity.identity_id,digest])?;
        self.conn.execute(
            "INSERT INTO organization_agent_edits VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                key,
                request,
                expected,
                digest,
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
}
