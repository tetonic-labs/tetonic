//! Child execution slots are projections of task bindings in the existing run
//! journal, committed in the same transaction. An interrupted admission holds
//! its slot until the journal proves that its worker has quiesced.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension};
use tetonic_domain::{AttemptState, RunSnapshot, TaskState};

impl Store {
    pub(crate) fn migrate_child_capacity_v58(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS registered_child_capacity (
            run_id TEXT NOT NULL, task_id TEXT NOT NULL, identity_id TEXT NOT NULL,
            execution_org_id TEXT NOT NULL, execution_principal_id TEXT NOT NULL,
            execution_team_id TEXT, execution_held INTEGER NOT NULL,
            PRIMARY KEY(run_id,task_id));
            CREATE INDEX IF NOT EXISTS child_identity_capacity ON registered_child_capacity(identity_id) WHERE execution_held=1;
            CREATE INDEX IF NOT EXISTS child_org_capacity ON registered_child_capacity(execution_org_id,execution_principal_id,execution_team_id) WHERE execution_held=1;")?;
        let rows = self
            .conn
            .prepare(
                "SELECT projection_json FROM run_projections r WHERE EXISTS (
            SELECT 1 FROM json_each(r.projection_json,'$.tasks') task
            WHERE json_extract(task.value,'$.binding.delegation') IS NOT NULL)",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for row in rows {
            let snapshot = serde_json::from_str(&row)
                .map_err(|_| StoreError::InvalidControlResource("run projection".into()))?;
            self.sync_child_capacity(&snapshot, false)?;
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(58,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// The caller owns the projection transaction. Recovery reconstructs holds;
    /// admission additionally checks ceilings before creating a new hold.
    pub(crate) fn sync_child_capacity(
        &self,
        snapshot: &RunSnapshot,
        admission: bool,
    ) -> Result<()> {
        for task in snapshot.tasks.values() {
            let Some(child) = &task.binding.delegation else {
                continue;
            };
            let scope = task
                .binding
                .execution_scope
                .as_ref()
                .ok_or(StoreError::ControlAccessDenied)?;
            let job = task
                .binding
                .job_spec
                .as_ref()
                .ok_or(StoreError::ControlAccessDenied)?;
            let parent = snapshot
                .attempts
                .get(&child.parent_attempt)
                .ok_or(StoreError::ControlAccessDenied)?;
            let parent_task = snapshot
                .tasks
                .get(&parent.task_id)
                .ok_or(StoreError::ControlAccessDenied)?;
            if task.binding.activation.is_some()
                || parent.task_id == task.task_id
                || parent_task.binding.execution_scope.as_ref() != Some(scope)
                || task.binding.execution_grant_id.is_none()
                || crate::usage::run_capacity::registered_scope(snapshot) != Some(scope)
            {
                return Err(StoreError::ControlAccessDenied);
            }
            let attempts: Vec<_> = snapshot
                .attempts
                .values()
                .filter(|a| a.task_id == task.task_id)
                .collect();
            let held = !(matches!(
                task.state,
                TaskState::Succeeded | TaskState::Failed | TaskState::Canceled | TaskState::Skipped
            ) && !attempts.is_empty()
                && attempts.iter().all(|a| {
                    a.execution_quiesced
                        && matches!(
                            a.state,
                            AttemptState::Succeeded
                                | AttemptState::Failed
                                | AttemptState::Canceled
                                | AttemptState::TimedOut
                                | AttemptState::LeaseExpired
                                | AttemptState::Superseded
                        )
                }));
            let team = self.team_for_execution_context(&scope.information_context_id)?;
            let old: Option<(String,bool,String,String,Option<String>)> = self.conn.query_row("SELECT identity_id,execution_held,execution_org_id,execution_principal_id,execution_team_id FROM registered_child_capacity WHERE run_id=?1 AND task_id=?2",
                params![snapshot.run_id.0,task.task_id.0],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
            if old
                .as_ref()
                .is_some_and(|(id, _, org, principal, old_team)| {
                    id != &job.identity_id.0
                        || org != &scope.organization_id
                        || principal != &scope.principal_id
                        || old_team != &team
                })
            {
                return Err(StoreError::ControlResourceConflict);
            }
            if admission && held && !old.as_ref().is_some_and(|(_, held, ..)| *held) {
                let occupied: bool = self.conn.query_row("SELECT EXISTS(
                    SELECT 1 FROM run_projections WHERE registered_identity_id=?1 AND execution_held=1
                    UNION ALL SELECT 1 FROM registered_child_capacity WHERE identity_id=?1 AND execution_held=1)",
                    [&job.identity_id.0],|row| row.get(0))?;
                if occupied {
                    return Err(StoreError::ExecutionCapacityExceeded);
                }
                self.enforce_registered_capacity(
                    &scope.organization_id,
                    &scope.principal_id,
                    team.as_deref(),
                    "",
                )?;
            }
            self.conn.execute(
                "INSERT INTO registered_child_capacity VALUES(?1,?2,?3,?4,?5,?6,?7)
                ON CONFLICT(run_id,task_id) DO UPDATE SET execution_held=excluded.execution_held",
                params![
                    snapshot.run_id.0,
                    task.task_id.0,
                    job.identity_id.0,
                    scope.organization_id,
                    scope.principal_id,
                    team,
                    held
                ],
            )?;
        }
        Ok(())
    }
}
