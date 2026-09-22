//! Side-effect idempotency before retry (M3-2).

use tetonic_domain::{RunSnapshot, RunSupervisorError, TaskId};

pub fn can_retry_side_effects(snapshot: &RunSnapshot, task_id: &TaskId) -> bool {
    let Some(task) = snapshot.tasks.get(task_id) else {
        return false;
    };
    for key in &task.side_effect_keys {
        if let Some(rec) = snapshot.side_effect_commits.get(key) {
            if rec.committed {
                return false;
            }
        }
    }
    true
}

pub fn record_side_effect_commit(
    snapshot: &mut RunSnapshot,
    task_id: &TaskId,
    operation_key: &str,
    transaction_id: Option<tetonic_domain::TransactionId>,
    committed_at: u64,
) -> Result<(), RunSupervisorError> {
    if snapshot
        .side_effect_commits
        .get(operation_key)
        .is_some_and(|r| r.committed)
    {
        return Err(RunSupervisorError::SideEffectAlreadyCommitted(
            operation_key.to_string(),
        ));
    }
    snapshot.side_effect_commits.insert(
        operation_key.to_string(),
        tetonic_domain::SideEffectCommitRecord {
            operation_key: operation_key.to_string(),
            committed: true,
            transaction_id,
            committed_at,
        },
    );
    if let Some(task) = snapshot.tasks.get_mut(task_id) {
        if !task.side_effect_keys.contains(&operation_key.to_string()) {
            task.side_effect_keys.push(operation_key.to_string());
        }
    }
    Ok(())
}
