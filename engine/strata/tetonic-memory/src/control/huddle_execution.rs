//! A durable start receipt for an agreed huddle. Execution stays in team work,
//! managed runs and the existing budget ledger; this is only their provenance.
use crate::{ControlPermission, PlanContent, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

mod progress;
pub use progress::{AssignmentProgress, AssignmentState, HuddleProgress};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlanAgentPin {
    pub assignment_key: String,
    pub work_id: String,
    pub agent_key: String,
    pub definition_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HuddleExecution {
    #[serde(default = "legacy_plan_deadline")]
    pub max_elapsed_seconds: u64,
    pub source_work_id: String,
    pub revision: i64,
    pub request_id: String,
    pub root_work_id: String,
    pub coordinator_digest: String,
    pub assignments: Vec<PlanAgentPin>,
    pub brief_revision: i64,
    pub brief: String,
    pub content: PlanContent,
    pub created_by: String,
    pub start_error: Option<String>,
}

fn legacy_plan_deadline() -> u64 {
    120
}

impl Store {
    pub(crate) fn migrate_huddle_execution_v59(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS huddle_executions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, source_work_id TEXT NOT NULL,
            revision INTEGER NOT NULL, request_id TEXT NOT NULL, root_work_id TEXT NOT NULL,
            payload TEXT NOT NULL, start_error TEXT,
            PRIMARY KEY(org_id,team_id,source_work_id),
            UNIQUE(org_id,team_id,request_id), UNIQUE(org_id,team_id,root_work_id),
            FOREIGN KEY(org_id,team_id,source_work_id,revision)
                REFERENCES huddle_proposals(org_id,team_id,huddle_id,proposal_version)
        );
        CREATE TABLE IF NOT EXISTS huddle_execution_work (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
            source_work_id TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,work_id),
            FOREIGN KEY(org_id,team_id,source_work_id) REFERENCES huddle_executions(org_id,team_id,source_work_id)
        );")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(59,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn huddle_execution_for_work(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Option<HuddleExecution>> {
        self.require_team_participant(actor, org, team)?;
        let source:Option<String>=self.conn.query_row("SELECT source_work_id FROM huddle_execution_work WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,work],|r|r.get(0)).optional()?;
        source
            .map(|source| self.huddle_execution(actor, org, team, &source))
            .transpose()
            .map(Option::flatten)
    }

    pub fn huddle_execution(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
    ) -> Result<Option<HuddleExecution>> {
        self.require_team_participant(actor, org, team)?;
        let row: Option<(String,Option<String>)> = self.conn.query_row(
            "SELECT payload,start_error FROM huddle_executions WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3",
            params![org,team,source], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        row.map(|(json, error)| {
            let mut receipt: HuddleExecution =
                serde_json::from_str(&json).map_err(|_| StoreError::ControlResourceConflict)?;
            receipt.start_error = error;
            Ok(receipt)
        })
        .transpose()
    }

    /// Claims the one finite execution of this huddle. A duplicate is a receipt,
    /// never permission to replay admission after a crash or lost response.
    pub fn begin_huddle_execution(
        &self,
        command: crate::BeginHuddleExecution<'_>,
    ) -> Result<(HuddleExecution, bool)> {
        let crate::BeginHuddleExecution {
            actor,
            org,
            team,
            source,
            revision,
            request,
            root,
            coordinator_digest,
            pins,
            max_elapsed_seconds,
        } = command;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(old) = self.huddle_execution(actor, org, team, source)? {
            if old.request_id != request || old.revision != revision || old.created_by != actor {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok((old, false));
        }
        if [request, root, coordinator_digest]
            .iter()
            .any(|s| s.is_empty() || s.len() > 128 || s.contains('\0'))
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let plan = self
            .huddle_plans(actor, org, team, source)?
            .into_iter()
            .next()
            .ok_or(StoreError::ControlResourceConflict)?;
        let (brief_revision,brief):(i64,String)=self.conn.query_row(
            "SELECT revision,body FROM work_brief_revisions WHERE org_id=?1 AND team_id=?2 AND work_id=?3 ORDER BY revision DESC LIMIT 1",
            params![org,team,source],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if plan.revision != revision
            || plan.brief_revision != brief_revision
            || plan.status != "agreed"
        {
            return Err(StoreError::ControlResourceConflict);
        }
        if max_elapsed_seconds == 0 || max_elapsed_seconds > 86400 {
            return Err(StoreError::ControlResourceConflict);
        }
        let content = plan.content.ok_or(StoreError::ControlResourceConflict)?;
        content.validate()?;
        self.validate_work_team_assignments(actor, org, team, source, &content)?;
        let mut ids = std::collections::HashSet::new();
        ids.insert(root);
        if pins.len() != content.assignments.len()
            || pins.iter().zip(&content.assignments).any(|(pin, a)| {
                pin.assignment_key != a.key
                    || pin.agent_key != a.agent_key
                    || pin.definition_digest.is_empty()
                    || pin.work_id.is_empty()
                    || pin.work_id.len() > 128
                    || !ids.insert(pin.work_id.as_str())
            })
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let receipt = HuddleExecution {
            max_elapsed_seconds,
            source_work_id: source.into(),
            revision,
            request_id: request.into(),
            root_work_id: root.into(),
            coordinator_digest: coordinator_digest.into(),
            assignments: pins.into(),
            brief_revision,
            brief,
            content,
            created_by: actor.into(),
            start_error: None,
        };
        self.conn.execute("INSERT INTO huddle_executions(org_id,team_id,source_work_id,revision,request_id,root_work_id,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![org,team,source,revision,request,root,serde_json::to_string(&receipt).unwrap()])?;
        for work in std::iter::once(root).chain(pins.iter().map(|pin| pin.work_id.as_str())) {
            self.conn.execute(
                "INSERT INTO huddle_execution_work VALUES(?1,?2,?3,?4)",
                params![org, team, work, source],
            )?;
        }
        tx.commit()?;
        Ok((receipt, true))
    }

    pub fn record_huddle_start_error(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
        error: &str,
    ) -> Result<()> {
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute("UPDATE huddle_executions SET start_error=COALESCE(start_error,?4) WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3",
            params![org,team,source,error.chars().take(2000).collect::<String>()])?;
        Ok(())
    }
}
