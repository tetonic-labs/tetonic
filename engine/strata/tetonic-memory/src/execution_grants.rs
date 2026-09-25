//! Explicit, revocable permission for one scoped agent job. This is not a
//! resource sandbox or budget reservation; those remain independent controls.
use crate::{ControlPermission, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use tetonic_domain::{AgentJobSpec, ExecutionScope};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionGrant {
    pub grant_id: String,
    pub scope: ExecutionScope,
    pub job: AgentJobSpec,
    pub expires_at: i64,
}

impl Store {
    pub(crate) fn migrate_execution_grants_v40(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE execution_grants (
            grant_id TEXT PRIMARY KEY NOT NULL, org_id TEXT NOT NULL REFERENCES organizations(org_id),
            principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
            context_id TEXT NOT NULL REFERENCES information_contexts(context_id),
            identity_id TEXT NOT NULL, definition_digest TEXT NOT NULL,
            payload TEXT NOT NULL, expires_at INTEGER NOT NULL, revoked_at INTEGER,
            FOREIGN KEY(identity_id,definition_digest) REFERENCES agent_definition_revisions(identity_id,definition_digest)
        );
        CREATE TRIGGER execution_grant_immutable BEFORE UPDATE OF grant_id,org_id,principal_id,context_id,identity_id,definition_digest,payload,expires_at ON execution_grants
        BEGIN SELECT RAISE(ABORT,'execution grant is immutable'); END;
        CREATE TABLE execution_grant_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            grant_id TEXT NOT NULL REFERENCES execution_grants(grant_id),
            actor TEXT NOT NULL REFERENCES control_principals(principal_id),
            action TEXT NOT NULL, at INTEGER NOT NULL
        );")?;
        self.conn.execute(
            "INSERT INTO schema_versions VALUES(40,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Actor must be verified by the application; authority is rechecked in the transaction.
    pub fn issue_execution_grant(
        &self,
        actor: &str,
        grant: &ExecutionGrant,
        now: i64,
    ) -> Result<()> {
        let payload =
            serde_json::to_string(grant).map_err(|_| StoreError::ControlResourceConflict)?;
        if payload.len() > 65536
            || grant.grant_id.trim().is_empty()
            || grant.grant_id.len() > 256
            || grant.grant_id.contains('\0')
            || now < 0
            || grant.expires_at <= now
        {
            return Err(StoreError::InvalidControlResource("execution grant".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(
            actor,
            ControlPermission::ManageOrganization,
            &grant.scope.organization_id,
            "",
        )? || !self.context_access_in_organization(
            &grant.scope.principal_id,
            &grant.scope.information_context_id,
            &grant.scope.organization_id,
        )? {
            return Err(StoreError::ControlAccessDenied);
        }
        let registered: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM organization_agents a
            JOIN agent_definition_revisions d ON d.identity_id=a.identity_id
            WHERE a.org_id=?1 AND a.identity_id=?2 AND d.definition_digest=?3)",
            params![
                grant.scope.organization_id,
                grant.job.identity_id.0,
                grant.job.definition_digest
            ],
            |r| r.get(0),
        )?;
        if !registered {
            return Err(StoreError::ControlAccessDenied);
        }
        let old: Option<(String, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT payload,revoked_at FROM execution_grants WHERE grant_id=?1",
                [&grant.grant_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((old, revoked)) = old {
            if old != payload || revoked.is_some() {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            self.conn.execute(
                "INSERT INTO execution_grants VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL)",
                params![
                    grant.grant_id,
                    grant.scope.organization_id,
                    grant.scope.principal_id,
                    grant.scope.information_context_id,
                    grant.job.identity_id.0,
                    grant.job.definition_digest,
                    payload,
                    grant.expires_at
                ],
            )?;
            self.conn.execute("INSERT INTO execution_grant_events(grant_id,actor,action,at) VALUES(?1,?2,'issue',?3)",params![grant.grant_id,actor,now])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn revoke_execution_grant(&self, actor: &str, org: &str, id: &str, now: i64) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let row: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT revoked_at FROM execution_grants WHERE grant_id=?1 AND org_id=?2",
                params![id, org],
                |r| r.get(0),
            )
            .optional()?;
        match row {
            None => return Err(StoreError::ControlAccessDenied),
            Some(None) => {
                self.conn.execute(
                    "UPDATE execution_grants SET revoked_at=?2 WHERE grant_id=?1",
                    params![id, now],
                )?;
                self.conn.execute("INSERT INTO execution_grant_events(grant_id,actor,action,at) VALUES(?1,?2,'revoke',?3)",params![id,actor,now])?;
            }
            Some(Some(_)) => {}
        }
        tx.commit()?;
        Ok(())
    }

    pub fn execution_grant_allows(
        &self,
        id: &str,
        scope: &ExecutionScope,
        job: &AgentJobSpec,
        now: i64,
    ) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let payload: Option<String> = self.conn.query_row("SELECT payload FROM execution_grants WHERE grant_id=?1 AND revoked_at IS NULL AND expires_at>?2",
            params![id,now],|r|r.get(0)).optional()?;
        let allowed = if let Some(payload) = payload {
            let grant: ExecutionGrant =
                serde_json::from_str(&payload).map_err(|_| StoreError::ControlResourceConflict)?;
            grant.scope == *scope
                && grant.job == *job
                && self.context_access_in_organization(
                    &scope.principal_id,
                    &scope.information_context_id,
                    &scope.organization_id,
                )?
        } else {
            false
        };
        tx.commit()?;
        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_job_grants_survive_restart_reject_changes_and_revoke() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grants.db");
        let db = Store::open(&path).unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        db.register_control_principal("alice").unwrap();
        db.set_organization_member("org", "alice", crate::OrganizationRole::Member)
            .unwrap();
        db.create_information_context(
            "alice",
            "private",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        let registered = db
            .register_organization_agent(
                "admin",
                "org",
                "agent",
                "general",
                &serde_json::json!({"instructions":"test"}),
            )
            .unwrap();
        let grant = ExecutionGrant {
            grant_id: "grant".into(),
            scope: ExecutionScope {
                principal_id: "alice".into(),
                organization_id: "org".into(),
                information_context_id: "private".into(),
            },
            job: AgentJobSpec {
                identity_id: tetonic_domain::IdentityId::new(registered.identity.identity_id),
                definition_digest: registered.identity.bound_definition_digest,
                input_digest: "input".into(),
                capability_bindings: vec!["recall".into()],
                artifact_bindings: vec![],
                recovery_id: "job".into(),
            },
            expires_at: 200,
        };
        assert!(db.issue_execution_grant("alice", &grant, 100).is_err());
        db.issue_execution_grant("admin", &grant, 100).unwrap();
        db.issue_execution_grant("admin", &grant, 100).unwrap();
        let count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM execution_grant_events", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        drop(db);
        let db = Store::open(&path).unwrap();
        assert!(db
            .execution_grant_allows("grant", &grant.scope, &grant.job, 101)
            .unwrap());
        assert!(!db
            .execution_grant_allows("grant", &grant.scope, &grant.job, 200)
            .unwrap());
        let mut changed = grant.clone();
        changed.job.capability_bindings.push("run_shell".into());
        assert!(!db
            .execution_grant_allows("grant", &changed.scope, &changed.job, 101)
            .unwrap());
        assert!(db.issue_execution_grant("admin", &changed, 101).is_err());
        db.remove_organization_member("org", "alice").unwrap();
        assert!(!db
            .execution_grant_allows("grant", &grant.scope, &grant.job, 101)
            .unwrap());
        db.set_organization_member("org", "alice", crate::OrganizationRole::Member)
            .unwrap();
        db.revoke_execution_grant("admin", "org", "grant", 102)
            .unwrap();
        assert!(!db
            .execution_grant_allows("grant", &grant.scope, &grant.job, 103)
            .unwrap());
        assert!(db.issue_execution_grant("admin", &grant, 103).is_err());
        // Audit failure must roll back issuance.
        db.conn.execute_batch("CREATE TRIGGER fail_grant_audit BEFORE INSERT ON execution_grant_events BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        let mut other = grant.clone();
        other.grant_id = "other".into();
        assert!(db.issue_execution_grant("admin", &other, 103).is_err());
        assert!(!db
            .execution_grant_allows("other", &other.scope, &other.job, 103)
            .unwrap());
    }
}
