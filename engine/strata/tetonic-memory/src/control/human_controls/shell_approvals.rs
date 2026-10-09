//! Exact shell proposals share the existing team approval records.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellApprovalProposal {
    // Missing for legacy shell proposals: preserve their signed digest exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub command: String,
    pub working_directory: String,
    pub shell: String,
    pub attempt_id: String,
    pub call_id: String,
    pub parameter_digest: String,
    pub confinement_warnings: Vec<String>,
}

impl ShellApprovalProposal {
    pub fn digest(&self) -> String {
        format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("shell proposal"))
        )
    }
}

impl Store {
    pub(crate) fn migrate_shell_approvals_v61(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=61)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        // The schema migrator owns the upgrade transaction and backup.
        let present: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('effect_approvals') WHERE name='proposal_json')", [], |r| r.get(0))?;
        if !present {
            self.conn
                .execute_batch("ALTER TABLE effect_approvals ADD COLUMN proposal_json TEXT;")?;
        }
        self.conn.execute(
            "INSERT INTO schema_versions VALUES(61,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub(super) fn shell_approval_live(&self, row: &EffectApproval, now: i64) -> Result<()> {
        if let Some(proposal) = &row.proposal {
            let work = row
                .work_id
                .as_deref()
                .ok_or(StoreError::ControlAccessDenied)?;
            let deadline = self.live_work_execution_deadline(
                &row.org_id,
                &row.team_id,
                work,
                &proposal.attempt_id,
                now.max(0) as u64,
            )?;
            if proposal.digest() != row.proposal_digest || row.expires_at > deadline as i64 {
                return Err(StoreError::ControlAccessDenied);
            }
        }
        Ok(())
    }

    /// One specific running attempt consumes an approved command once. Polling a
    /// pending request is not authority; cancellation and deadline are rechecked.
    pub fn consume_shell_approval(
        &self,
        org: &str,
        team: &str,
        id: &str,
        digest: &str,
        now: i64,
    ) -> Result<Option<bool>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let row = self
            .get_effect_approval(org, team, id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if row.proposal.is_none() || row.proposal_digest != digest || row.expires_at <= now {
            return Err(StoreError::ControlAccessDenied);
        }
        self.shell_approval_live(&row, now)?;
        let result = match row.status.as_str() {
            "pending" => None,
            "approved" => {
                self.conn.execute("UPDATE effect_approvals SET status='consumed' WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='approved'", params![org, team, id])?;
                Some(true)
            }
            _ => Some(false),
        };
        tx.commit()?;
        Ok(result)
    }
}
