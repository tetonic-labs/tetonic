//! Question receipts use the existing run, grant, stop and allocation authorities.
use super::*;
use tetonic_domain::{ArtifactRef, AttemptId, AttemptState, RunState, TaskState};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedHumanWait {
    pub checkpoint: ArtifactRef,
    pub call_id: String,
    pub max_wait_seconds: u64,
    pub run_id: String,
    pub task_id: String,
    pub task_version: u64,
    pub execution_grant_id: String,
    pub stop_binding: String,
}

/// Host evidence, deliberately not a deserializable employee API request.
pub struct SuspendedHumanQuestion {
    pub stop_binding: String,
    pub checkpoint: ArtifactRef,
    pub call_id: String,
    pub max_wait_seconds: u64,
}

pub(super) fn question_id(attempt: &str, call: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(format!("{attempt}:{call}")))
}

impl Store {
    pub(super) fn suspended_human_binding(
        &self,
        org: &str,
        team: &str,
        work: &str,
        attempt: &str,
        proof: &SuspendedHumanQuestion,
        now: u64,
    ) -> Result<(SavedHumanWait, u64)> {
        self.require_work_allocation_open(org, team, work)?;
        if !(1..=604_800).contains(&proof.max_wait_seconds) || !text_ok(&proof.call_id, 256) {
            return Err(StoreError::ControlAccessDenied);
        }
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
            .get(&AttemptId::new(attempt))
            .ok_or(StoreError::ControlAccessDenied)?;
        let t = run
            .tasks
            .get(&a.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let saved = a
            .suspension
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let scope = t
            .binding
            .execution_scope
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let grant_id = t
            .binding
            .execution_grant_id
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let job = t
            .binding
            .job_spec
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        if run.state != RunState::Active
            || run.cancellation.run_canceled
            || run.tasks.len() != 1
            || a.state != AttemptState::Suspended
            || !a.execution_claimed
            || !a.execution_quiesced
            || t.state != TaskState::Parked
            || t.active_attempt.as_ref() != Some(&a.attempt_id)
            || t.binding.task_definition_version != a.task_version
            || t.finalization_claim.is_some()
            || t.winning_attempt.is_some()
            || t.binding.delegation.is_some()
            || saved.reason != tetonic_domain::SuspensionReason::HumanInput
            || saved.remaining_seconds == 0
            || saved.suspended_at > now
            || t.binding
                .deadline
                .and_then(|d| d.checked_sub(saved.suspended_at))
                != Some(saved.remaining_seconds)
            || saved.checkpoint != proof.checkpoint
            || scope.organization_id != org
            || self.execution_grant_is_delegated(grant_id)?
            || !self.context_access_in_organization(
                &scope.principal_id,
                &scope.information_context_id,
                org,
            )?
        {
            return Err(StoreError::ControlAccessDenied);
        }
        // Inside the caller's transaction; do not nest execution_grant_allows' transaction.
        let (payload, expiry): (String, i64) = self.conn.query_row(
            "SELECT payload,expires_at FROM execution_grants WHERE grant_id=?1 AND org_id=?2 AND revoked_at IS NULL",
            params![grant_id,org], |r| Ok((r.get(0)?,r.get(1)?)))?;
        let grant: crate::ExecutionGrant =
            serde_json::from_str(&payload).map_err(|_| StoreError::ControlAccessDenied)?;
        if grant.grant_id != *grant_id
            || grant.scope != *scope
            || grant.job != *job
            || grant.expires_at != expiry
            || expiry <= 0
            || now >= expiry as u64
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let stop_binding = self.work_human_stop_binding(org, team, work, &job.identity_id.0)?;
        if stop_binding != proof.stop_binding {
            return Err(StoreError::ControlAccessDenied);
        }
        let deadline = saved
            .suspended_at
            .saturating_add(proof.max_wait_seconds)
            .min(expiry as u64);
        Ok((
            SavedHumanWait {
                checkpoint: proof.checkpoint.clone(),
                call_id: proof.call_id.clone(),
                max_wait_seconds: proof.max_wait_seconds,
                run_id: run.run_id.0,
                task_id: a.task_id.0.clone(),
                task_version: a.task_version,
                execution_grant_id: grant_id.clone(),
                stop_binding,
            },
            deadline,
        ))
    }

    /// Trusted composition pins this existing control-generation digest into
    /// the activation. A stop cleared during a crash cannot revive that job.
    pub fn work_human_stop_binding(
        &self,
        org: &str,
        team: &str,
        work: &str,
        identity: &str,
    ) -> Result<String> {
        let mut scopes =
            std::collections::BTreeSet::from([("org", org.to_owned()), ("team", team.to_owned())]);
        for id in self.work_ancestors(org, team, work)? {
            let ancestor = self
                .get_team_work_item(org, team, &id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            scopes.insert(("work", id));
            if let Some(goal) = ancestor.goal_id {
                scopes.insert(("goal", goal));
            }
        }
        let key: String = self.conn.query_row(
            "SELECT agent_key FROM organization_agents WHERE org_id=?1 AND identity_id=?2",
            params![org, identity],
            |r| r.get(0),
        )?;
        scopes.insert(("agent", key));
        self.control_stop_generation_binding(org, scopes)
    }

    pub(crate) fn validate_human_wait(
        &self,
        org: &str,
        team: &str,
        row: &WorkHumanQuestion,
        now: u64,
    ) -> Result<()> {
        if let Some(saved) = &row.saved_wait {
            let proof = SuspendedHumanQuestion {
                stop_binding: saved.stop_binding.clone(),
                checkpoint: saved.checkpoint.clone(),
                call_id: saved.call_id.clone(),
                max_wait_seconds: saved.max_wait_seconds,
            };
            let (current, deadline) = self.suspended_human_binding(
                org,
                team,
                &row.work_id,
                &row.attempt_id,
                &proof,
                now,
            )?;
            if &current != saved
                || deadline != row.deadline
                || row.id != question_id(&row.attempt_id, &saved.call_id)
                || (row.answer.is_none() && now >= deadline)
            {
                return Err(StoreError::ControlAccessDenied);
            }
        } else {
            self.live_work_execution_deadline(org, team, &row.work_id, &row.attempt_id, now)?;
        }
        Ok(())
    }

    /// Current delivery, unlike the historical question list. An answer is task
    /// guidance only: stopped, expired or replaced execution cannot consume it.
    pub fn pending_work_human_question(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        id: &str,
        now: u64,
    ) -> Result<WorkHumanQuestion> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let row = self
            .work_human_questions(actor, org, team, work)?
            .into_iter()
            .find(|q| q.id == id)
            .ok_or(StoreError::ControlAccessDenied)?;
        self.validate_human_wait(org, team, &row, now)?;
        tx.commit()?;
        Ok(row)
    }
}
