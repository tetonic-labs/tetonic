//! Check executor ownership independently of long-lived work permission.
use super::{ActiveAttempt, ManagedRunService};
use std::sync::Arc;
use tetonic_domain::{AttemptState, RunState, TaskState};
use tetonic_memory::RecoverMutex;

impl ManagedRunService {
    pub(super) async fn authorize_executor(&self, issued: &ActiveAttempt) -> Result<(), ()> {
        let current = self
            .active
            .lock_recover()
            .get(&issued.binding.attempt_id)
            .cloned()
            .ok_or(())?;
        // A legitimate same-host resume updates the lease in this registry and
        // keeps the clock/scope. A replacement attachment has a different clock.
        if current.binding != issued.binding
            || !Arc::ptr_eq(&current.clock, &issued.clock)
            || current.suspension().is_some()
            || current.work_scope.is_canceled()
            || current.deadline_elapsed()
        {
            return Err(());
        }
        let snapshot = self
            .inspect_run(&issued.binding.run_id)
            .await
            .map_err(|_| ())?;
        let attempt = snapshot
            .attempts
            .get(&issued.binding.attempt_id)
            .ok_or(())?;
        let task = snapshot.tasks.get(&issued.binding.task_id).ok_or(())?;
        if snapshot.state != RunState::Active
            || snapshot.cancellation.run_canceled
            || attempt.state != AttemptState::Running
            || !attempt.execution_claimed
            || attempt.execution_quiesced
            || attempt.suspension.is_some()
            || task.state != TaskState::Running
            || task.active_attempt.as_ref() != Some(&attempt.attempt_id)
            || task.binding.task_definition_version != attempt.task_version
            || task.finalization_claim.is_some()
            || !crate::lease::is_lease_current(attempt, super::lifetime::unix_now())
            || !attempt.lease.as_ref().is_some_and(|lease| {
                lease.attempt_id == current.binding.attempt_id
                    && lease.lease_id == current.lease_proof.lease_id
                    && lease.lease_epoch == current.lease_proof.lease_epoch
                    && lease.holder == current.lease_proof.holder
            })
        {
            return Err(());
        }
        Ok(())
    }
}
