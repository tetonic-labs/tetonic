//! Journaled task admission, including the delegating executor's lease fence.
use super::*;
use tetonic_domain::AddTask;

pub(super) fn add_task(
    snapshot: &RunSnapshot,
    cmd: &AddTask,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    if let Some(delegation) = &cmd.binding.delegation {
        // Old journals can omit the fence. New managed admissions always carry
        // it, and checking it here makes dispatch atomic with lease replacement.
        if let Some(proof) = &delegation.parent_lease {
            let parent = snapshot
                .attempts
                .get(&delegation.parent_attempt)
                .ok_or_else(|| {
                    RunSupervisorError::AttemptNotFound(delegation.parent_attempt.to_string())
                })?;
            validate_lease_proof(parent, proof)?;
            let task = snapshot
                .tasks
                .get(&parent.task_id)
                .ok_or_else(|| RunSupervisorError::TaskNotFound(parent.task_id.to_string()))?;
            if parent.state != AttemptState::Running
                || !parent.execution_claimed
                || parent.execution_quiesced
                || parent.suspension.is_some()
                || task.state != TaskState::Running
                || task.active_attempt.as_ref() != Some(&parent.attempt_id)
                || task.finalization_claim.is_some()
                || !crate::lease::is_lease_current(parent, cmd.envelope.timestamp)
                || task
                    .binding
                    .deadline
                    .is_some_and(|d| d <= cmd.envelope.timestamp)
            {
                return Err(RunSupervisorError::InvalidTransition(
                    "delegation requires the current running parent".into(),
                ));
            }
        }
    }
    if snapshot.tasks.contains_key(&cmd.task_id) {
        return Err(RunSupervisorError::Conflict(format!(
            "task {} exists",
            cmd.task_id
        )));
    }
    let mut out = snapshot.clone();
    out.tasks.insert(
        cmd.task_id.clone(),
        new_task(cmd.task_id.clone(), cmd.binding.clone()),
    );
    recompute_blocked_ready(&mut out);
    Ok(out)
}
