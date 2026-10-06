//! Attempt-bound accounting on the existing work allowance. Token limits stop
//! subsequent calls based on provider reports; they are not a provider billing cap.
use crate::{ControlPermission, Result, Store, StoreError, WorkBudget};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use tetonic_domain::{AttemptId, AttemptState, RunState, TaskId};

/// Preserve the existing activation-key mapping for work request IDs.
pub fn work_activation_request_id(request: &str) -> String {
    if request.is_empty()
        || request.len() > 128
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        use sha2::Digest;
        format!("tw_{:x}", sha2::Sha256::digest(request.as_bytes()))
    } else {
        request.into()
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkUsage {
    pub work_id: String,
    pub title: String,
    pub purpose: String,
    pub budget: Option<WorkBudget>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub calls: i64,
    pub pending_calls: i64,
    pub unknown_calls: i64,
    pub held_tokens: i64,
    pub released_tokens: i64,
    pub over_limit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TeamBudgetSetting {
    pub revision: i64,
    pub token_limit: Option<i64>,
}

impl Store {
    pub(crate) fn migrate_work_usage_v57(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS work_budget_executions (
            attempt_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
            run_id TEXT NOT NULL, task_id TEXT NOT NULL, fence TEXT NOT NULL,
            reservation_id TEXT, allowance INTEGER, released_tokens INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id),
            FOREIGN KEY(org_id,team_id,reservation_id) REFERENCES work_budget_reservations(org_id,team_id,request_id)
        );
        CREATE INDEX IF NOT EXISTS work_budget_executions_work ON work_budget_executions(org_id,team_id,work_id);
        CREATE TABLE IF NOT EXISTS work_usage_calls (
            call_id TEXT PRIMARY KEY, attempt_id TEXT NOT NULL REFERENCES work_budget_executions(attempt_id),
            model TEXT NOT NULL, input_tokens INTEGER, output_tokens INTEGER,
            state TEXT NOT NULL CHECK(state IN ('pending','reported','unknown')), created_at TEXT NOT NULL,
            CHECK(input_tokens IS NULL OR input_tokens>=0), CHECK(output_tokens IS NULL OR output_tokens>=0)
        );
        CREATE INDEX IF NOT EXISTS work_usage_calls_attempt ON work_usage_calls(attempt_id);
        CREATE TABLE IF NOT EXISTS team_budget_settings (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, revision INTEGER NOT NULL, token_limit INTEGER,
            request_id TEXT NOT NULL, actor TEXT NOT NULL, PRIMARY KEY(org_id,team_id),
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id)
        );")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(57,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn team_budget_setting(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<TeamBudgetSetting> {
        self.require_team_participant(actor, org, team)?;
        Ok(self.conn.query_row("SELECT revision,token_limit FROM team_budget_settings WHERE org_id=?1 AND team_id=?2",
            params![org,team], |r| Ok(TeamBudgetSetting { revision:r.get(0)?,token_limit:r.get(1)? })).optional()?
            .unwrap_or(TeamBudgetSetting {revision:0,token_limit:None}))
    }

    pub fn set_team_budget_setting(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        request: &str,
        expected: i64,
        tokens: Option<i64>,
    ) -> Result<TeamBudgetSetting> {
        if request.is_empty()
            || request.len() > 128
            || expected < 0
            || expected == i64::MAX
            || request.contains('\0')
            || tokens.is_some_and(|v| !(1..=1_000_000_000).contains(&v))
        {
            return Err(StoreError::InvalidControlResource("budget setting".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let old = self.team_budget_setting(actor, org, team)?;
        let previous: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT request_id,actor FROM team_budget_settings WHERE org_id=?1 AND team_id=?2",
                params![org, team],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if previous == Some((request.into(), actor.into()))
            && old.token_limit == tokens
            && old.revision == expected + 1
        {
            tx.commit()?;
            return Ok(old);
        }
        if previous.as_ref().is_some_and(|v| v.0 == request) || old.revision != expected {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute("INSERT INTO team_budget_settings VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(org_id,team_id) DO UPDATE SET revision=excluded.revision,token_limit=excluded.token_limit,request_id=excluded.request_id,actor=excluded.actor",
            params![org,team,expected+1,tokens,request,actor])?;
        tx.commit()?;
        Ok(TeamBudgetSetting {
            revision: expected + 1,
            token_limit: tokens,
        })
    }

    /// Host-only call gate. IDs come from the stamped managed request, not tool
    /// arguments. A durable pending record precedes every provider invocation.
    pub fn begin_work_inference(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        run: &str,
        task: &str,
        attempt: &str,
        call: &str,
        model: &str,
        now: u64,
    ) -> Result<Option<i64>> {
        self.begin_work_inference_with_limit(
            actor, org, team, work, run, task, attempt, call, model, now, None,
        )
    }

    /// A host ceiling limits the attempt's own share, leaving unreserved capacity
    /// for children. It never enlarges the durable work allocation.
    pub fn begin_work_inference_with_limit(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        run: &str,
        task: &str,
        attempt: &str,
        call: &str,
        model: &str,
        now: u64,
        own_limit: Option<i64>,
    ) -> Result<Option<i64>> {
        if own_limit.is_some_and(|v| v <= 0) {
            return Err(StoreError::InvalidControlResource(
                "work token allowance".into(),
            ));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.require_work_allocation_open(org, team, work)?;
        self.require_bounded_work_reports(org, team, work)?;
        let row = self
            .get_team_work_item(org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let snapshot = self
            .load_run_snapshot(run)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let task_row = snapshot
            .tasks
            .get(&TaskId::new(task))
            .ok_or(StoreError::ControlAccessDenied)?;
        let attempt_row = snapshot
            .attempts
            .get(&AttemptId::new(attempt))
            .ok_or(StoreError::ControlAccessDenied)?;
        let scope = task_row
            .binding
            .execution_scope
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let lease = attempt_row
            .lease
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let activation = task_row
            .binding
            .activation
            .as_ref()
            .or_else(|| {
                task_row
                    .binding
                    .delegation
                    .as_ref()
                    .map(|child| &child.activation)
            })
            .ok_or(StoreError::ControlAccessDenied)?;
        if snapshot.state != RunState::Active
            || snapshot.cancellation.run_canceled
            || attempt_row.state != AttemptState::Running
            || !attempt_row.execution_claimed
            || attempt_row.execution_quiesced
            || attempt_row.task_id.0 != task
            || task_row.active_attempt.as_ref() != Some(&AttemptId::new(attempt))
            || task_row.binding.task_definition_version != attempt_row.task_version
            || task_row.binding.deadline.is_some_and(|d| d <= now)
            || lease.expires_at <= now
            || lease.attempt_id.0 != attempt
            || scope.principal_id != actor
            || scope.organization_id != org
            || row.run_id.as_deref().is_some_and(|id| id != run)
            || row.attempt_id.as_deref().is_some_and(|id| id != attempt)
            || work_activation_request_id(&row.request_id) != activation.request_id
            || !self.context_access_in_organization(actor, &scope.information_context_id, org)?
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let fence = serde_json::json!([
            lease.lease_id,
            lease.lease_epoch,
            lease.holder,
            attempt_row.task_version
        ])
        .to_string();
        let existing: Option<(String,String,String,Option<i64>,i64,String,String,String)> = self.conn.query_row(
            "SELECT work_id,run_id,fence,allowance,released_tokens,org_id,team_id,task_id FROM work_budget_executions WHERE attempt_id=?1",[attempt],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
        let allowance = if let Some((
            old_work,
            old_run,
            old_fence,
            allowance,
            released,
            old_org,
            old_team,
            old_task,
        )) = existing
        {
            if old_work != work
                || old_run != run
                || old_fence != fence
                || released != 0
                || old_org != org
                || old_team != team
                || old_task != task
            {
                return Err(StoreError::ControlAccessDenied);
            }
            allowance
        } else {
            let ancestors = self.work_ancestors(org, team, work)?;
            let root = ancestors.last().ok_or(StoreError::ControlAccessDenied)?;
            let funded: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM work_budget_envelopes WHERE org_id=?1 AND team_id=?2 AND work_id=?3)",params![org,team,root],|r|r.get(0))?;
            let allowance = if funded {
                let available = self
                    .work_budget_unchecked(org, team, work)?
                    .available_tokens;
                if available <= 0 {
                    return Err(StoreError::InvalidControlResource(
                        "work token allowance exhausted".into(),
                    ));
                }
                Some(own_limit.map_or(available, |cap| cap.min(available)))
            } else {
                None
            };
            let reservation = allowance.map(|_| format!("execution/{attempt}"));
            if let (Some(tokens), Some(id)) = (allowance, reservation.as_ref()) {
                self.reserve_work_budget_in_tx(actor, org, team, work, id, tokens)?;
            }
            self.conn.execute("INSERT INTO work_budget_executions(attempt_id,org_id,team_id,work_id,run_id,task_id,fence,reservation_id,allowance) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![attempt,org,team,work,run,task,fence,reservation,allowance])?;
            allowance
        };
        let (used,unresolved): (i64,i64) = self.conn.query_row("SELECT COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0),COALESCE(SUM(state!='reported'),0) FROM work_usage_calls WHERE attempt_id=?1",[attempt],|r| Ok((r.get(0)?,r.get(1)?)))?;
        if allowance.is_some() && (unresolved > 0 || allowance.is_some_and(|limit| used >= limit)) {
            return Err(StoreError::InvalidControlResource(
                "work token allowance exhausted or usage unconfirmed".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO work_usage_calls VALUES(?1,?2,?3,NULL,NULL,'pending',?4)",
            params![call, attempt, model, crate::util::now()],
        )?;
        tx.commit()?;
        Ok(allowance.map(|limit| limit - used))
    }

    /// A reported overrun in any allocated branch closes further inference in
    /// that tree. In-flight requests may still report usage; it is never hidden.
    pub(crate) fn require_bounded_work_reports(
        &self,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<()> {
        let ancestors = self.work_ancestors(org, team, work)?;
        let root = ancestors.last().ok_or(StoreError::ControlAccessDenied)?;
        let exceeded:bool=self.conn.query_row(
            "WITH RECURSIVE tree(work_id,depth) AS (
                SELECT ?3,0 UNION ALL
                SELECT d.child_work_id,t.depth+1 FROM tree t JOIN work_delegations d
                ON d.parent_work_id=t.work_id AND d.org_id=?1 AND d.team_id=?2 WHERE t.depth<31
            ) SELECT EXISTS(SELECT e.attempt_id FROM work_budget_executions e
                JOIN tree t ON t.work_id=e.work_id JOIN work_usage_calls c ON c.attempt_id=e.attempt_id
                WHERE e.org_id=?1 AND e.team_id=?2 AND e.allowance IS NOT NULL
                GROUP BY e.attempt_id HAVING SUM(COALESCE(c.input_tokens,0)+COALESCE(c.output_tokens,0))>e.allowance)",
            params![org,team,root],|r|r.get(0))?;
        if exceeded {
            return Err(StoreError::InvalidControlResource(
                "work tree token allowance exceeded".into(),
            ));
        }
        Ok(())
    }

    /// Trusted provider reports only. Unknown/canceled calls retain their hold.
    /// Exact duplicate completion is harmless; conflicting reports never rewrite history.
    pub fn finish_work_inference(
        &self,
        call: &str,
        input: Option<i64>,
        output: Option<i64>,
    ) -> Result<()> {
        if input.is_some_and(|v| v < 0) || output.is_some_and(|v| v < 0) {
            return Err(StoreError::InvalidControlResource("usage".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (state, old_input, old_output): (String, Option<i64>, Option<i64>) =
            self.conn.query_row(
                "SELECT state,input_tokens,output_tokens FROM work_usage_calls WHERE call_id=?1",
                [call],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
        if state != "pending" {
            if old_input != input || old_output != output {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            let state = if input.is_some() && output.is_some() {
                "reported"
            } else {
                "unknown"
            };
            self.conn.execute("UPDATE work_usage_calls SET state=?2,input_tokens=?3,output_tokens=?4 WHERE call_id=?1",params![call,state,input,output])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Release unused tokens only after the managed owner proves quiescence and
    /// every provider call has a complete report. No read operation settles work.
    pub fn settle_work_inference(&self, attempt: &str) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let row: Option<(String, Option<i64>, String, String)> = self
            .conn
            .query_row(
                "SELECT run_id,allowance,task_id,fence FROM work_budget_executions WHERE attempt_id=?1",
                [attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((run, Some(allowance), task, fence)) = row {
            let snapshot = self
                .load_run_snapshot(&run)?
                .ok_or(StoreError::ControlAccessDenied)?;
            let owner = snapshot
                .attempts
                .get(&AttemptId::new(attempt))
                .ok_or(StoreError::ControlAccessDenied)?;
            let lease = owner
                .lease
                .as_ref()
                .ok_or(StoreError::ControlAccessDenied)?;
            let current_fence = serde_json::json!([
                lease.lease_id,
                lease.lease_epoch,
                lease.holder,
                owner.task_version
            ])
            .to_string();
            if !owner.execution_quiesced
                || owner.task_id.0 != task
                || current_fence != fence
                || !matches!(
                    owner.state,
                    AttemptState::Succeeded
                        | AttemptState::Failed
                        | AttemptState::TimedOut
                        | AttemptState::LeaseExpired
                        | AttemptState::Canceled
                        | AttemptState::Superseded
                )
            {
                return Err(StoreError::ControlAccessDenied);
            }
            let (used,unknown): (i64,i64) = self.conn.query_row("SELECT COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0),COALESCE(SUM(state!='reported'),0) FROM work_usage_calls WHERE attempt_id=?1",[attempt],|r| Ok((r.get(0)?,r.get(1)?)))?;
            if unknown == 0 && used <= allowance {
                self.conn.execute(
                    "UPDATE work_budget_executions SET released_tokens=?2 WHERE attempt_id=?1",
                    params![attempt, allowance - used],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn team_work_usage(&self, actor: &str, org: &str, team: &str) -> Result<Vec<WorkUsage>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        self.require_team_participant(actor, org, team)?;
        let work = self.list_team_work_items(actor, org, team)?;
        let mut result = Vec::new();
        for item in work {
            let ancestors = self.work_ancestors(org, team, &item.work_id)?;
            let funded: bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM work_budget_envelopes WHERE org_id=?1 AND team_id=?2 AND work_id=?3)",params![org,team,ancestors.last()],|r|r.get(0))?;
            let budget = if funded {
                Some(self.work_budget_unchecked(org, team, &item.work_id)?)
            } else {
                None
            };
            let (input,output,calls,unreported,pending): (i64,i64,i64,i64,i64)=self.conn.query_row(
                "SELECT COALESCE(SUM(c.input_tokens),0),COALESCE(SUM(c.output_tokens),0),COUNT(*),COALESCE(SUM(c.state='unknown'),0),COALESCE(SUM(c.state='pending'),0)
                FROM work_usage_calls c JOIN work_budget_executions e ON e.attempt_id=c.attempt_id WHERE e.org_id=?1 AND e.team_id=?2 AND e.work_id=?3",params![org,team,item.work_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            // Pending calls may survive a crash. Durable run truth distinguishes
            // work still owned by an active attempt from unconfirmed usage.
            let mut live_pending = 0;
            let mut stmt=self.conn.prepare("SELECT e.run_id,e.attempt_id,COUNT(*) FROM work_usage_calls c JOIN work_budget_executions e ON e.attempt_id=c.attempt_id WHERE e.org_id=?1 AND e.team_id=?2 AND e.work_id=?3 AND c.state='pending' GROUP BY e.run_id,e.attempt_id")?;
            let groups = stmt
                .query_map(params![org, team, item.work_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (run, attempt, count) in groups {
                if self.load_run_snapshot(&run)?.is_some_and(|r| {
                    r.state == RunState::Active
                        && r.attempts.get(&AttemptId::new(attempt)).is_some_and(|a| {
                            a.state == AttemptState::Running
                                && !a.execution_quiesced
                                && a.lease.as_ref().is_some_and(|lease| {
                                    lease.expires_at > chrono::Utc::now().timestamp().max(0) as u64
                                })
                        })
                }) {
                    live_pending += count;
                }
            }
            let released: i64=self.conn.query_row("SELECT COALESCE(SUM(released_tokens),0) FROM work_budget_executions WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,item.work_id],|r|r.get(0))?;
            let used = input.saturating_add(output);
            let attempt_overrun: bool = self.conn.query_row("SELECT EXISTS(
                SELECT e.attempt_id FROM work_budget_executions e JOIN work_usage_calls c ON c.attempt_id=e.attempt_id
                WHERE e.org_id=?1 AND e.team_id=?2 AND e.work_id=?3 AND e.allowance IS NOT NULL
                GROUP BY e.attempt_id HAVING SUM(COALESCE(c.input_tokens,0)+COALESCE(c.output_tokens,0))>e.allowance)",params![org,team,item.work_id],|r|r.get(0))?;
            result.push(WorkUsage {
                work_id: item.work_id,
                title: item.title,
                purpose: format!("{:?}", item.purpose).to_lowercase(),
                held_tokens: budget
                    .as_ref()
                    .map_or(0, |b| b.reserved_tokens.saturating_sub(used).max(0)),
                over_limit: attempt_overrun
                    || budget.as_ref().is_some_and(|b| used > b.token_limit),
                budget,
                input_tokens: input,
                output_tokens: output,
                calls,
                pending_calls: live_pending,
                unknown_calls: unreported + pending - live_pending,
                released_tokens: released,
            });
        }
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "work_usage_tests.rs"]
mod tests;
