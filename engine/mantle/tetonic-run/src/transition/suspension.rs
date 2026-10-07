//! Suspension is part of the existing run state machine, not a work-status side table.
use super::*;
use tetonic_domain::{AttemptSuspension, ResumeAttempt, SuspendAttempt};

pub(super) fn suspend(
    snapshot: &RunSnapshot,
    cmd: &SuspendAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    let task = snapshot
        .tasks
        .get(&attempt.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(attempt.task_id.to_string()))?;
    if snapshot.tasks.len() != 1
        || task.active_attempt.as_ref() != Some(&cmd.attempt_id)
        || snapshot.attempts.values().any(|other| {
            other.attempt_id != cmd.attempt_id
                && (!other.execution_quiesced || !crate::lease::is_terminal_attempt(&other.state))
        })
        || task.binding.activation.is_none()
        || task.binding.delegation.is_some()
        || task.binding.execution_scope.is_none()
        || attempt.state != AttemptState::Running
        || !attempt.execution_claimed
        || attempt.execution_quiesced
        || attempt.suspension.is_some()
        || task.state != TaskState::Running
        || task.finalization_claim.is_some()
        || !crate::lease::is_lease_current(attempt, cmd.envelope.timestamp)
        || cmd.checkpoint.artifact_id.is_empty()
        || cmd.checkpoint.artifact_id.len() > 256
        || cmd.checkpoint.digest.is_empty()
        || cmd.checkpoint.digest.len() > 256
    {
        return Err(RunSupervisorError::InvalidTransition(
            "suspension requires a claimed, quiescent executor boundary".into(),
        ));
    }
    let remaining_seconds = task
        .binding
        .deadline
        .and_then(|deadline| deadline.checked_sub(cmd.envelope.timestamp))
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| {
            RunSupervisorError::InvalidTransition(
                "suspension requires unspent execution time".into(),
            )
        })?;
    let mut out = snapshot.clone();
    let parked = out.attempts.get_mut(&cmd.attempt_id).unwrap();
    parked.state = AttemptState::Suspended;
    parked.execution_quiesced = true;
    parked.suspension = Some(AttemptSuspension {
        checkpoint: cmd.checkpoint.clone(),
        reason: cmd.reason.clone(),
        suspended_at: cmd.envelope.timestamp,
        remaining_seconds,
    });
    out.tasks.get_mut(&attempt.task_id).unwrap().state = TaskState::Parked;
    Ok(out)
}

pub(super) fn resume(
    snapshot: &RunSnapshot,
    cmd: &ResumeAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    let parked = attempt.suspension.as_ref().ok_or_else(|| {
        RunSupervisorError::InvalidTransition("attempt has no saved suspension".into())
    })?;
    let task = snapshot
        .tasks
        .get(&attempt.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(attempt.task_id.to_string()))?;
    if snapshot.tasks.len() != 1
        || task.active_attempt.as_ref() != Some(&cmd.attempt_id)
        || !attempt.execution_claimed
        || task.finalization_claim.is_some()
        || snapshot.attempts.values().any(|other| {
            other.attempt_id != cmd.attempt_id
                && (!other.execution_quiesced || !crate::lease::is_terminal_attempt(&other.state))
        })
        || task.binding.activation.is_none()
        || task.binding.delegation.is_some()
        || task.binding.execution_scope.is_none()
        || attempt.state != AttemptState::Suspended
        || !attempt.execution_quiesced
        || task.state != TaskState::Parked
        || parked.checkpoint != cmd.checkpoint
        || cmd.envelope.timestamp < parked.suspended_at
        || parked.remaining_seconds == 0
    {
        return Err(RunSupervisorError::InvalidTransition(
            "suspension changed before resumption".into(),
        ));
    }
    let deadline = cmd
        .envelope
        .timestamp
        .checked_add(parked.remaining_seconds)
        .ok_or_else(|| RunSupervisorError::InvalidTransition("resume deadline overflow".into()))?;
    let mut out = snapshot.clone();
    let epoch = out
        .next_lease_epoch
        .checked_add(1)
        .ok_or_else(|| RunSupervisorError::InvalidTransition("lease epoch overflow".into()))?;
    out.next_lease_epoch = epoch;
    let resumed = out.attempts.get_mut(&cmd.attempt_id).unwrap();
    resumed.state = AttemptState::Starting;
    resumed.execution_claimed = false;
    resumed.execution_quiesced = false;
    resumed.suspension = None;
    let lease = resumed.lease.as_mut().unwrap();
    lease.lease_epoch = epoch;
    lease.holder = cmd.holder.clone();
    lease.issued_at = cmd.envelope.timestamp;
    lease.expires_at = cmd.envelope.timestamp.saturating_add(300);
    lease.last_heartbeat_sequence = 0;
    let resumed_task = out.tasks.get_mut(&attempt.task_id).unwrap();
    resumed_task.state = TaskState::Leased;
    resumed_task.binding.deadline = Some(deadline);
    if task.binding.activation.is_some() {
        out.deadlines.run_deadline = Some(deadline);
    }
    Ok(out)
}
