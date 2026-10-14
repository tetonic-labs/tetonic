//! Charge reported overruns to the existing allocation tree. A child's unused
//! allocation stays reserved; only liabilities beyond that allocation spill up.
use crate::{Result, Store, StoreError};
use rusqlite::params;
use std::collections::{HashMap, HashSet};

impl Store {
    /// Per-call admission uses actual spend plus unresolved calls, rather than
    /// treating every future assignment allowance as already spent. Pending or
    /// unknown calls retain the attempt's entire unspent reservation. The caller
    /// records its pending call in the same writer transaction.
    pub(crate) fn work_inference_headroom(
        &self,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Option<i64>> {
        self.work_usage_headroom(org, team, work, true)
    }

    pub(crate) fn work_reported_overrun(&self, org: &str, team: &str, work: &str) -> Result<bool> {
        Ok(self
            .work_usage_headroom(org, team, work, false)?
            .is_some_and(|n| n < 0))
    }

    fn work_usage_headroom(
        &self,
        org: &str,
        team: &str,
        work: &str,
        include_holds: bool,
    ) -> Result<Option<i64>> {
        let ancestors = self.work_ancestors(org, team, work)?;
        let funded: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_budget_envelopes WHERE org_id=?1 AND team_id=?2 AND work_id=?3)",
            params![org, team, ancestors.last()], |r| r.get(0))?;
        if !funded {
            return Ok(None);
        }
        let mut headroom = i64::MAX;
        for id in ancestors {
            let limit = self.work_budget_unchecked(org, team, &id)?.token_limit;
            let committed: i64 = self.conn.query_row(
                "WITH RECURSIVE tree(work_id,depth) AS (
                    SELECT ?3,0 UNION ALL SELECT d.child_work_id,t.depth+1
                    FROM tree t JOIN work_delegations d ON d.parent_work_id=t.work_id
                    AND d.org_id=?1 AND d.team_id=?2 WHERE t.depth<31
                ), charges AS (
                    SELECT e.attempt_id,e.allowance,e.released_tokens,
                        COALESCE(SUM(COALESCE(c.input_tokens,0)+COALESCE(c.output_tokens,0)),0) AS used,
                        COALESCE(SUM(c.state!='reported'),0) AS unresolved
                    FROM work_budget_executions e JOIN tree t ON t.work_id=e.work_id
                    LEFT JOIN work_usage_calls c ON c.attempt_id=e.attempt_id
                    WHERE e.org_id=?1 AND e.team_id=?2 GROUP BY e.attempt_id
                ) SELECT COALESCE(SUM(CASE WHEN unresolved>0 AND ?4 THEN
                    MAX(used,COALESCE(allowance,?5)-released_tokens) ELSE used END),0) FROM charges",
                params![org, team, id, include_holds, limit], |r| r.get(0))?;
            headroom = headroom.min(limit.saturating_sub(committed));
        }
        Ok(Some(headroom))
    }

    /// Caller holds the budget transaction. Returns additional liabilities not
    /// covered by this work's reservations or its direct children's allocations.
    pub(crate) fn work_budget_overflow(&self, org: &str, team: &str, work: &str) -> Result<i64> {
        let mut statement = self.conn.prepare(
            "WITH RECURSIVE tree(work_id,parent_id,allocation,depth) AS (
                SELECT ?3,NULL,0,0 UNION ALL
                SELECT d.child_work_id,d.parent_work_id,d.child_budget_tokens,t.depth+1
                FROM tree t JOIN work_delegations d
                ON d.parent_work_id=t.work_id AND d.org_id=?1 AND d.team_id=?2
                WHERE t.depth<31
            ), usage AS (
                SELECT e.work_id,SUM(MAX(0,COALESCE(c.used,0)-(e.allowance-e.released_tokens))) AS overflow
                FROM work_budget_executions e LEFT JOIN (
                    SELECT attempt_id,SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)) AS used
                    FROM work_usage_calls WHERE attempt_id IN (
                        SELECT attempt_id FROM work_budget_executions
                        WHERE org_id=?1 AND team_id=?2 AND work_id IN (SELECT work_id FROM tree)
                    ) GROUP BY attempt_id
                ) c ON c.attempt_id=e.attempt_id
                WHERE e.org_id=?1 AND e.team_id=?2 AND e.allowance IS NOT NULL GROUP BY e.work_id
            ) SELECT t.work_id,t.parent_id,t.allocation,t.depth,
                COALESCE((SELECT SUM(tokens) FROM work_budget_reservations r
                    WHERE r.org_id=?1 AND r.team_id=?2 AND r.work_id=t.work_id),0)
                - COALESCE((SELECT SUM(released_tokens) FROM work_budget_executions e
                    WHERE e.org_id=?1 AND e.team_id=?2 AND e.work_id=t.work_id),0),
                COALESCE(u.overflow,0),
                COALESCE((SELECT SUM(child_budget_tokens) FROM work_delegations d
                    WHERE d.org_id=?1 AND d.team_id=?2 AND d.parent_work_id=t.work_id),0)
            FROM tree t LEFT JOIN usage u ON u.work_id=t.work_id ORDER BY t.depth DESC",
        )?;
        let rows = statement
            .query_map(params![org, team, work], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, usize>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut seen = HashSet::new();
        let mut overflow = HashMap::<String, i64>::new();
        for (id, parent, allocation, depth, reserved, own_overflow, delegated) in rows {
            if !seen.insert(id.clone())
                || reserved < 0
                || delegated < 0
                || (depth == 31 && delegated > 0)
            {
                return Err(StoreError::ControlAccessDenied);
            }
            let extra = own_overflow
                .checked_add(overflow.remove(&id).unwrap_or(0))
                .ok_or(StoreError::ControlAccessDenied)?;
            let Some(parent) = parent else {
                return Ok(extra);
            };
            if allocation <= 0 {
                return Err(StoreError::ControlAccessDenied);
            }
            let liability = reserved
                .checked_add(delegated)
                .and_then(|n| n.checked_add(extra))
                .ok_or(StoreError::ControlAccessDenied)?;
            let parent_extra = overflow.entry(parent).or_default();
            *parent_extra = parent_extra
                .checked_add(liability.saturating_sub(allocation).max(0))
                .ok_or(StoreError::ControlAccessDenied)?;
        }
        Err(StoreError::ControlAccessDenied)
    }
}
