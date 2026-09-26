//! Durable organization admission policy. Counts are derived from the run
//! projection; these limits are not a second reservation or usage ledger.
use crate::{ControlPermission, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OrganizationExecutionLimits {
    pub revision: u64,
    pub max_active_runs: u32,
    pub max_active_runs_per_principal: u32,
}

/// Concurrent admitted runs for one team. This is not cumulative token spend.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TeamExecutionLimits {
    pub revision: u64,
    pub max_active_runs: u32,
}

impl Store {
    /// Reject preparation when an organization, principal, or team ceiling is
    /// already full. The run-command transaction remains authoritative if two
    /// callers pass this read together.
    pub fn preflight_registered_capacity(
        &self,
        org: &str,
        principal: &str,
        context: &str,
    ) -> Result<()> {
        let team = self.team_for_execution_context(context)?;
        self.enforce_registered_capacity(org, principal, team.as_deref(), "")
    }

    /// Reject a new held registered run when an organization, principal, or team
    /// ceiling is already full. The run-command transaction remains authoritative
    /// if two callers pass this read together.
    pub(crate) fn enforce_registered_capacity(
        &self,
        org: &str,
        principal: &str,
        team: Option<&str>,
        exclude_run: &str,
    ) -> Result<()> {
        let limits = self.execution_limits(org)?;
        let (org_count, principal_count): (u64, u64) = self.conn.query_row(
            "SELECT count(*),coalesce(sum(execution_principal_id=?2),0) FROM run_projections
             WHERE execution_org_id=?1 AND execution_held=1 AND run_id<>?3",
            params![org, principal, exclude_run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if org_count >= u64::from(limits.max_active_runs) {
            return Err(StoreError::OrganizationCapacityExceeded);
        }
        if principal_count >= u64::from(limits.max_active_runs_per_principal) {
            return Err(StoreError::PrincipalCapacityExceeded);
        }
        if let Some(team) = team {
            let team_limits = self.team_execution_limits(org, team)?;
            let team_count: u64 = self.conn.query_row(
                "SELECT count(*) FROM run_projections
                 WHERE execution_org_id=?1 AND execution_team_id=?2 AND execution_held=1 AND run_id<>?3",
                params![org, team, exclude_run],
                |r| r.get(0),
            )?;
            if team_count >= u64::from(team_limits.max_active_runs) {
                return Err(StoreError::TeamCapacityExceeded);
            }
        }
        Ok(())
    }

    /// Internal admission read. A missing policy is an error, never an unlimited
    /// fallback. The caller's run-command transaction serializes policy changes.
    pub(crate) fn execution_limits(&self, org: &str) -> Result<OrganizationExecutionLimits> {
        self.conn.query_row(
            "SELECT revision,max_active_runs,max_active_runs_per_principal FROM organization_execution_limits WHERE org_id=?1",
            [org], |r| Ok(OrganizationExecutionLimits { revision: r.get(0)?, max_active_runs: r.get(1)?, max_active_runs_per_principal: r.get(2)? }),
        ).optional()?.ok_or_else(|| StoreError::InvalidControlResource("organization execution policy missing".into()))
    }

    /// Verified actor only. Reading policy conveys no access to private work.
    pub fn get_execution_limits(
        &self,
        actor: &str,
        org: &str,
    ) -> Result<OrganizationExecutionLimits> {
        if !self.control_access(actor, ControlPermission::ReadOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.execution_limits(org)
    }

    /// Compare-and-set policy and its full before/after audit in one transaction.
    /// Lowering a ceiling blocks new admissions; it never cancels admitted work.
    pub fn set_execution_limits(
        &self,
        actor: &str,
        org: &str,
        expected_revision: u64,
        max_active_runs: u32,
        max_active_runs_per_principal: u32,
    ) -> Result<OrganizationExecutionLimits> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let before = self.execution_limits(org)?;
        if before.revision != expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        if before.max_active_runs == max_active_runs
            && before.max_active_runs_per_principal == max_active_runs_per_principal
        {
            return Ok(before);
        }
        let revision = before
            .revision
            .checked_add(1)
            .filter(|r| *r <= i64::MAX as u64)
            .ok_or(StoreError::ControlResourceConflict)?;
        let after = OrganizationExecutionLimits {
            revision,
            max_active_runs,
            max_active_runs_per_principal,
        };
        self.conn.execute(
            "UPDATE organization_execution_limits SET revision=?2,max_active_runs=?3,max_active_runs_per_principal=?4 WHERE org_id=?1",
            params![org, after.revision, after.max_active_runs, after.max_active_runs_per_principal],
        )?;
        let details = serde_json::json!({"before":before,"after":after}).to_string();
        self.conn.execute(
            "INSERT INTO control_admin_events(actor_kind,actor_principal_id,action,org_id,subject_principal_id,at,details_json)
             VALUES('authenticated_principal',?1,'set_execution_limits',?2,?1,?3,?4)",
            params![actor, org, crate::util::now(), details],
        )?;
        tx.commit()?;
        Ok(after)
    }

    pub(crate) fn team_for_execution_context(&self, context: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT team_id FROM information_contexts WHERE context_id=?1 AND kind='team'",
                [context],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn team_execution_limits(
        &self,
        org: &str,
        team: &str,
    ) -> Result<TeamExecutionLimits> {
        self.conn
            .query_row(
                "SELECT revision,max_active_runs FROM team_execution_limits WHERE org_id=?1 AND team_id=?2",
                params![org, team],
                |r| {
                    Ok(TeamExecutionLimits {
                        revision: r.get(0)?,
                        max_active_runs: r.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidControlResource("team execution policy missing".into()))
    }

    pub fn get_team_execution_limits(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<TeamExecutionLimits> {
        if !self.control_access(actor, ControlPermission::ReadTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.team_execution_limits(org, team)
    }

    /// Compare-and-set a team ceiling. Lowering it blocks new admissions and does
    /// not cancel work already admitted.
    pub fn set_team_execution_limits(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        expected_revision: u64,
        max_active_runs: u32,
    ) -> Result<TeamExecutionLimits> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let before = self.team_execution_limits(org, team)?;
        if before.revision != expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        if before.max_active_runs == max_active_runs {
            return Ok(before);
        }
        let revision = before
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or(StoreError::ControlResourceConflict)?;
        let after = TeamExecutionLimits {
            revision,
            max_active_runs,
        };
        self.conn.execute(
            "UPDATE team_execution_limits SET revision=?3,max_active_runs=?4 WHERE org_id=?1 AND team_id=?2",
            params![org, team, after.revision, after.max_active_runs],
        )?;
        let details = serde_json::json!({"before": before, "after": after}).to_string();
        self.conn.execute(
            "INSERT INTO control_admin_events(actor_kind,actor_principal_id,action,org_id,subject_principal_id,at,details_json)
             VALUES('authenticated_principal',?1,'set_team_execution_limits',?2,?1,?3,?4)",
            params![actor, org, crate::util::now(), details],
        )?;
        tx.commit()?;
        Ok(after)
    }
}
