//! Inert stopped-output references in the existing scoped execution audit.
use crate::{Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceDraftReceipt {
    pub artifact_id: String,
    pub digest: String,
    pub run_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub file_count: usize,
}

impl Store {
    /// Host-only: the object must already have been sealed through scoped storage.
    pub fn record_workspace_draft(
        &self,
        actor: &str,
        context: &str,
        session: &str,
        receipt: &WorkspaceDraftReceipt,
    ) -> Result<()> {
        if !self.context_artifact_access(actor, context, &receipt.artifact_id)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.require_execution_audit_history(context, session)?;
        self.append_event(
            session,
            "workspace_draft",
            "system",
            &serde_json::to_string(receipt)
                .map_err(|_| StoreError::InvalidControlResource("workspace_draft".into()))?,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn workspace_drafts(
        &self,
        actor: &str,
        context: &str,
        session: &str,
    ) -> Result<Vec<WorkspaceDraftReceipt>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.require_execution_audit_history(context, session)?;
        let values = {
            let mut stmt = self.conn.prepare("SELECT payload FROM events WHERE session_id=?1 AND kind='workspace_draft' AND actor='system' ORDER BY seq DESC LIMIT 32")?;
            let rows = stmt
                .query_map(params![session], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let receipts = values
            .iter()
            .map(|s| serde_json::from_str(s))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| StoreError::InvalidControlResource("workspace_draft".into()))?;
        tx.commit()?;
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwner, OrganizationRole};

    #[test]
    fn draft_receipts_survive_reopen_and_require_exact_context_and_current_membership() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.db");
        let receipt = WorkspaceDraftReceipt {
            artifact_id: "draft-1".into(),
            digest: "digest".into(),
            run_id: "run".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            file_count: 2,
        };
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
                db.create_execution_audit_history("alice", context, context)
                    .unwrap();
            }
            assert!(db
                .record_workspace_draft("alice", "private", "private", &receipt)
                .is_err());
            db.bind_new_context_artifact("alice", "private", "draft-1")
                .unwrap();
            db.record_workspace_draft("alice", "private", "private", &receipt)
                .unwrap();
            assert!(db
                .record_workspace_draft("alice", "private", "other", &receipt)
                .is_err());
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(
            db.workspace_drafts("alice", "private", "private").unwrap()[0].artifact_id,
            "draft-1"
        );
        assert!(db.workspace_drafts("admin", "private", "private").is_err());
        assert!(db.workspace_drafts("alice", "other", "private").is_err());
        assert!(db
            .workspace_drafts("alice", "other", "other")
            .unwrap()
            .is_empty());
        db.remove_organization_member("org", "alice").unwrap();
        assert!(db.workspace_drafts("alice", "private", "private").is_err());
    }
}
