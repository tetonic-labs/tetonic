//! Hierarchical stop scopes, effect approvals and team effort (MVP-401/402).
//! These sit on the control store with team work — not a second lifecycle authority.

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ControlStop {
    pub org_id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub mode: String,
    pub generation: i64,
    pub reason: String,
    pub created_by: String,
    pub cleared_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EffectApproval {
    pub org_id: String,
    pub team_id: String,
    pub approval_id: String,
    pub work_id: Option<String>,
    pub proposal_digest: String,
    pub status: String,
    pub request_id: String,
    pub expires_at: i64,
    pub created_by: String,
    pub resolved_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TeamEffortEntry {
    pub org_id: String,
    pub team_id: String,
    pub goal_id: Option<String>,
    pub work_id: Option<String>,
    pub entry_id: String,
    pub request_id: String,
    /// Measured provider usage. Absent means unknown, never treated as zero.
    pub measured_tokens: Option<i64>,
    pub status: String,
    pub created_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TeamWorkInspection {
    pub org_id: String,
    pub team_id: String,
    pub active_stops: Vec<ControlStop>,
    pub work_items: Vec<crate::TeamWorkItem>,
    pub effort: Vec<TeamEffortEntry>,
    pub pending_approvals: Vec<EffectApproval>,
}

fn validate_id(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > 128 {
        return Err(StoreError::InvalidControlResource(field.into()));
    }
    Ok(())
}

fn validate_mode(mode: &str) -> Result<()> {
    match mode {
        "pause" | "cancel" | "estop" => Ok(()),
        _ => Err(StoreError::InvalidControlResource("mode".into())),
    }
}

fn validate_scope_kind(kind: &str) -> Result<()> {
    match kind {
        "org" | "team" | "goal" | "work" | "agent" => Ok(()),
        _ => Err(StoreError::InvalidControlResource("scope_kind".into())),
    }
}

impl Store {
    pub(crate) fn migrate_human_controls_v49(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=49)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS control_stop_scopes (
                org_id TEXT NOT NULL,
                scope_kind TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                generation INTEGER NOT NULL,
                reason TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                cleared_at TEXT,
                PRIMARY KEY (org_id, scope_kind, scope_id, generation)
             );
             CREATE INDEX IF NOT EXISTS idx_control_stops_active
                ON control_stop_scopes(org_id, scope_kind, scope_id)
                WHERE cleared_at IS NULL;
             CREATE TABLE IF NOT EXISTS effect_approvals (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                approval_id TEXT NOT NULL,
                work_id TEXT,
                proposal_digest TEXT NOT NULL,
                status TEXT NOT NULL,
                request_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                resolved_by TEXT REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, approval_id),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id),
                UNIQUE (org_id, team_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS team_effort_entries (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                goal_id TEXT,
                work_id TEXT,
                entry_id TEXT NOT NULL,
                request_id TEXT NOT NULL,
                measured_tokens INTEGER,
                status TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, entry_id),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id),
                UNIQUE (org_id, team_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS control_stop_unresolved (
                org_id TEXT NOT NULL,
                scope_kind TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                generation INTEGER NOT NULL,
                effect_id TEXT NOT NULL,
                detail TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, scope_kind, scope_id, generation, effect_id)
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(49,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Record a hierarchical stop. New descendants under this scope must deny.
    pub fn request_control_stop(
        &self,
        actor: &str,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
        mode: &str,
        reason: &str,
    ) -> Result<ControlStop> {
        validate_id(org, "org_id")?;
        validate_id(scope_id, "scope_id")?;
        validate_scope_kind(scope_kind)?;
        validate_mode(mode)?;
        if reason.trim().is_empty() || reason.len() > 512 || reason.contains('\0') {
            return Err(StoreError::InvalidControlResource("reason".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_stop_authority(actor, org, scope_kind, scope_id)?;
        let generation: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(generation),0)+1 FROM control_stop_scopes
             WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3",
            params![org, scope_kind, scope_id],
            |r| r.get(0),
        )?;
        // Clear prior active stops on this exact scope before inserting the new generation.
        self.conn.execute(
            "UPDATE control_stop_scopes SET cleared_at=?4
             WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3 AND cleared_at IS NULL",
            params![org, scope_kind, scope_id, crate::util::now()],
        )?;
        self.conn.execute(
            "INSERT INTO control_stop_scopes(
                org_id,scope_kind,scope_id,mode,generation,reason,created_by,created_at,cleared_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL)",
            params![
                org,
                scope_kind,
                scope_id,
                mode,
                generation,
                reason,
                actor,
                crate::util::now()
            ],
        )?;
        if matches!(mode, "pause" | "cancel" | "estop") {
            self.park_work_under_stop(org, scope_kind, scope_id)?;
        }
        let row = self
            .active_control_stop(org, scope_kind, scope_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn require_stop_authority(
        &self,
        actor: &str,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<()> {
        match scope_kind {
            "org" => {
                if scope_id != org {
                    return Err(StoreError::InvalidControlResource("scope_id".into()));
                }
                if !self.control_access(
                    actor,
                    crate::ControlPermission::ManageOrganization,
                    org,
                    "",
                )? {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
            "team" => self.require_team_participant(actor, org, scope_id)?,
            "goal" | "work" => self.require_org_member(actor, org)?,
            "agent" => {
                if !self.control_access(
                    actor,
                    crate::ControlPermission::ManageOrganization,
                    org,
                    "",
                )? {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
            _ => return Err(StoreError::InvalidControlResource("scope_kind".into())),
        }
        Ok(())
    }

    fn require_org_member(&self, actor: &str, org: &str) -> Result<()> {
        let enabled: bool = self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM organization_members m
                JOIN control_principals p ON p.principal_id=m.principal_id
                WHERE m.org_id=?1 AND m.principal_id=?2 AND p.enabled=1
             )",
            params![org, actor],
            |r| r.get(0),
        )?;
        if enabled {
            Ok(())
        } else {
            Err(StoreError::ControlAccessDenied)
        }
    }

    fn park_work_under_stop(&self, org: &str, scope_kind: &str, scope_id: &str) -> Result<()> {
        match scope_kind {
            "org" => {
                self.conn.execute(
                    "UPDATE team_work_items
                     SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
                     WHERE org_id=?1 AND status IN ('open','running')",
                    params![org],
                )?;
            }
            "team" => {
                self.conn.execute(
                    "UPDATE team_work_items
                     SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
                     WHERE org_id=?1 AND team_id=?2 AND status IN ('open','running')",
                    params![org, scope_id],
                )?;
            }
            "goal" => {
                self.conn.execute(
                    "UPDATE team_work_items
                     SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
                     WHERE org_id=?1 AND goal_id=?2 AND status IN ('open','running')",
                    params![org, scope_id],
                )?;
            }
            "work" => {
                self.conn.execute(
                    "UPDATE team_work_items
                     SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
                     WHERE org_id=?1 AND work_id=?2 AND status IN ('open','running')",
                    params![org, scope_id],
                )?;
            }
            "agent" => {}
            _ => {}
        }
        Ok(())
    }

    pub fn active_control_stop(
        &self,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<Option<ControlStop>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,scope_kind,scope_id,mode,generation,reason,created_by,cleared_at
                 FROM control_stop_scopes
                 WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3 AND cleared_at IS NULL
                 ORDER BY generation DESC LIMIT 1",
                params![org, scope_kind, scope_id],
                |r| {
                    Ok(ControlStop {
                        org_id: r.get(0)?,
                        scope_kind: r.get(1)?,
                        scope_id: r.get(2)?,
                        mode: r.get(3)?,
                        generation: r.get(4)?,
                        reason: r.get(5)?,
                        created_by: r.get(6)?,
                        cleared_at: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// True when any ancestor stop blocks new admission/activation under this work.
    pub fn activation_blocked_by_stop(
        &self,
        org: &str,
        team: &str,
        work_id: &str,
        goal_id: Option<&str>,
        agent_key: Option<&str>,
    ) -> Result<Option<ControlStop>> {
        if let Some(stop) = self.active_control_stop(org, "org", org)? {
            return Ok(Some(stop));
        }
        if let Some(stop) = self.active_control_stop(org, "team", team)? {
            return Ok(Some(stop));
        }
        if let Some(goal) = goal_id {
            if let Some(stop) = self.active_control_stop(org, "goal", goal)? {
                return Ok(Some(stop));
            }
        }
        if let Some(stop) = self.active_control_stop(org, "work", work_id)? {
            return Ok(Some(stop));
        }
        if let Some(agent) = agent_key {
            if let Some(stop) = self.active_control_stop(org, "agent", agent)? {
                return Ok(Some(stop));
            }
        }
        Ok(None)
    }

    pub fn clear_control_stop(
        &self,
        actor: &str,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<ControlStop> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_stop_authority(actor, org, scope_kind, scope_id)?;
        let updated = self.conn.execute(
            "UPDATE control_stop_scopes SET cleared_at=?4
             WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3 AND cleared_at IS NULL",
            params![org, scope_kind, scope_id, crate::util::now()],
        )?;
        if updated == 0 {
            return Err(StoreError::ControlAccessDenied);
        }
        let row = self
            .conn
            .query_row(
                "SELECT org_id,scope_kind,scope_id,mode,generation,reason,created_by,cleared_at
                 FROM control_stop_scopes
                 WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3
                 ORDER BY generation DESC LIMIT 1",
                params![org, scope_kind, scope_id],
                |r| {
                    Ok(ControlStop {
                        org_id: r.get(0)?,
                        scope_kind: r.get(1)?,
                        scope_id: r.get(2)?,
                        mode: r.get(3)?,
                        generation: r.get(4)?,
                        reason: r.get(5)?,
                        created_by: r.get(6)?,
                        cleared_at: r.get(7)?,
                    })
                },
            )?;
        tx.commit()?;
        Ok(row)
    }

    pub fn list_active_stops_for_team(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<ControlStop>> {
        self.require_team_participant(actor, org, team)?;
        let mut stops = Vec::new();
        for (kind, id) in [
            ("org", org),
            ("team", team),
        ] {
            if let Some(stop) = self.active_control_stop(org, kind, id)? {
                stops.push(stop);
            }
        }
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT goal_id FROM team_work_items
             WHERE org_id=?1 AND team_id=?2 AND goal_id IS NOT NULL",
        )?;
        let goals: Vec<String> = stmt
            .query_map(params![org, team], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for goal in goals {
            if let Some(stop) = self.active_control_stop(org, "goal", &goal)? {
                stops.push(stop);
            }
        }
        Ok(stops)
    }

    pub fn record_unresolved_stop_effect(
        &self,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
        generation: i64,
        effect_id: &str,
        detail: &str,
    ) -> Result<()> {
        validate_id(effect_id, "effect_id")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO control_stop_unresolved(
                org_id,scope_kind,scope_id,generation,effect_id,detail,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                scope_kind,
                scope_id,
                generation,
                effect_id,
                detail,
                crate::util::now()
            ],
        )?;
        Ok(())
    }

    pub fn list_unresolved_stop_effects(
        &self,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
        generation: i64,
    ) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT effect_id, detail FROM control_stop_unresolved
             WHERE org_id=?1 AND scope_kind=?2 AND scope_id=?3 AND generation=?4
             ORDER BY effect_id",
        )?;
        let rows = stmt
            .query_map(params![org, scope_kind, scope_id, generation], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Propose an effect approval. Retries with the same request_id are idempotent.
    /// A changed proposal digest on the same request conflicts.
    pub fn propose_effect_approval(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        approval_id: &str,
        proposal_digest: &str,
        request_id: &str,
        expires_at: i64,
        work_id: Option<&str>,
    ) -> Result<EffectApproval> {
        validate_id(org, "org_id")?;
        validate_id(team, "team_id")?;
        validate_id(approval_id, "approval_id")?;
        validate_id(request_id, "request_id")?;
        validate_id(proposal_digest, "proposal_digest")?;
        if expires_at <= 0 {
            return Err(StoreError::InvalidControlResource("expires_at".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if let Some(existing) = self.effect_approval_by_request(org, team, request_id)? {
            if existing.approval_id != approval_id
                || existing.proposal_digest != proposal_digest
            {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        self.conn.execute(
            "INSERT INTO effect_approvals(
                org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                expires_at,created_by,resolved_by,created_at
             ) VALUES(?1,?2,?3,?4,?5,'pending',?6,?7,?8,NULL,?9)",
            params![
                org,
                team,
                approval_id,
                work_id,
                proposal_digest,
                request_id,
                expires_at,
                actor,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn effect_approval_by_request(
        &self,
        org: &str,
        team: &str,
        request_id: &str,
    ) -> Result<Option<EffectApproval>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                        expires_at,created_by,resolved_by
                 FROM effect_approvals WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request_id],
                Self::map_effect_approval,
            )
            .optional()?)
    }

    pub fn get_effect_approval(
        &self,
        org: &str,
        team: &str,
        approval_id: &str,
    ) -> Result<Option<EffectApproval>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                        expires_at,created_by,resolved_by
                 FROM effect_approvals WHERE org_id=?1 AND team_id=?2 AND approval_id=?3",
                params![org, team, approval_id],
                Self::map_effect_approval,
            )
            .optional()?)
    }

    fn map_effect_approval(r: &rusqlite::Row<'_>) -> rusqlite::Result<EffectApproval> {
        Ok(EffectApproval {
            org_id: r.get(0)?,
            team_id: r.get(1)?,
            approval_id: r.get(2)?,
            work_id: r.get(3)?,
            proposal_digest: r.get(4)?,
            status: r.get(5)?,
            request_id: r.get(6)?,
            expires_at: r.get(7)?,
            created_by: r.get(8)?,
            resolved_by: r.get(9)?,
        })
    }

    /// Approve or reject. Rejected/expired approvals never authorize dispatch.
    /// A digest mismatch means the proposal changed and must be re-proposed.
    pub fn resolve_effect_approval(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        approval_id: &str,
        proposal_digest: &str,
        allow: bool,
        now_unix: i64,
    ) -> Result<EffectApproval> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let current = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if current.proposal_digest != proposal_digest {
            return Err(StoreError::ControlResourceConflict);
        }
        if current.status != "pending" {
            if current.status == "approved" && allow
                || current.status == "rejected" && !allow
            {
                tx.commit()?;
                return Ok(current);
            }
            return Err(StoreError::ControlResourceConflict);
        }
        if current.expires_at <= now_unix {
            self.conn.execute(
                "UPDATE effect_approvals SET status='expired', resolved_by=?4
                 WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
                params![org, team, approval_id, actor],
            )?;
            return Err(StoreError::ControlAccessDenied);
        }
        let status = if allow { "approved" } else { "rejected" };
        self.conn.execute(
            "UPDATE effect_approvals SET status=?4, resolved_by=?5
             WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
            params![org, team, approval_id, status, actor],
        )?;
        let row = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Dispatch gate: only an unexpired approved digest may proceed.
    pub fn effect_approval_allows_dispatch(
        &self,
        org: &str,
        team: &str,
        approval_id: &str,
        proposal_digest: &str,
        now_unix: i64,
    ) -> Result<bool> {
        let Some(row) = self.get_effect_approval(org, team, approval_id)? else {
            return Ok(false);
        };
        if row.proposal_digest != proposal_digest {
            return Ok(false);
        }
        if row.status == "approved" && row.expires_at > now_unix {
            return Ok(true);
        }
        if row.status == "pending" && row.expires_at <= now_unix {
            let _ = self.conn.execute(
                "UPDATE effect_approvals SET status='expired'
                 WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
                params![org, team, approval_id],
            );
        }
        Ok(false)
    }

    pub fn record_team_effort(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        entry_id: &str,
        request_id: &str,
        measured_tokens: Option<i64>,
        goal_id: Option<&str>,
        work_id: Option<&str>,
    ) -> Result<TeamEffortEntry> {
        validate_id(entry_id, "entry_id")?;
        validate_id(request_id, "request_id")?;
        if measured_tokens.is_some_and(|v| v < 0) {
            return Err(StoreError::InvalidControlResource("measured_tokens".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if let Some(existing) = self.effort_by_request(org, team, request_id)? {
            if existing.entry_id != entry_id || existing.measured_tokens != measured_tokens {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        let status = if measured_tokens.is_some() {
            "measured"
        } else {
            "unknown"
        };
        self.conn.execute(
            "INSERT INTO team_effort_entries(
                org_id,team_id,goal_id,work_id,entry_id,request_id,measured_tokens,
                status,created_by,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                org,
                team,
                goal_id,
                work_id,
                entry_id,
                request_id,
                measured_tokens,
                status,
                actor,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_team_effort(org, team, entry_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn effort_by_request(
        &self,
        org: &str,
        team: &str,
        request_id: &str,
    ) -> Result<Option<TeamEffortEntry>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,goal_id,work_id,entry_id,request_id,measured_tokens,
                        status,created_by
                 FROM team_effort_entries WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request_id],
                Self::map_effort,
            )
            .optional()?)
    }

    pub fn get_team_effort(
        &self,
        org: &str,
        team: &str,
        entry_id: &str,
    ) -> Result<Option<TeamEffortEntry>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,goal_id,work_id,entry_id,request_id,measured_tokens,
                        status,created_by
                 FROM team_effort_entries WHERE org_id=?1 AND team_id=?2 AND entry_id=?3",
                params![org, team, entry_id],
                Self::map_effort,
            )
            .optional()?)
    }

    fn map_effort(r: &rusqlite::Row<'_>) -> rusqlite::Result<TeamEffortEntry> {
        Ok(TeamEffortEntry {
            org_id: r.get(0)?,
            team_id: r.get(1)?,
            goal_id: r.get(2)?,
            work_id: r.get(3)?,
            entry_id: r.get(4)?,
            request_id: r.get(5)?,
            measured_tokens: r.get(6)?,
            status: r.get(7)?,
            created_by: r.get(8)?,
        })
    }

    pub fn list_team_effort(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<TeamEffortEntry>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt = self.conn.prepare(
            "SELECT org_id,team_id,goal_id,work_id,entry_id,request_id,measured_tokens,
                    status,created_by
             FROM team_effort_entries WHERE org_id=?1 AND team_id=?2
             ORDER BY created_at, entry_id",
        )?;
        let rows = stmt
            .query_map(params![org, team], Self::map_effort)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_pending_effect_approvals(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<EffectApproval>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt = self.conn.prepare(
            "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                    expires_at,created_by,resolved_by
             FROM effect_approvals
             WHERE org_id=?1 AND team_id=?2 AND status='pending'
             ORDER BY created_at, approval_id",
        )?;
        let rows = stmt
            .query_map(params![org, team], Self::map_effect_approval)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// One team view: work, stops, effort and pending approvals. No private bodies.
    pub fn inspect_team_work(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<TeamWorkInspection> {
        self.require_team_participant(actor, org, team)?;
        Ok(TeamWorkInspection {
            org_id: org.into(),
            team_id: team.into(),
            active_stops: self.list_active_stops_for_team(actor, org, team)?,
            work_items: self.list_team_work_items(actor, org, team)?,
            effort: self.list_team_effort(actor, org, team)?,
            pending_approvals: self.list_pending_effect_approvals(actor, org, team)?,
        })
    }

    /// Run ids bound to work under a stop scope — for managed cancel propagation.
    pub fn run_ids_under_stop(
        &self,
        org: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<Vec<String>> {
        let mut stmt = match scope_kind {
            "org" => self.conn.prepare(
                "SELECT DISTINCT run_id FROM team_work_items
                 WHERE org_id=?1 AND run_id IS NOT NULL",
            )?,
            "team" => self.conn.prepare(
                "SELECT DISTINCT run_id FROM team_work_items
                 WHERE org_id=?1 AND team_id=?2 AND run_id IS NOT NULL",
            )?,
            "goal" => self.conn.prepare(
                "SELECT DISTINCT run_id FROM team_work_items
                 WHERE org_id=?1 AND goal_id=?2 AND run_id IS NOT NULL",
            )?,
            "work" => self.conn.prepare(
                "SELECT DISTINCT run_id FROM team_work_items
                 WHERE org_id=?1 AND work_id=?2 AND run_id IS NOT NULL",
            )?,
            _ => return Ok(Vec::new()),
        };
        let id_param = if scope_kind == "org" { org } else { scope_id };
        let rows = if scope_kind == "org" {
            stmt.query_map(params![org], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![org, id_param], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }
}

#[cfg(test)]
#[path = "human_controls_tests.rs"]
mod tests;
