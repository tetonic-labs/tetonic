//! Durable allocation admission for team work. These records reserve capacity;
//! they are not execution grants, provider usage, or evidence of completed work.
use crate::{ControlPermission, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkBudget {
    pub org_id: String,
    pub team_id: String,
    pub work_id: String,
    pub root_work_id: String,
    pub payer_principal_id: String,
    pub stop_scope: String,
    pub token_limit: i64,
    pub delegated_tokens: i64,
    pub reserved_tokens: i64,
    pub available_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkBudgetReservation {
    pub work_id: String,
    pub request_id: String,
    pub tokens: i64,
    pub created_by: String,
}

fn validate(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > 128 {
        return Err(StoreError::InvalidControlResource(
            "budget identifier".into(),
        ));
    }
    Ok(())
}

impl Store {
    pub fn work_budget_if_present(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Option<WorkBudget>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        self.require_team_participant(actor, org, team)?;
        let ancestors = self.work_ancestors(org, team, work)?;
        let present:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM work_budget_envelopes WHERE org_id=?1 AND team_id=?2 AND work_id=?3)",params![org,team,ancestors.last()],|r|r.get(0))?;
        let budget = if present {
            Some(self.work_budget_unchecked(org, team, work)?)
        } else {
            None
        };
        tx.commit()?;
        Ok(budget)
    }
    pub(crate) fn migrate_work_budgets_v55(&self) -> Result<()> {
        // Called in the store's schema migration transaction. Existing delegation
        // numbers are never promoted into authority by migration.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS work_budget_envelopes (
                org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
                token_limit INTEGER NOT NULL CHECK(token_limit > 0),
                payer_principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
                request_id TEXT NOT NULL, created_at TEXT NOT NULL,
                PRIMARY KEY(org_id,team_id,work_id),
                UNIQUE(org_id,team_id,request_id),
                FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id)
             );
             CREATE TABLE IF NOT EXISTS work_budget_reservations (
                org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
                request_id TEXT NOT NULL, tokens INTEGER NOT NULL CHECK(tokens > 0),
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                PRIMARY KEY(org_id,team_id,request_id),
                FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id)
             );
             CREATE INDEX IF NOT EXISTS work_budget_reservations_work
                ON work_budget_reservations(org_id,team_id,work_id);",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions(version,applied_at) VALUES(55,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Explicit, immutable root allowance. Only a team manager can fund work.
    /// Children inherit an allocation; they cannot open a new payer or allowance.
    /// No automatic backfill for legacy or already running work.
    pub fn authorize_work_budget(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        request: &str,
        tokens: i64,
    ) -> Result<WorkBudget> {
        validate(work)?;
        validate(request)?;
        if tokens <= 0 {
            return Err(StoreError::InvalidControlResource("token_limit".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let existing: Option<(String, i64, String)> = self
            .conn
            .query_row(
                "SELECT work_id,token_limit,payer_principal_id FROM work_budget_envelopes
             WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((old_work, old_tokens, payer)) = existing {
            if old_work != work || old_tokens != tokens || payer != actor {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            let parent = self
                .get_team_work_item(org, team, work)?
                .ok_or(StoreError::ControlAccessDenied)?;
            self.require_work_allocation_open(org, team, work)?;
            let has_lineage: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM work_delegations WHERE org_id=?1 AND team_id=?2
                 AND (parent_work_id=?3 OR child_work_id=?3))",
                params![org, team, work],
                |r| r.get(0),
            )?;
            let already_funded: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM work_budget_envelopes
                 WHERE org_id=?1 AND team_id=?2 AND work_id=?3)",
                params![org, team, work],
                |r| r.get(0),
            )?;
            if has_lineage
                || already_funded
                || parent.status != "open"
                || parent.run_id.is_some()
                || parent.attempt_id.is_some()
            {
                return Err(StoreError::ControlResourceConflict);
            }
            self.conn.execute(
                "INSERT INTO work_budget_envelopes
                 (org_id,team_id,work_id,token_limit,payer_principal_id,request_id,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![org, team, work, tokens, actor, request, crate::util::now()],
            )?;
        }
        let budget = self.work_budget_unchecked(org, team, work)?;
        tx.commit()?;
        Ok(budget)
    }

    pub fn work_budget(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<WorkBudget> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        self.require_team_participant(actor, org, team)?;
        let budget = self.work_budget_unchecked(org, team, work)?;
        tx.commit()?;
        Ok(budget)
    }

    /// Reserve the parent's own effort from the same allowance as child work.
    /// A host must call this before the effect, with a stable operation identity.
    /// Missing usage, failure, cancellation and restart do not refund reservations.
    /// Settlement/release needs a separate proof of quiescence and is not exposed.
    pub fn reserve_work_budget(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        request: &str,
        tokens: i64,
    ) -> Result<WorkBudgetReservation> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let receipt = self.reserve_work_budget_in_tx(actor, org, team, work, request, tokens)?;
        tx.commit()?;
        Ok(receipt)
    }

    pub(crate) fn reserve_work_budget_in_tx(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        request: &str,
        tokens: i64,
    ) -> Result<WorkBudgetReservation> {
        validate(work)?;
        validate(request)?;
        if tokens <= 0 {
            return Err(StoreError::InvalidControlResource("tokens".into()));
        }
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let existing = self
            .conn
            .query_row(
                "SELECT work_id,request_id,tokens,created_by FROM work_budget_reservations
             WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request],
                |r| {
                    Ok(WorkBudgetReservation {
                        work_id: r.get(0)?,
                        request_id: r.get(1)?,
                        tokens: r.get(2)?,
                        created_by: r.get(3)?,
                    })
                },
            )
            .optional()?;
        let receipt = WorkBudgetReservation {
            work_id: work.into(),
            request_id: request.into(),
            tokens,
            created_by: actor.into(),
        };
        if let Some(existing) = existing {
            if existing != receipt {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            self.require_work_allocation_open(org, team, work)?;
            self.require_bounded_work_reports(org, team, work)?;
            if self
                .work_budget_unchecked(org, team, work)?
                .available_tokens
                < tokens
            {
                return Err(StoreError::InvalidControlResource(
                    "work budget exhausted".into(),
                ));
            }
            self.conn.execute(
                "INSERT INTO work_budget_reservations
                 (org_id,team_id,work_id,request_id,tokens,created_by,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![org, team, work, request, tokens, actor, crate::util::now()],
            )?;
        }
        Ok(receipt)
    }

    /// Scoped, bounded lineage walk. Ambiguous legacy parents or cycles deny.
    pub(crate) fn work_ancestors(&self, org: &str, team: &str, work: &str) -> Result<Vec<String>> {
        let mut ancestors = Vec::new();
        let mut next = work.to_string();
        loop {
            if ancestors.len() >= 32 || ancestors.contains(&next) {
                return Err(StoreError::ControlAccessDenied);
            }
            ancestors.push(next.clone());
            let mut stmt = self.conn.prepare(
                "SELECT parent_work_id FROM work_delegations
                 WHERE org_id=?1 AND team_id=?2 AND child_work_id=?3 LIMIT 2",
            )?;
            let parents = stmt
                .query_map(params![org, team, next], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            match parents.as_slice() {
                [] => return Ok(ancestors),
                [parent] => next = parent.clone(),
                _ => return Err(StoreError::ControlAccessDenied),
            }
        }
    }

    pub(crate) fn require_work_allocation_open(
        &self,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<()> {
        for id in self.work_ancestors(org, team, work)? {
            let work = self
                .get_team_work_item(org, team, &id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if !matches!(work.status.as_str(), "open" | "running")
                || self
                    .activation_blocked_by_stop(org, team, &id, work.goal_id.as_deref(), None)?
                    .is_some()
            {
                return Err(StoreError::ControlAccessDenied);
            }
        }
        Ok(())
    }

    /// Caller owns a transaction and has checked authorization.
    pub(crate) fn work_budget_unchecked(
        &self,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<WorkBudget> {
        let ancestors = self.work_ancestors(org, team, work)?;
        let root = ancestors.last().ok_or(StoreError::ControlAccessDenied)?;
        let (root_limit, payer): (i64, String) = self
            .conn
            .query_row(
                "SELECT token_limit,payer_principal_id FROM work_budget_envelopes
             WHERE org_id=?1 AND team_id=?2 AND work_id=?3",
                params![org, team, root],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(StoreError::ControlAccessDenied)?;
        let stop_scope = format!("work/{root}");
        let mut limit = root_limit;
        for id in ancestors.iter().rev().skip(1) {
            let delegation = self
                .work_delegation_for_child(org, team, id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if delegation.parent_budget_tokens != limit
                || delegation.child_budget_tokens <= 0
                || delegation.child_budget_tokens > limit
                || delegation.payer_principal_id != payer
                || delegation.stop_scope != stop_scope
            {
                return Err(StoreError::ControlAccessDenied);
            }
            limit = delegation.child_budget_tokens;
        }
        let delegated: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(child_budget_tokens),0) FROM work_delegations
             WHERE org_id=?1 AND team_id=?2 AND parent_work_id=?3",
            params![org, team, work],
            |r| r.get(0),
        )?;
        let reserved: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(tokens),0) - (SELECT COALESCE(SUM(released_tokens),0)
             FROM work_budget_executions WHERE org_id=?1 AND team_id=?2 AND work_id=?3)
             FROM work_budget_reservations WHERE org_id=?1 AND team_id=?2 AND work_id=?3",
            params![org, team, work],
            |r| r.get(0),
        )?;
        let available = limit
            .checked_sub(delegated)
            .and_then(|v| v.checked_sub(reserved))
            .filter(|v| *v >= 0 && delegated >= 0 && reserved >= 0)
            .ok_or(StoreError::ControlAccessDenied)?;
        Ok(WorkBudget {
            org_id: org.into(),
            team_id: team.into(),
            work_id: work.into(),
            root_work_id: root.clone(),
            payer_principal_id: payer,
            stop_scope,
            token_limit: limit,
            delegated_tokens: delegated,
            reserved_tokens: reserved,
            available_tokens: available,
        })
    }
}

#[cfg(test)]
#[path = "work_budgets_tests.rs"]
mod tests;
