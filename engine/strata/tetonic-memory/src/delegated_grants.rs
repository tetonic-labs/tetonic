//! A child job's permission is a reduction of an existing, live parent grant.
//! Allocation, grant derivation and execution remain distinct decisions.
use crate::{ControlPermission, ExecutionGrant, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use tetonic_domain::{AgentJobSpec, AttemptId, AttemptState, ExecutionScope, RunState, TaskId};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegatedGrantRequest {
    pub request_id: String,
    pub grant_id: String,
    pub parent_grant_id: String,
    pub delegation_id: String,
    pub job: AgentJobSpec,
    pub expires_at: i64,
}

/// Derived from current durable work and run truth, never supplied by an employee.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DelegatedGrantLineage {
    pub org_id: String,
    pub team_id: String,
    pub delegation_id: String,
    pub parent_grant_id: String,
    pub parent_work_id: String,
    pub child_work_id: String,
    pub parent_run_id: String,
    pub parent_task_id: TaskId,
    pub parent_attempt_id: AttemptId,
    pub parent_task_version: u64,
    pub parent_lease: tetonic_domain::LeaseProof,
    pub payer_principal_id: String,
    pub stop_scope: String,
    pub allocated_tokens: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DelegatedExecutionGrant {
    pub grant: ExecutionGrant,
    pub lineage: DelegatedGrantLineage,
}

fn deny<T>() -> Result<T> {
    Err(StoreError::ControlAccessDenied)
}

fn bounded_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 128 && !id.contains('\0')
}

fn subset(child: &[String], parent: &[String]) -> bool {
    child.iter().all(|item| parent.contains(item))
}

impl Store {
    pub(crate) fn migrate_delegated_grants_v56(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS execution_grant_lineage (
                grant_id TEXT PRIMARY KEY NOT NULL REFERENCES execution_grants(grant_id),
                parent_grant_id TEXT NOT NULL REFERENCES execution_grants(grant_id),
                org_id TEXT NOT NULL, team_id TEXT NOT NULL, delegation_id TEXT NOT NULL,
                request_id TEXT NOT NULL, request_json TEXT NOT NULL, lineage_json TEXT NOT NULL,
                UNIQUE(org_id,team_id,request_id), UNIQUE(org_id,team_id,delegation_id),
                FOREIGN KEY(org_id,team_id,delegation_id) REFERENCES work_delegations(org_id,team_id,delegation_id)
             );
             CREATE TRIGGER IF NOT EXISTS execution_grant_lineage_immutable BEFORE UPDATE ON execution_grant_lineage
             BEGIN SELECT RAISE(ABORT,'execution grant lineage is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS execution_grant_lineage_no_delete BEFORE DELETE ON execution_grant_lineage
             BEGIN SELECT RAISE(ABORT,'execution grant lineage is durable'); END;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(56,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Trusted lookup used by admission to reject using a child grant as a root.
    pub fn execution_grant_is_delegated(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM execution_grant_lineage WHERE grant_id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    pub(crate) fn delegated_grant_lineage(
        &self,
        id: &str,
    ) -> Result<Option<DelegatedGrantLineage>> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT lineage_json FROM execution_grant_lineage WHERE grant_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(|_| StoreError::ControlAccessDenied))
            .transpose()
    }

    fn live_grant(&self, id: &str, now: i64) -> Result<ExecutionGrant> {
        let value: Option<String> = self.conn.query_row(
            "SELECT payload FROM execution_grants WHERE grant_id=?1 AND revoked_at IS NULL AND expires_at>?2",
            params![id, now], |r| r.get(0),
        ).optional()?;
        let grant: ExecutionGrant =
            serde_json::from_str(&value.ok_or(StoreError::ControlAccessDenied)?)
                .map_err(|_| StoreError::ControlAccessDenied)?;
        if grant.grant_id != id
            || now < 0
            || !self.context_access_in_organization(
                &grant.scope.principal_id,
                &grant.scope.information_context_id,
                &grant.scope.organization_id,
            )?
        {
            return deny();
        }
        let registered: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM organization_agents a
             JOIN agent_definition_revisions d ON d.identity_id=a.identity_id
             WHERE a.org_id=?1 AND a.identity_id=?2 AND d.definition_digest=?3)",
            params![
                grant.scope.organization_id,
                grant.job.identity_id.0,
                grant.job.definition_digest
            ],
            |r| r.get(0),
        )?;
        if !registered {
            return deny();
        }
        Ok(grant)
    }

    /// Only shared team context can be delegated. A private working context is
    /// not silently published just because its owner also belongs to the team.
    fn require_shared_grant_context(
        &self,
        scope: &ExecutionScope,
        org: &str,
        team: &str,
    ) -> Result<()> {
        let shared: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM information_contexts
             WHERE context_id=?1 AND org_id=?2 AND team_id=?3 AND kind='team')",
            params![scope.information_context_id, org, team],
            |r| r.get(0),
        )?;
        if scope.organization_id != org || !shared {
            return deny();
        }
        Ok(())
    }

    fn validate_parent_attempt(
        &self,
        lineage: &DelegatedGrantLineage,
        parent: &ExecutionGrant,
        now: i64,
    ) -> Result<()> {
        let work = self
            .get_team_work_item(&lineage.org_id, &lineage.team_id, &lineage.parent_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if work.status != "running"
            || work.run_id.as_deref() != Some(&lineage.parent_run_id)
            || work.attempt_id.as_deref() != Some(lineage.parent_attempt_id.0.as_str())
        {
            return deny();
        }
        let run = self
            .load_run_snapshot(&lineage.parent_run_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let attempt = run
            .attempts
            .get(&lineage.parent_attempt_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let task = run
            .tasks
            .get(&lineage.parent_task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        if run.state != RunState::Active
            || run.cancellation.run_canceled
            || attempt.state != AttemptState::Running
            || attempt.execution_quiesced
            || !attempt.execution_claimed
            || attempt.task_id != lineage.parent_task_id
            || attempt.task_version != lineage.parent_task_version
            || task.binding.task_definition_version != lineage.parent_task_version
            || attempt.lease.as_ref().map_or(true, |lease| {
                lease.expires_at <= now as u64
                    || lease.attempt_id != lineage.parent_attempt_id
                    || lease.lease_id != lineage.parent_lease.lease_id
                    || lease.lease_epoch != lineage.parent_lease.lease_epoch
                    || lease.holder != lineage.parent_lease.holder
            })
            || task.active_attempt.as_ref() != Some(&lineage.parent_attempt_id)
            || task.binding.execution_scope.as_ref() != Some(&parent.scope)
            || task.binding.execution_grant_id.as_deref() != Some(parent.grant_id.as_str())
            || task.binding.job_spec.as_ref() != Some(&parent.job)
            || task
                .binding
                .deadline
                .is_some_and(|deadline| deadline <= now as u64)
        {
            return deny();
        }
        Ok(())
    }

    /// Caller owns a read/write transaction. Every ancestor remains revocable;
    /// no copied grant can outlive its parent's job, membership or work scope.
    fn validate_delegated_chain(&self, grant: &ExecutionGrant, now: i64) -> Result<()> {
        let mut child = grant.clone();
        let mut visited = Vec::new();
        while let Some(lineage) = self.delegated_grant_lineage(&child.grant_id)? {
            if visited.len() >= 32 || visited.contains(&child.grant_id) {
                return deny();
            }
            visited.push(child.grant_id.clone());
            let parent = self.live_grant(&lineage.parent_grant_id, now)?;
            self.require_shared_grant_context(&child.scope, &lineage.org_id, &lineage.team_id)?;
            if child.scope != parent.scope
                || child.expires_at > parent.expires_at
                || !subset(
                    &child.job.capability_bindings,
                    &parent.job.capability_bindings,
                )
                || !subset(&child.job.artifact_bindings, &parent.job.artifact_bindings)
            {
                return deny();
            }
            self.validate_parent_attempt(&lineage, &parent, now)?;
            self.require_work_allocation_open(
                &lineage.org_id,
                &lineage.team_id,
                &lineage.child_work_id,
            )?;
            let allocation = self.work_budget_unchecked(
                &lineage.org_id,
                &lineage.team_id,
                &lineage.child_work_id,
            )?;
            let delegation = self
                .get_work_delegation(&lineage.org_id, &lineage.team_id, &lineage.delegation_id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if allocation.payer_principal_id != lineage.payer_principal_id
                || allocation.stop_scope != lineage.stop_scope
                || allocation.token_limit != lineage.allocated_tokens
                || delegation.child_work_id != lineage.child_work_id
                || delegation.parent_work_id != lineage.parent_work_id
            {
                return deny();
            }
            if let Some(parent_lineage) = self.delegated_grant_lineage(&parent.grant_id)? {
                if parent_lineage.child_work_id != lineage.parent_work_id
                    || parent_lineage.org_id != lineage.org_id
                    || parent_lineage.team_id != lineage.team_id
                {
                    return deny();
                }
            }
            child = parent;
        }
        Ok(())
    }

    /// Explicit manager decision for one allocated child, from a currently live
    /// parent job. Caller cannot choose the principal, context, payer or lineage.
    pub fn derive_execution_grant(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        request: &DelegatedGrantRequest,
        now: i64,
    ) -> Result<DelegatedExecutionGrant> {
        let request_json =
            serde_json::to_string(request).map_err(|_| StoreError::ControlResourceConflict)?;
        if request_json.len() > 65536
            || now < 0
            || request.expires_at <= now
            || [
                &request.request_id,
                &request.grant_id,
                &request.parent_grant_id,
                &request.delegation_id,
            ]
            .iter()
            .any(|id| !bounded_id(id))
            || request.grant_id == request.parent_grant_id
        {
            return Err(StoreError::InvalidControlResource("delegated grant".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return deny();
        }
        let parent = self.live_grant(&request.parent_grant_id, now)?;
        self.require_shared_grant_context(&parent.scope, org, team)?;
        // The initiating principal remains accountable. Another manager cannot
        // quietly turn that person's existing root grant into delegation authority.
        if parent.scope.principal_id != actor
            || request.expires_at > parent.expires_at
            || !subset(
                &request.job.capability_bindings,
                &parent.job.capability_bindings,
            )
            || !subset(
                &request.job.artifact_bindings,
                &parent.job.artifact_bindings,
            )
        {
            return deny();
        }
        self.validate_delegated_chain(&parent, now)?;
        let existing: Option<String> = self.conn.query_row(
            "SELECT request_json FROM execution_grant_lineage WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
            params![org, team, request.request_id], |r| r.get(0),
        ).optional()?;
        if let Some(existing) = existing {
            if existing != request_json {
                return Err(StoreError::ControlResourceConflict);
            }
            let grant = self.live_grant(&request.grant_id, now)?;
            self.validate_delegated_chain(&grant, now)?;
            let lineage = self
                .delegated_grant_lineage(&grant.grant_id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            tx.commit()?;
            return Ok(DelegatedExecutionGrant { grant, lineage });
        }
        // A root grant ID must never be converted into a derived grant (or vice versa).
        let occupied: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM execution_grants WHERE grant_id=?1)",
            [&request.grant_id],
            |r| r.get(0),
        )?;
        if occupied {
            return Err(StoreError::ControlResourceConflict);
        }
        let delegation = self
            .get_work_delegation(org, team, &request.delegation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        self.require_work_allocation_open(org, team, &delegation.child_work_id)?;
        let child_work = self
            .get_team_work_item(org, team, &delegation.child_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if child_work.status != "open"
            || child_work.run_id.is_some()
            || child_work.attempt_id.is_some()
        {
            return deny();
        }
        let allocation = self.work_budget_unchecked(org, team, &delegation.child_work_id)?;
        let work = self
            .get_team_work_item(org, team, &delegation.parent_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let run_id = work.run_id.ok_or(StoreError::ControlAccessDenied)?;
        let attempt_id = AttemptId::new(work.attempt_id.ok_or(StoreError::ControlAccessDenied)?);
        let run = self
            .load_run_snapshot(&run_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let attempt = run
            .attempts
            .get(&attempt_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let task_id = attempt.task_id.clone();
        let lease = attempt
            .lease
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let lineage = DelegatedGrantLineage {
            org_id: org.into(),
            team_id: team.into(),
            delegation_id: request.delegation_id.clone(),
            parent_grant_id: request.parent_grant_id.clone(),
            parent_work_id: delegation.parent_work_id,
            child_work_id: delegation.child_work_id,
            parent_run_id: run_id,
            parent_task_id: task_id,
            parent_attempt_id: attempt_id,
            parent_task_version: attempt.task_version,
            parent_lease: tetonic_domain::LeaseProof {
                lease_id: lease.lease_id.clone(),
                lease_epoch: lease.lease_epoch,
                holder: lease.holder.clone(),
            },
            payer_principal_id: allocation.payer_principal_id,
            stop_scope: allocation.stop_scope,
            allocated_tokens: allocation.token_limit,
        };
        self.validate_parent_attempt(&lineage, &parent, now)?;
        let grant = ExecutionGrant {
            grant_id: request.grant_id.clone(),
            scope: parent.scope,
            job: request.job.clone(),
            expires_at: request.expires_at,
        };
        let payload =
            serde_json::to_string(&grant).map_err(|_| StoreError::ControlResourceConflict)?;
        // Foreign keys pin the child to an existing registered definition revision.
        self.insert_execution_grant(actor, &grant, &payload, now, "derive")?;
        self.conn.execute(
            "INSERT INTO execution_grant_lineage VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                request.grant_id,
                request.parent_grant_id,
                org,
                team,
                request.delegation_id,
                request.request_id,
                request_json,
                serde_json::to_string(&lineage).map_err(|_| StoreError::ControlResourceConflict)?
            ],
        )?;
        self.live_grant(&grant.grant_id, now)?;
        self.validate_delegated_chain(&grant, now)?;
        tx.commit()?;
        Ok(DelegatedExecutionGrant { grant, lineage })
    }

    /// Parent proof comes from the trusted managed host, not a tool argument.
    /// Resolve child delivery against the immutable grant lineage and allocation.
    /// The runtime additionally requires a live parent handle from its registry.
    pub fn require_delegated_execution_binding(
        &self,
        id: &str,
        scope: &ExecutionScope,
        job: &AgentJobSpec,
        parent_run: &str,
        parent_attempt: &str,
        request: &str,
        now: i64,
    ) -> Result<DelegatedGrantLineage> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let lineage = self
            .delegated_grant_lineage(id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if lineage.parent_run_id != parent_run || lineage.parent_attempt_id.0 != parent_attempt {
            return deny();
        }
        let grant = self.live_grant(id, now)?;
        if grant.scope != *scope || grant.job != *job {
            return deny();
        }
        self.validate_delegated_chain(&grant, now)?;
        self.require_work_allocation_open(
            &lineage.org_id,
            &lineage.team_id,
            &lineage.child_work_id,
        )?;
        self.require_bounded_work_reports(
            &lineage.org_id,
            &lineage.team_id,
            &lineage.child_work_id,
        )?;
        let work = self
            .get_team_work_item(&lineage.org_id, &lineage.team_id, &lineage.child_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if crate::work_activation_request_id(&work.request_id) != request
            || work.run_id.as_deref().is_some_and(|run| run != parent_run)
        {
            return deny();
        }
        tx.commit()?;
        Ok(lineage)
    }

    /// Parent proof comes from the trusted managed host, not a tool argument.
    /// This checks permission only; the manager must still bind/reserve execution.
    pub fn delegated_execution_grant_allows(
        &self,
        id: &str,
        scope: &ExecutionScope,
        job: &AgentJobSpec,
        parent_run: &str,
        parent_attempt: &str,
        now: i64,
    ) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let check = || -> Result<()> {
            let lineage = self
                .delegated_grant_lineage(id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if lineage.parent_run_id != parent_run || lineage.parent_attempt_id.0 != parent_attempt
            {
                return deny();
            }
            let grant = self.live_grant(id, now)?;
            if grant.scope != *scope || grant.job != *job {
                return deny();
            }
            self.validate_delegated_chain(&grant, now)
        };
        let allowed = match check() {
            Ok(()) => true,
            Err(StoreError::ControlAccessDenied) => false,
            Err(error) => return Err(error),
        };
        tx.commit()?;
        Ok(allowed)
    }
}

#[cfg(test)]
#[path = "delegated_grants_tests.rs"]
mod tests;
