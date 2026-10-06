//! Scoped questions and owner amendments for the existing finite plan execution.
//! These records do not grant tools, create runs, or change an allowance.
use crate::{ControlPermission, Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanQuestionContent {
    pub question: String,
    pub why: String,
    #[serde(default)]
    pub options: Vec<String>,
}
impl HumanQuestionContent {
    pub fn validate(&self) -> Result<()> {
        if !text_ok(&self.question, 800)
            || !text_ok(&self.why, 1200)
            || self.options.len() > 4
            || self.options.iter().any(|s| !text_ok(s, 300))
        {
            return Err(StoreError::InvalidControlResource("human question".into()));
        }
        Ok(())
    }
}
fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.contains('\0')
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkHumanQuestion {
    pub id: String,
    pub work_id: String,
    pub source_work_id: String,
    pub attempt_id: String,
    pub content: HumanQuestionContent,
    pub deadline: u64,
    pub answer: Option<String>,
    pub response_id: Option<String>,
    pub created_at: String,
    pub answered_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDirection {
    pub revision: i64,
    pub request_id: String,
    pub assignment_key: String,
    pub instructions: String,
    pub affected_work_ids: Vec<String>,
    pub retained_work_ids: Vec<String>,
    pub actor: String,
}

impl Store {
    pub(crate) fn migrate_plan_human_v60(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS work_human_questions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, question_id TEXT NOT NULL,
            work_id TEXT NOT NULL, source_work_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
            payload TEXT NOT NULL, answer TEXT, response_id TEXT, answered_by TEXT,
            PRIMARY KEY(org_id,team_id,question_id), UNIQUE(org_id,team_id,response_id),
            FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id)
        );
        CREATE INDEX IF NOT EXISTS work_human_questions_work ON work_human_questions(org_id,team_id,work_id);
        CREATE TABLE IF NOT EXISTS huddle_execution_directions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, source_work_id TEXT NOT NULL,
            revision INTEGER NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,source_work_id,revision), UNIQUE(org_id,team_id,request_id),
            FOREIGN KEY(org_id,team_id,source_work_id) REFERENCES huddle_executions(org_id,team_id,source_work_id)
        );")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(60,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub(crate) fn human_live_deadline(
        &self,
        org: &str,
        team: &str,
        work: &str,
        attempt: &str,
        now: u64,
    ) -> Result<u64> {
        self.require_work_allocation_open(org, team, work)?;
        let w = self
            .get_team_work_item(org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if w.attempt_id.as_deref() != Some(attempt) {
            return Err(StoreError::ControlAccessDenied);
        }
        let run = self
            .load_run_snapshot(w.run_id.as_deref().ok_or(StoreError::ControlAccessDenied)?)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let a = run
            .attempts
            .get(&tetonic_domain::AttemptId::new(attempt))
            .ok_or(StoreError::ControlAccessDenied)?;
        let t = run
            .tasks
            .get(&a.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let deadline = t.binding.deadline.ok_or(StoreError::ControlAccessDenied)?;
        if run.state != tetonic_domain::RunState::Active
            || run.cancellation.run_canceled
            || a.state != tetonic_domain::AttemptState::Running
            || t.state != tetonic_domain::TaskState::Running
            || now >= deadline
        {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(deadline)
    }

    pub fn work_human_questions(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Vec<WorkHumanQuestion>> {
        self.require_team_participant(actor, org, team)?;
        let mut q=self.conn.prepare("SELECT payload,answer,response_id,answered_by FROM work_human_questions WHERE org_id=?1 AND team_id=?2 AND work_id=?3 ORDER BY rowid")?;
        let rows = q
            .query_map(params![org, team, work], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(json, answer, response_id, answered_by)| {
                let mut row: WorkHumanQuestion =
                    serde_json::from_str(&json).map_err(|_| StoreError::ControlResourceConflict)?;
                row.answer = answer;
                row.response_id = response_id;
                row.answered_by = answered_by;
                Ok(row)
            })
            .collect()
    }

    pub fn ask_work_human(&self, command: crate::AskWorkHuman<'_>) -> Result<WorkHumanQuestion> {
        let crate::AskWorkHuman {
            actor,
            org,
            team,
            work,
            attempt,
            id,
            content,
            now,
        } = command;
        content.validate()?;
        if !text_ok(id, 128) {
            return Err(StoreError::ControlResourceConflict);
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let deadline = self.human_live_deadline(org, team, work, attempt, now)?;
        let plan = self
            .huddle_execution_for_work(actor, org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let previous = self.work_human_questions(actor, org, team, work)?;
        if let Some(old) = previous.iter().find(|r| r.id == id) {
            if old.attempt_id != attempt || old.content != content {
                return Err(StoreError::ControlResourceConflict);
            }
            let old = old.clone();
            tx.commit()?;
            return Ok(old);
        }
        // A bounded conversation, not an unlimited model/human interruption loop.
        if previous.iter().any(|r| r.answer.is_none()) || previous.len() >= 2 {
            return Err(StoreError::ControlResourceConflict);
        }
        let row = WorkHumanQuestion {
            id: id.into(),
            work_id: work.into(),
            source_work_id: plan.source_work_id.clone(),
            attempt_id: attempt.into(),
            content,
            deadline,
            answer: None,
            response_id: None,
            created_at: crate::util::now(),
            answered_by: None,
        };
        self.conn.execute("INSERT INTO work_human_questions(org_id,team_id,question_id,work_id,source_work_id,attempt_id,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![org,team,id,work,plan.source_work_id,attempt,serde_json::to_string(&row).unwrap()])?;
        tx.commit()?;
        Ok(row)
    }

    pub fn answer_work_human(
        &self,
        command: crate::AnswerWorkHuman<'_>,
    ) -> Result<WorkHumanQuestion> {
        let crate::AnswerWorkHuman {
            actor,
            org,
            team,
            work,
            id,
            request,
            answer,
            now,
        } = command;
        if !text_ok(answer, 6000) || !text_ok(request, 128) {
            return Err(StoreError::InvalidControlResource("human answer".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let mut row = self
            .work_human_questions(actor, org, team, work)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(StoreError::ControlAccessDenied)?;
        if row.answer.is_some() {
            if row.response_id.as_deref() != Some(request)
                || row.answer.as_deref() != Some(answer)
                || row.answered_by.as_deref() != Some(actor)
            {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(row);
        }
        self.human_live_deadline(org, team, work, &row.attempt_id, now)?;
        self.conn.execute("UPDATE work_human_questions SET answer=?4,response_id=?5,answered_by=?6 WHERE org_id=?1 AND team_id=?2 AND question_id=?3 AND answer IS NULL",params![org,team,id,answer,request,actor])?;
        row.answer = Some(answer.into());
        row.response_id = Some(request.into());
        row.answered_by = Some(actor.into());
        tx.commit()?;
        Ok(row)
    }

    pub fn plan_directions(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
    ) -> Result<Vec<PlanDirection>> {
        self.require_team_participant(actor, org, team)?;
        let mut q=self.conn.prepare("SELECT payload FROM huddle_execution_directions WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 ORDER BY revision")?;
        let rows = q
            .query_map(params![org, team, source], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|v| serde_json::from_str(&v).map_err(|_| StoreError::ControlResourceConflict))
            .collect()
    }

    /// Called under the same local admission gate as dispatch; transaction rechecks
    /// every affected work binding to close the last-read/activation race.
    pub fn amend_plan_assignment(
        &self,
        command: crate::AmendPlanAssignment<'_>,
    ) -> Result<PlanDirection> {
        let crate::AmendPlanAssignment {
            actor,
            org,
            team,
            source,
            expected,
            request,
            key,
            instructions,
            now,
        } = command;
        if !(0..=12).contains(&expected) || !text_ok(instructions, 6000) || !text_ok(request, 128) {
            return Err(StoreError::InvalidControlResource("plan direction".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let old = self.plan_directions(actor, org, team, source)?;
        if let Some(row) = old.iter().find(|r| r.request_id == request) {
            if row.revision != expected + 1
                || row.assignment_key != key
                || row.instructions != instructions
                || row.actor != actor
            {
                return Err(StoreError::ControlResourceConflict);
            }
            let row = row.clone();
            tx.commit()?;
            return Ok(row);
        }
        if old.last().map_or(0, |r| r.revision) != expected || old.len() >= 12 {
            return Err(StoreError::ControlResourceConflict);
        }
        let plan = self
            .huddle_execution(actor, org, team, source)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let root = self
            .get_team_work_item(org, team, &plan.root_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        self.human_live_deadline(
            org,
            team,
            &plan.root_work_id,
            root.attempt_id
                .as_deref()
                .ok_or(StoreError::ControlAccessDenied)?,
            now,
        )?;
        if !plan.assignments.iter().any(|a| a.assignment_key == key) {
            return Err(StoreError::ControlResourceConflict);
        }
        let mut affected = std::collections::HashSet::from([key.to_string()]);
        loop {
            let count = affected.len();
            for a in &plan.content.assignments {
                if a.depends_on.iter().any(|d| affected.contains(d)) {
                    affected.insert(a.key.clone());
                }
            }
            if affected.len() == count {
                break;
            }
        }
        let mut affected_work_ids = vec![];
        let mut retained_work_ids = vec![];
        for pin in &plan.assignments {
            let item = self
                .get_team_work_item(org, team, &pin.work_id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if affected.contains(&pin.assignment_key) {
                if item.run_id.is_some() || item.attempt_id.is_some() || item.status != "open" {
                    return Err(StoreError::ControlResourceConflict);
                }
                affected_work_ids.push(pin.work_id.clone());
            } else {
                retained_work_ids.push(pin.work_id.clone());
            }
        }
        let row = PlanDirection {
            revision: expected + 1,
            request_id: request.into(),
            assignment_key: key.into(),
            instructions: instructions.into(),
            affected_work_ids,
            retained_work_ids,
            actor: actor.into(),
        };
        self.conn.execute(
            "INSERT INTO huddle_execution_directions VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                org,
                team,
                source,
                row.revision,
                request,
                serde_json::to_string(&row).unwrap()
            ],
        )?;
        tx.commit()?;
        Ok(row)
    }
}
