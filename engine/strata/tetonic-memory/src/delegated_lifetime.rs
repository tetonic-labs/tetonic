//! Work authority outlives a lease, but never a task incarnation or stop.
use crate::{DelegatedGrantLineage, DelegationLifetime, Result, StoreError};
use tetonic_domain::{AttemptRecord, AttemptState, RunSnapshot, TaskRecord, TaskState};

impl crate::Store {
    /// Existing control records are the generation authority. Include both
    /// agents and every work/goal ancestor, but do not couple unrelated teams.
    pub(super) fn delegation_stop_binding(
        &self,
        lineage: &DelegatedGrantLineage,
        parent: &tetonic_domain::AgentJobSpec,
        child: &tetonic_domain::AgentJobSpec,
    ) -> Result<String> {
        use sha2::Digest;
        let org = &lineage.org_id;
        let mut scopes = std::collections::BTreeSet::from([
            ("org", org.clone()),
            ("team", lineage.team_id.clone()),
        ]);
        for id in self.work_ancestors(org, &lineage.team_id, &lineage.child_work_id)? {
            let work = self
                .get_team_work_item(org, &lineage.team_id, &id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            scopes.insert(("work", id));
            if let Some(goal) = work.goal_id {
                scopes.insert(("goal", goal));
            }
        }
        for job in [parent, child] {
            let key: String = self.conn.query_row(
                "SELECT agent_key FROM organization_agents WHERE org_id=?1 AND identity_id=?2",
                rusqlite::params![org, job.identity_id.0],
                |r| r.get(0),
            )?;
            scopes.insert(("agent", key));
        }
        let mut generations = Vec::new();
        for (kind, id) in scopes {
            if self.active_control_stop(org, kind, &id)?.is_some() {
                return Err(StoreError::ControlAccessDenied);
            }
            let generation: i64 = self.conn.query_row(
                "SELECT COALESCE(MAX(generation),0) FROM control_stop_scopes WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3",
                rusqlite::params![org, kind, id], |r| r.get(0),
            )?;
            generations.push((kind, id, generation));
        }
        let bytes =
            serde_json::to_vec(&generations).map_err(|_| StoreError::ControlResourceConflict)?;
        Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
    }
}

fn lease_matches(lineage: &DelegatedGrantLineage, attempt: &AttemptRecord) -> bool {
    attempt.lease.as_ref().is_some_and(|lease| {
        lease.attempt_id == lineage.parent_attempt_id
            && lease.lease_id == lineage.parent_lease.lease_id
            && lease.lease_epoch == lineage.parent_lease.lease_epoch
            && lease.holder == lineage.parent_lease.holder
    })
}

pub(super) fn validate_live_parent(
    lineage: &DelegatedGrantLineage,
    run: &RunSnapshot,
    now: i64,
) -> Result<()> {
    let attempt = run
        .attempts
        .get(&lineage.parent_attempt_id)
        .ok_or(StoreError::ControlAccessDenied)?;
    let task = run
        .tasks
        .get(&lineage.parent_task_id)
        .ok_or(StoreError::ControlAccessDenied)?;
    if !lease_matches(lineage, attempt) || !running(task, attempt, now) {
        return Err(StoreError::ControlAccessDenied);
    }
    Ok(())
}

fn current_time(task: &TaskRecord, attempt: &AttemptRecord, now: i64) -> bool {
    now >= 0
        && attempt.lease.as_ref().is_some_and(|lease| {
            lease.attempt_id == attempt.attempt_id && lease.expires_at > now as u64
        })
        && task
            .binding
            .deadline
            .map_or(true, |deadline| deadline > now as u64)
}

fn running(task: &TaskRecord, attempt: &AttemptRecord, now: i64) -> bool {
    attempt.state == AttemptState::Running
        && task.state == TaskState::Running
        && attempt.execution_claimed
        && !attempt.execution_quiesced
        && attempt.suspension.is_none()
        && current_time(task, attempt, now)
}

pub(super) fn validate_parent_lifetime(
    lineage: &DelegatedGrantLineage,
    task: &TaskRecord,
    attempt: &AttemptRecord,
    now: i64,
) -> Result<()> {
    let allowed = match lineage.lifetime {
        DelegationLifetime::ParentLease => {
            lease_matches(lineage, attempt) && running(task, attempt, now)
        }
        DelegationLifetime::ParentWork => {
            // The caller verifies the same task version and active attempt.
            // Recovery or a replacement attempt is never automatic continuation.
            let lease_valid = attempt.lease.as_ref().is_some_and(|lease| {
                lease.attempt_id == lineage.parent_attempt_id
                    && lease.lease_id == lineage.parent_lease.lease_id
                    && (lease.lease_epoch > lineage.parent_lease.lease_epoch
                        || lease_matches(lineage, attempt))
            });
            let parked = attempt.state == AttemptState::Suspended
                && task.state == TaskState::Parked
                && attempt.execution_claimed
                && attempt.execution_quiesced
                && attempt.suspension.as_ref().is_some_and(|wait| {
                    wait.reason == tetonic_domain::SuspensionReason::HumanInput
                        && wait.remaining_seconds > 0
                        && now >= 0
                        && wait.suspended_at <= now as u64
                        && task
                            .binding
                            .deadline
                            .and_then(|d| d.checked_sub(wait.suspended_at))
                            == Some(wait.remaining_seconds)
                        && !wait.checkpoint.artifact_id.is_empty()
                        && !wait.checkpoint.digest.is_empty()
                });
            let resuming = attempt.state == AttemptState::Starting
                && task.state == TaskState::Leased
                && !attempt.execution_claimed
                && !attempt.execution_quiesced
                && attempt.suspension.is_none()
                && attempt
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.lease_epoch > lineage.parent_lease.lease_epoch)
                && current_time(task, attempt, now);
            lease_valid && (parked || resuming || running(task, attempt, now))
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(StoreError::ControlAccessDenied)
    }
}
