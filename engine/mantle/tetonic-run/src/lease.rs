//! Lease validation and expiration (M3-2).

use tetonic_domain::{
    AttemptLease, AttemptRecord, AttemptState, LeaseProof, RunSnapshot, RunState,
    RunSupervisorError, TaskState,
};

pub fn validate_lease_proof(
    attempt: &AttemptRecord,
    proof: &LeaseProof,
) -> Result<(), RunSupervisorError> {
    let Some(lease) = &attempt.lease else {
        return Err(RunSupervisorError::InvalidTransition(
            "attempt has no lease".into(),
        ));
    };
    if lease.lease_id != proof.lease_id {
        return Err(RunSupervisorError::StaleResult("lease id mismatch".into()));
    }
    if lease.lease_epoch != proof.lease_epoch {
        return Err(RunSupervisorError::StaleLeaseEpoch {
            expected: lease.lease_epoch,
            actual: proof.lease_epoch,
        });
    }
    if lease.holder != proof.holder {
        return Err(RunSupervisorError::StaleResult(
            "lease holder mismatch".into(),
        ));
    }
    Ok(())
}

pub fn is_lease_current(attempt: &AttemptRecord, now: u64) -> bool {
    attempt.lease.as_ref().is_some_and(|l| {
        l.expires_at >= now
            && matches!(
                attempt.state,
                AttemptState::Leased | AttemptState::Starting | AttemptState::Running
            )
    })
}

pub fn is_result_acceptable_state(state: &AttemptState) -> bool {
    matches!(
        state,
        AttemptState::Running | AttemptState::Starting | AttemptState::Leased
    )
}

pub fn is_terminal_attempt(state: &AttemptState) -> bool {
    matches!(
        state,
        AttemptState::Succeeded
            | AttemptState::Failed
            | AttemptState::TimedOut
            | AttemptState::LeaseExpired
            | AttemptState::Canceled
            | AttemptState::Superseded
    )
}

pub fn renew_lease(lease: &mut AttemptLease, heartbeat_sequence: u64, new_expires_at: u64) {
    lease.last_heartbeat_sequence = heartbeat_sequence;
    lease.expires_at = new_expires_at;
}

pub fn can_renew_lease(snapshot: &RunSnapshot, attempt: &AttemptRecord, now: u64) -> bool {
    if snapshot.cancellation.run_canceled || snapshot.state == RunState::Canceled {
        return false;
    }
    let task = snapshot.tasks.get(&attempt.task_id);
    if task.is_some_and(|t| t.state == TaskState::Canceled) {
        return false;
    }
    is_lease_current(attempt, now)
}
