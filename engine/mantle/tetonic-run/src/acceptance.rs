//! Atomic result acceptance and winner selection (M3-2).

use tetonic_domain::{
    AttemptId, AttemptState, ClaimFinalization, CompleteAttempt, RunSnapshot, RunSupervisorError,
    TaskState,
};

use crate::idempotency::{input_digest_matches, workspace_matches};
use crate::lease::{is_lease_current, is_result_acceptable_state, validate_lease_proof};

pub fn try_accept_completion(
    snapshot: &RunSnapshot,
    cmd: &CompleteAttempt,
) -> Result<(), RunSupervisorError> {
    if snapshot.cancellation.run_canceled {
        return Err(RunSupervisorError::InvalidTransition("run canceled".into()));
    }
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;

    if attempt.state == AttemptState::Succeeded
        && attempt.result_digest.as_deref() == Some(&cmd.result_digest)
    {
        return Ok(());
    }

    if !is_result_acceptable_state(&attempt.state) {
        return Err(RunSupervisorError::StaleResult(format!(
            "cannot accept result from {:?}",
            attempt.state
        )));
    }

    validate_lease_proof(attempt, &cmd.lease_proof)?;
    if !is_lease_current(attempt, cmd.envelope.timestamp) {
        return Err(RunSupervisorError::StaleResult("lease expired".into()));
    }

    if attempt.task_version != cmd.task_version {
        return Err(RunSupervisorError::Conflict("task version mismatch".into()));
    }

    let task = snapshot
        .tasks
        .get(&attempt.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(attempt.task_id.to_string()))?;

    if task.state == TaskState::Canceled {
        return Err(RunSupervisorError::InvalidTransition(
            "task canceled".into(),
        ));
    }

    let digest_ok = match task
        .binding
        .job_spec
        .as_ref()
        .or(snapshot.job_spec.as_ref())
    {
        Some(spec) => spec.input_digest == cmd.input_digest,
        None => input_digest_matches(&task.binding, &cmd.input_digest),
    };
    if !digest_ok {
        return Err(RunSupervisorError::Conflict("input digest mismatch".into()));
    }

    if !workspace_matches(&task.binding, &cmd.workspace_version) {
        return Err(RunSupervisorError::Conflict(
            "workspace version mismatch".into(),
        ));
    }

    if cmd.result_digest.trim().is_empty() {
        return Err(RunSupervisorError::Conflict("invalid result digest".into()));
    }

    if let Some(winner) = &task.winning_attempt {
        if winner != &cmd.attempt_id {
            return Err(RunSupervisorError::StaleResult(
                "another attempt already won".into(),
            ));
        }
    }

    if let Some(completed) = task.completed_version {
        if completed == cmd.task_version && task.winning_attempt.is_some() {
            return Err(RunSupervisorError::StaleResult(
                "task version already completed".into(),
            ));
        }
    }

    if let Some(deadline) = task.binding.deadline {
        if cmd.envelope.timestamp > deadline {
            return Err(RunSupervisorError::DeadlineExceeded(
                tetonic_domain::TimeoutKind::Task,
            ));
        }
    }

    Ok(())
}

pub fn try_claim_finalization(
    snapshot: &RunSnapshot,
    cmd: &ClaimFinalization,
) -> Result<(), RunSupervisorError> {
    if snapshot.cancellation.run_canceled {
        return Err(RunSupervisorError::InvalidTransition("run canceled".into()));
    }
    let task = snapshot
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    if task.finalization_claim.as_ref() == Some(&cmd.attempt_id) {
        return Ok(());
    }
    if task.finalization_claim.is_some() {
        return Err(RunSupervisorError::StaleResult(
            "another attempt already claimed".into(),
        ));
    }
    if let Some(winner) = &task.winning_attempt {
        if winner != &cmd.attempt_id {
            return Err(RunSupervisorError::StaleResult(
                "another attempt already won".into(),
            ));
        }
    }
    if task.state == TaskState::Canceled {
        return Err(RunSupervisorError::InvalidTransition(
            "task canceled".into(),
        ));
    }
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.task_id != cmd.task_id {
        return Err(RunSupervisorError::Conflict("task mismatch".into()));
    }
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    if !is_lease_current(attempt, cmd.envelope.timestamp) {
        return Err(RunSupervisorError::StaleResult("lease expired".into()));
    }
    if attempt.task_version != cmd.task_version {
        return Err(RunSupervisorError::Conflict("task version mismatch".into()));
    }
    let digest_ok = match task
        .binding
        .job_spec
        .as_ref()
        .or(snapshot.job_spec.as_ref())
    {
        Some(spec) => spec.input_digest == cmd.input_digest,
        None => input_digest_matches(&task.binding, &cmd.input_digest),
    };
    if !digest_ok {
        return Err(RunSupervisorError::Conflict("input digest mismatch".into()));
    }
    Ok(())
}

pub fn apply_winner_selection(
    out: &mut RunSnapshot,
    attempt_id: &AttemptId,
    task_id: &tetonic_domain::TaskId,
    task_version: u64,
) {
    let task = out.tasks.get_mut(task_id).unwrap();
    task.winning_attempt = Some(attempt_id.clone());
    task.completed_version = Some(task_version);

    let sibling_ids: Vec<_> = out
        .attempts
        .iter()
        .filter(|(id, a)| a.task_id == *task_id && *id != attempt_id)
        .map(|(id, _)| id.clone())
        .collect();

    for sid in sibling_ids {
        if let Some(a) = out.attempts.get_mut(&sid) {
            if matches!(
                a.state,
                AttemptState::Created
                    | AttemptState::Leased
                    | AttemptState::Starting
                    | AttemptState::Running
            ) {
                a.state = AttemptState::Superseded;
            }
        }
    }
}
