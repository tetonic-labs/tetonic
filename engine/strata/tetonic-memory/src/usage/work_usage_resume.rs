//! Transfer an existing usage reservation only inside a journaled resume.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension};
use tetonic_domain::{
    AttemptRecord, AttemptState, RunCommand, RunEventEnvelope, RunSnapshot, TaskState,
};

pub(crate) fn execution_fence(attempt: &AttemptRecord) -> Result<String> {
    let lease = attempt
        .lease
        .as_ref()
        .ok_or(StoreError::ControlAccessDenied)?;
    Ok(serde_json::json!([
        lease.lease_id,
        lease.lease_epoch,
        lease.holder,
        attempt.task_version
    ])
    .to_string())
}

impl Store {
    /// Caller holds the same writer transaction as the run event/projection.
    /// Projection-only recovery and arbitrary lease changes cannot move money.
    pub(crate) fn resume_work_usage_in_tx(
        &self,
        next: &RunSnapshot,
        event: &RunEventEnvelope,
    ) -> Result<()> {
        if event
            .payload
            .get("type")
            .and_then(serde_json::Value::as_str)
            != Some("resume_attempt")
        {
            return Ok(());
        }
        let RunCommand::ResumeAttempt(command) = serde_json::from_value(event.payload.clone())
            .map_err(|_| StoreError::ControlAccessDenied)?
        else {
            return Err(StoreError::ControlAccessDenied);
        };
        let old = self
            .load_run_snapshot(&next.run_id.0)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let before = old
            .attempts
            .get(&command.attempt_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let after = next
            .attempts
            .get(&command.attempt_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let old_task = old
            .tasks
            .get(&before.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let new_task = next
            .tasks
            .get(&before.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let old_lease = before
            .lease
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let new_lease = after
            .lease
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        if command.run_id != next.run_id
            || old.sequence.checked_add(1) != Some(next.sequence)
            || before.state != AttemptState::Suspended
            || !before.execution_quiesced
            || !before.execution_claimed
            || before.suspension.as_ref().map(|s| &s.checkpoint) != Some(&command.checkpoint)
            || after.state != AttemptState::Starting
            || after.execution_claimed
            || after.execution_quiesced
            || after.suspension.is_some()
            || before.task_id != after.task_id
            || before.task_version != after.task_version
            || old_task.state != TaskState::Parked
            || new_task.state != TaskState::Leased
            || old_task.binding.job_spec != new_task.binding.job_spec
            || old_task.binding.execution_scope != new_task.binding.execution_scope
            || old_task.binding.execution_grant_id != new_task.binding.execution_grant_id
            || old_task.binding.activation != new_task.binding.activation
            || old_lease.lease_id != new_lease.lease_id
            || new_lease.lease_epoch <= old_lease.lease_epoch
            || command.lease_proof.lease_id != old_lease.lease_id
            || command.lease_proof.lease_epoch != old_lease.lease_epoch
            || command.lease_proof.holder != old_lease.holder
            || new_lease.holder != command.holder
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let existing: Option<(String, String, String, i64)> = self.conn.query_row(
            "SELECT run_id,task_id,fence,released_tokens FROM work_budget_executions WHERE attempt_id=?1",
            [&command.attempt_id.0], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((run, task, fence, released)) = existing {
            if run != next.run_id.0
                || task != before.task_id.0
                || fence != execution_fence(before)?
                || released != 0
            {
                return Err(StoreError::ControlAccessDenied);
            }
            // Same attempt, allowance, reservation, reported calls and unresolved
            // charges. Only executor ownership changes; no refund or top-up.
            self.conn.execute(
                "UPDATE work_budget_executions SET fence=?2 WHERE attempt_id=?1",
                params![command.attempt_id.0, execution_fence(after)?],
            )?;
        }
        Ok(())
    }
}
