//! Restart recovery for ambiguous run state (M3-1 / M3-2).

use tetonic_domain::{
    AttemptId, AttemptState, FailureClass, RunSnapshot, RunState, RunSupervisorError, TaskState,
};

use crate::retry::apply_failure_with_retry;

pub fn detect_recovery_required(snapshot: &RunSnapshot, now: u64) -> bool {
    if snapshot.state == RunState::RecoveryRequired {
        return true;
    }
    // An intentionally parked run is different from an abandoned live worker.
    // Only the supported root-only contract qualifies; never exempt a subtree.
    if snapshot.state == RunState::Active
        && !snapshot.cancellation.run_canceled
        && snapshot.tasks.len() == 1
        && snapshot.tasks.values().all(|t| {
            t.binding.activation.is_some()
                && t.binding.execution_scope.is_some()
                && t.binding.delegation.is_none()
                && t.finalization_claim.is_none()
        })
        && snapshot
            .attempts
            .values()
            .any(|a| a.state == AttemptState::Suspended)
        && snapshot.attempts.values().all(|a| {
            a.execution_quiesced
                && (crate::lease::is_terminal_attempt(&a.state)
                    || (a.state == AttemptState::Suspended
                        && a.execution_claimed
                        && a.lease.is_some()
                        && a.suspension.as_ref().is_some_and(|s| {
                            s.remaining_seconds > 0
                                && !s.checkpoint.artifact_id.is_empty()
                                && !s.checkpoint.digest.is_empty()
                        })
                        && snapshot.tasks.get(&a.task_id).is_some_and(|t| {
                            t.state == TaskState::Parked
                                && t.active_attempt.as_ref() == Some(&a.attempt_id)
                        })))
        })
        && snapshot.tasks.values().all(|t| {
            matches!(
                t.state,
                TaskState::Parked
                    | TaskState::Succeeded
                    | TaskState::Failed
                    | TaskState::Canceled
                    | TaskState::Skipped
            )
        })
    {
        return false;
    }
    if snapshot
        .attempts
        .values()
        .any(|a| a.state == AttemptState::Suspended)
    {
        return true;
    }
    for attempt in snapshot.attempts.values() {
        if matches!(
            attempt.state,
            AttemptState::Leased | AttemptState::Starting | AttemptState::Running
        ) {
            if let Some(lease) = &attempt.lease {
                if lease.expires_at < now {
                    return true;
                }
            } else if snapshot.state == RunState::Active {
                return true;
            }
        }
    }
    for task in snapshot.tasks.values() {
        if (task.state == TaskState::Leased || task.state == TaskState::Running)
            && task.active_attempt.is_none()
        {
            return true;
        }
        // A: claimed + Active (crash or fail-closed after ClaimFinalization).
        if task.finalization_claim.is_some() && snapshot.state == RunState::Active {
            return true;
        }
    }
    if snapshot.state == RunState::Active {
        // B: CompleteAttempt without FinishRun.
        if snapshot
            .attempts
            .values()
            .any(|a| a.state == AttemptState::Succeeded)
        {
            return true;
        }
        // D: coding/sessionless FailAttempt without FinishRun. Hops are job_spec: None.
        let has_terminal_failed = snapshot.attempts.values().any(|a| {
            matches!(
                a.state,
                AttemptState::Failed
                    | AttemptState::LeaseExpired
                    | AttemptState::TimedOut
                    | AttemptState::Canceled
            )
        });
        let has_inflight = snapshot.attempts.values().any(|a| {
            matches!(
                a.state,
                AttemptState::Leased | AttemptState::Starting | AttemptState::Running
            )
        });
        if snapshot.job_spec.is_some() && has_terminal_failed && !has_inflight {
            return true;
        }
    }
    false
}

// `mark_recovery_required` was deleted in M6. It set RunState::RecoveryRequired
// with no detector guard, which is the one way to produce that state without a
// condition in the snapshot that implies it. Recovery's durability across
// restarts rests on the flag being derivable from persisted state
// (`detect_recovery_required` above), so an unguarded setter is not a
// convenience — it is the counterexample. The sole writer is now
// `DurableRunSupervisor::recover_at_startup`, guarded by that detector.

pub fn recover_expired_leases(
    snapshot: &mut RunSnapshot,
    now: u64,
) -> Result<(), RunSupervisorError> {
    let expired: Vec<AttemptId> = snapshot
        .attempts
        .iter()
        .filter(|(_, a)| {
            matches!(
                a.state,
                AttemptState::Leased | AttemptState::Starting | AttemptState::Running
            ) && a.lease.as_ref().is_some_and(|l| l.expires_at < now)
        })
        .map(|(id, _)| id.clone())
        .collect();

    for attempt_id in expired {
        let task_id = snapshot.attempts.get(&attempt_id).unwrap().task_id.clone();
        if let Some(a) = snapshot.attempts.get_mut(&attempt_id) {
            a.state = AttemptState::LeaseExpired;
            a.failure_class = Some(FailureClass::LeaseExpired);
            a.failure_reason = Some("lease expired on recovery".into());
        }
        apply_failure_with_retry(
            snapshot,
            &task_id,
            FailureClass::LeaseExpired,
            "lease expired on recovery",
            now,
        );
    }
    Ok(())
}
