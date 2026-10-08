//! A reviewed continuation is another huddle, never a replay of an old attempt.
use crate::{ControlPermission, PlanContent, Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetainedPlanWork {
    pub work_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlanContinuation {
    pub source_work_id: String,
    pub root_work_id: String,
    pub continuation_work_id: String,
    pub request_id: String,
    pub retained: Vec<RetainedPlanWork>,
    pub review_before_repeat: Vec<RetainedPlanWork>,
    pub created_by: String,
}

pub struct CreatePlanContinuation<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub receipt: &'a PlanContinuation,
    pub guide_key: &'a str,
    pub brief: &'a str,
    pub content: &'a PlanContent,
}

impl Store {
    pub(crate) fn migrate_plan_continuation_v63(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS plan_continuations (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, source_work_id TEXT NOT NULL,
            continuation_work_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,source_work_id),
            UNIQUE(org_id,team_id,continuation_work_id),
            FOREIGN KEY(org_id,team_id,source_work_id) REFERENCES huddle_executions(org_id,team_id,source_work_id),
            FOREIGN KEY(org_id,team_id,continuation_work_id) REFERENCES team_work_items(org_id,team_id,work_id)
        );")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(63,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Read both links without starting or repairing any work.
    pub fn plan_continuation_links(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
    ) -> Result<Vec<PlanContinuation>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt = self.conn.prepare("SELECT payload FROM plan_continuations WHERE org_id=?1 AND team_id=?2 AND (source_work_id=?3 OR continuation_work_id=?3)")?;
        let values = stmt
            .query_map(params![org, team, source], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|v| serde_json::from_str(&v).map_err(|_| StoreError::ControlResourceConflict))
            .collect()
    }

    pub fn create_plan_continuation(
        &self,
        command: CreatePlanContinuation<'_>,
    ) -> Result<PlanContinuation> {
        let CreatePlanContinuation {
            actor,
            org,
            team,
            receipt,
            guide_key,
            brief,
            content,
        } = command;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)?
            || receipt.created_by != actor
        {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(old) = self
            .plan_continuation_links(actor, org, team, &receipt.source_work_id)?
            .into_iter()
            .find(|r| r.source_work_id == receipt.source_work_id)
        {
            if old.root_work_id != receipt.root_work_id {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(old);
        }
        let execution = self
            .huddle_execution(actor, org, team, &receipt.source_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if execution.root_work_id != receipt.root_work_id {
            return Err(StoreError::ControlResourceConflict);
        }
        // Terminal journal state is required even if the UI already sees a failed
        // bound attempt. An active parent may still own descendants/finalization.
        if let Some(root) = self.get_team_work_item(org, team, &execution.root_work_id)? {
            if let Some(run_id) = root.run_id {
                let run = self
                    .load_run_snapshot(&run_id)?
                    .ok_or(StoreError::ControlAccessDenied)?;
                if !matches!(
                    run.state,
                    tetonic_domain::RunState::Failed
                        | tetonic_domain::RunState::Canceled
                        | tetonic_domain::RunState::Succeeded
                ) {
                    return Err(StoreError::ControlResourceConflict);
                }
            } else if execution.start_error.is_none() {
                return Err(StoreError::ControlResourceConflict);
            }
        } else if execution.start_error.is_none() {
            return Err(StoreError::ControlResourceConflict);
        }
        let id = &receipt.continuation_work_id;
        if id == &receipt.source_work_id || self.get_team_work_item(org, team, id)?.is_some() {
            return Err(StoreError::ControlResourceConflict);
        }
        // Compose the existing work/brief/huddle owners under a single commit.
        // A failed validation leaves neither a phantom source nor a partial link.
        self.create_team_work_item_in_transaction(
            crate::CreateTeamWorkItem {
                actor,
                org,
                team,
                work_id: id,
                title: &content.title,
                request_id: &format!("{id}@{guide_key}"),
                goal_id: None,
            },
            Some("Review the continuation proposal before starting more work."),
            crate::WorkPurpose::Explore,
        )?;
        self.inherit_work_team(org, team, &receipt.source_work_id, id)?;
        self.save_work_brief_in_transaction(crate::SaveWorkBrief {
            actor,
            org,
            team,
            work: id,
            request: &receipt.request_id,
            expected: 0,
            body: brief,
        })?;
        self.save_huddle_plan_in_transaction(crate::SaveHuddlePlan {
            actor,
            org,
            team,
            work: id,
            request: &receipt.request_id,
            expected: 0,
            brief_revision: 1,
            generation_id: &format!("continuation-proposal-{id}"),
            generation_input: "Owner-requested continuation from recorded plan outcomes.",
            content: Some(content),
        })?;
        self.conn.execute(
            "INSERT INTO plan_continuations VALUES(?1,?2,?3,?4,?5)",
            params![
                org,
                team,
                receipt.source_work_id,
                id,
                serde_json::to_string(receipt).map_err(|_| StoreError::ControlResourceConflict)?
            ],
        )?;
        tx.commit()?;
        Ok(receipt.clone())
    }
}
