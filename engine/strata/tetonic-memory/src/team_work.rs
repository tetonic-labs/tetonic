//! Durable team goals, work items and huddle proposals (MVP-301).
//! These sit above managed runs. They do not start inference by themselves.

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TeamGoal {
    pub org_id: String,
    pub team_id: String,
    pub goal_id: String,
    pub title: String,
    pub status: String,
    pub created_by: String,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TeamWorkItem {
    pub org_id: String,
    pub team_id: String,
    pub work_id: String,
    pub goal_id: Option<String>,
    pub title: String,
    pub status: String,
    pub owner_principal_id: Option<String>,
    pub request_id: String,
    pub attempt_id: Option<String>,
    pub run_id: Option<String>,
    pub created_by: String,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuddleProposal {
    pub org_id: String,
    pub team_id: String,
    pub huddle_id: String,
    pub proposal_version: i64,
    pub status: String,
    pub request_id: String,
    pub created_by: String,
    pub work_titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkActivationCursor {
    pub org_id: String,
    pub team_id: String,
    pub source: String,
    pub cursor_key: String,
    pub last_event_id: String,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDelegation {
    pub org_id: String,
    pub team_id: String,
    pub delegation_id: String,
    pub parent_work_id: String,
    pub child_work_id: String,
    pub goal_id: Option<String>,
    pub payer_principal_id: String,
    pub stop_scope: String,
    pub parent_budget_tokens: i64,
    pub child_budget_tokens: i64,
    pub request_id: String,
    pub status: String,
    pub created_by: String,
}

fn validate_id(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > 128 {
        return Err(StoreError::InvalidControlResource(field.into()));
    }
    Ok(())
}

fn validate_title(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > 512 {
        return Err(StoreError::InvalidControlResource("title".into()));
    }
    Ok(())
}

impl Store {
    pub(crate) fn migrate_team_work_v46(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=46)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS team_goals (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                goal_id TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (org_id, team_id, goal_id),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id)
             );
             CREATE TABLE IF NOT EXISTS team_work_items (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                work_id TEXT NOT NULL,
                goal_id TEXT,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                owner_principal_id TEXT REFERENCES control_principals(principal_id),
                request_id TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (org_id, team_id, work_id),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id),
                UNIQUE (org_id, team_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS huddle_proposals (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                huddle_id TEXT NOT NULL,
                proposal_version INTEGER NOT NULL,
                status TEXT NOT NULL,
                request_id TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                work_titles_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, huddle_id, proposal_version),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id),
                UNIQUE (org_id, team_id, request_id)
             );
             CREATE INDEX IF NOT EXISTS idx_team_work_status ON team_work_items(org_id, team_id, status);
             CREATE INDEX IF NOT EXISTS idx_team_goals_status ON team_goals(org_id, team_id, status);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(46,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub(crate) fn migrate_team_work_activation_v47(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=47)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        let has_attempt: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('team_work_items') WHERE name='attempt_id'",
            [],
            |r| r.get(0),
        )?;
        if has_attempt == 0 {
            self.conn
                .execute("ALTER TABLE team_work_items ADD COLUMN attempt_id TEXT", [])?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS work_activation_cursors (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                source TEXT NOT NULL,
                cursor_key TEXT NOT NULL,
                last_event_id TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, source, cursor_key),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id)
             );
             CREATE TABLE IF NOT EXISTS work_delegations (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                delegation_id TEXT NOT NULL,
                parent_work_id TEXT NOT NULL,
                child_work_id TEXT NOT NULL,
                goal_id TEXT,
                payer_principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
                stop_scope TEXT NOT NULL,
                parent_budget_tokens INTEGER NOT NULL,
                child_budget_tokens INTEGER NOT NULL,
                request_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, delegation_id),
                FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, team_id),
                UNIQUE (org_id, team_id, request_id)
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(47,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub(crate) fn migrate_team_work_run_binding_v48(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=48)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        let has_run: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('team_work_items') WHERE name='run_id'",
            [],
            |r| r.get(0),
        )?;
        if has_run == 0 {
            self.conn
                .execute("ALTER TABLE team_work_items ADD COLUMN run_id TEXT", [])?;
        }
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(48,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    fn map_work_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<TeamWorkItem> {
        Ok(TeamWorkItem {
            org_id: r.get(0)?,
            team_id: r.get(1)?,
            work_id: r.get(2)?,
            goal_id: r.get(3)?,
            title: r.get(4)?,
            status: r.get(5)?,
            owner_principal_id: r.get(6)?,
            request_id: r.get(7)?,
            attempt_id: r.get(8)?,
            run_id: r.get(9)?,
            created_by: r.get(10)?,
            version: r.get(11)?,
        })
    }

    pub fn require_team_participant(&self, actor: &str, org: &str, team: &str) -> Result<()> {
        let enabled: bool = self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM organization_members m
                JOIN control_principals p ON p.principal_id=m.principal_id
                WHERE m.org_id=?1 AND m.principal_id=?2 AND p.enabled=1
             )",
            params![org, actor],
            |r| r.get(0),
        )?;
        if !enabled {
            return Err(StoreError::ControlAccessDenied);
        }
        let team_row = self.get_team(org, team)?.ok_or(StoreError::ControlAccessDenied)?;
        if team_row.owner_principal_id == actor {
            return Ok(());
        }
        let member: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM team_members WHERE org_id=?1 AND team_id=?2 AND principal_id=?3)",
            params![org, team, actor],
            |r| r.get(0),
        )?;
        if member {
            Ok(())
        } else {
            Err(StoreError::ControlAccessDenied)
        }
    }

    pub fn create_team_goal(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        goal_id: &str,
        title: &str,
    ) -> Result<TeamGoal> {
        validate_id(org, "org_id")?;
        validate_id(team, "team_id")?;
        validate_id(goal_id, "goal_id")?;
        validate_title(title)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        self.conn.execute(
            "INSERT INTO team_goals(org_id,team_id,goal_id,title,status,created_by,created_at,version)
             VALUES(?1,?2,?3,?4,'open',?5,?6,1)
             ON CONFLICT(org_id,team_id,goal_id) DO NOTHING",
            params![org, team, goal_id, title, actor, crate::util::now()],
        )?;
        let row = self
            .get_team_goal(org, team, goal_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        if row.title != title || row.created_by != actor {
            return Err(StoreError::ControlResourceConflict);
        }
        tx.commit()?;
        Ok(row)
    }

    pub fn get_team_goal(
        &self,
        org: &str,
        team: &str,
        goal_id: &str,
    ) -> Result<Option<TeamGoal>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,goal_id,title,status,created_by,version
                 FROM team_goals WHERE org_id=?1 AND team_id=?2 AND goal_id=?3",
                params![org, team, goal_id],
                |r| {
                    Ok(TeamGoal {
                        org_id: r.get(0)?,
                        team_id: r.get(1)?,
                        goal_id: r.get(2)?,
                        title: r.get(3)?,
                        status: r.get(4)?,
                        created_by: r.get(5)?,
                        version: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    /// Quick task: no huddle required. `request_id` makes retries idempotent.
    pub fn create_team_work_item(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work_id: &str,
        title: &str,
        request_id: &str,
        goal_id: Option<&str>,
    ) -> Result<TeamWorkItem> {
        validate_id(org, "org_id")?;
        validate_id(team, "team_id")?;
        validate_id(work_id, "work_id")?;
        validate_id(request_id, "request_id")?;
        validate_title(title)?;
        if let Some(goal) = goal_id {
            validate_id(goal, "goal_id")?;
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if let Some(goal) = goal_id {
            if self.get_team_goal(org, team, goal)?.is_none() {
                return Err(StoreError::InvalidControlResource("goal_id".into()));
            }
        }
        if let Some(existing) = self.work_item_by_request(org, team, request_id)? {
            if existing.work_id != work_id || existing.title != title {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        self.conn.execute(
            "INSERT INTO team_work_items(
                org_id,team_id,work_id,goal_id,title,status,owner_principal_id,
                request_id,created_by,created_at,version
             ) VALUES(?1,?2,?3,?4,?5,'open',NULL,?6,?7,?8,1)",
            params![
                org,
                team,
                work_id,
                goal_id,
                title,
                request_id,
                actor,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_team_work_item(org, team, work_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn work_item_by_request(
        &self,
        org: &str,
        team: &str,
        request_id: &str,
    ) -> Result<Option<TeamWorkItem>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,work_id,goal_id,title,status,owner_principal_id,request_id,attempt_id,run_id,created_by,version
                 FROM team_work_items WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request_id],
                Self::map_work_item,
            )
            .optional()?)
    }

    pub fn get_team_work_item(
        &self,
        org: &str,
        team: &str,
        work_id: &str,
    ) -> Result<Option<TeamWorkItem>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,work_id,goal_id,title,status,owner_principal_id,request_id,attempt_id,run_id,created_by,version
                 FROM team_work_items WHERE org_id=?1 AND team_id=?2 AND work_id=?3",
                params![org, team, work_id],
                Self::map_work_item,
            )
            .optional()?)
    }

    pub fn park_team_work_item(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work_id: &str,
    ) -> Result<TeamWorkItem> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let updated = self.conn.execute(
            "UPDATE team_work_items SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
             WHERE org_id=?1 AND team_id=?2 AND work_id=?3 AND status IN ('open','running')",
            params![org, team, work_id],
        )?;
        if updated == 0 {
            return Err(StoreError::ControlAccessDenied);
        }
        let row = self
            .get_team_work_item(org, team, work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn list_team_work_items(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<TeamWorkItem>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt = self.conn.prepare(
            "SELECT org_id,team_id,work_id,goal_id,title,status,owner_principal_id,request_id,attempt_id,run_id,created_by,version
             FROM team_work_items WHERE org_id=?1 AND team_id=?2 ORDER BY created_at, work_id",
        )?;
        let rows = stmt
            .query_map(params![org, team], Self::map_work_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Accept a huddle proposal. Creates one open work item per title.
    /// Retries with the same request_id return the same work and do not duplicate.
    pub fn accept_huddle_proposal(
        &self,
        actor: &str,
        proposal: &HuddleProposal,
    ) -> Result<Vec<TeamWorkItem>> {
        validate_id(&proposal.org_id, "org_id")?;
        validate_id(&proposal.team_id, "team_id")?;
        validate_id(&proposal.huddle_id, "huddle_id")?;
        validate_id(&proposal.request_id, "request_id")?;
        if proposal.work_titles.is_empty() || proposal.work_titles.len() > 32 {
            return Err(StoreError::InvalidControlResource("work_titles".into()));
        }
        for title in &proposal.work_titles {
            validate_title(title)?;
        }
        let titles_json = serde_json::to_string(&proposal.work_titles)
            .map_err(|e| StoreError::InvalidControlResource(e.to_string()))?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, &proposal.org_id, &proposal.team_id)?;

        if let Some(existing) = self
            .conn
            .query_row(
                "SELECT status, work_titles_json FROM huddle_proposals
                 WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![proposal.org_id, proposal.team_id, proposal.request_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if existing.0 != "accepted" || existing.1 != titles_json {
                return Err(StoreError::ControlResourceConflict);
            }
            let items = self.list_team_work_items(actor, &proposal.org_id, &proposal.team_id)?;
            let prefix = format!("huddle/{}", proposal.huddle_id);
            let matched: Vec<_> = items
                .into_iter()
                .filter(|w| w.work_id.starts_with(&prefix))
                .collect();
            tx.commit()?;
            return Ok(matched);
        }

        self.conn.execute(
            "INSERT INTO huddle_proposals(
                org_id,team_id,huddle_id,proposal_version,status,request_id,
                created_by,work_titles_json,created_at
             ) VALUES(?1,?2,?3,?4,'accepted',?5,?6,?7,?8)",
            params![
                proposal.org_id,
                proposal.team_id,
                proposal.huddle_id,
                proposal.proposal_version,
                proposal.request_id,
                actor,
                titles_json,
                crate::util::now()
            ],
        )?;

        let mut created = Vec::new();
        for (index, title) in proposal.work_titles.iter().enumerate() {
            let work_id = format!("huddle/{}/{}", proposal.huddle_id, index + 1);
            let request_id = format!("{}/{}", proposal.request_id, index + 1);
            self.conn.execute(
                "INSERT INTO team_work_items(
                    org_id,team_id,work_id,goal_id,title,status,owner_principal_id,
                    request_id,created_by,created_at,version
                 ) VALUES(?1,?2,?3,NULL,?4,'open',NULL,?5,?6,?7,1)",
                params![
                    proposal.org_id,
                    proposal.team_id,
                    work_id,
                    title,
                    request_id,
                    actor,
                    crate::util::now()
                ],
            )?;
            created.push(
                self.get_team_work_item(&proposal.org_id, &proposal.team_id, &work_id)?
                    .ok_or(StoreError::ControlResourceConflict)?,
            );
        }
        tx.commit()?;
        Ok(created)
    }

    /// Bind an open/parked work item to a managed attempt/run. Membership is rechecked.
    /// Retries with the same attempt_id are idempotent; a different attempt conflicts.
    pub fn activate_team_work_item(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work_id: &str,
        attempt_id: &str,
        run_id: &str,
    ) -> Result<TeamWorkItem> {
        validate_id(attempt_id, "attempt_id")?;
        validate_id(run_id, "run_id")?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let current = self
            .get_team_work_item(org, team, work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if current.status == "running" {
            if current.attempt_id.as_deref() == Some(attempt_id)
                && current.run_id.as_deref() == Some(run_id)
            {
                tx.commit()?;
                return Ok(current);
            }
            return Err(StoreError::ControlResourceConflict);
        }
        if current.status != "open" && current.status != "parked" {
            return Err(StoreError::ControlAccessDenied);
        }
        let updated = self.conn.execute(
            "UPDATE team_work_items
             SET status='running', attempt_id=?4, run_id=?5, version=version+1
             WHERE org_id=?1 AND team_id=?2 AND work_id=?3
               AND status IN ('open','parked')",
            params![org, team, work_id, attempt_id, run_id],
        )?;
        if updated == 0 {
            return Err(StoreError::ControlResourceConflict);
        }
        let row = self
            .get_team_work_item(org, team, work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Resume parked work after membership/context revalidation. Does not start inference.
    pub fn resume_team_work_item(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work_id: &str,
    ) -> Result<TeamWorkItem> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let updated = self.conn.execute(
            "UPDATE team_work_items
             SET status='open', attempt_id=NULL, run_id=NULL, version=version+1
             WHERE org_id=?1 AND team_id=?2 AND work_id=?3 AND status='parked'",
            params![org, team, work_id],
        )?;
        if updated == 0 {
            return Err(StoreError::ControlAccessDenied);
        }
        let row = self
            .get_team_work_item(org, team, work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Advance an event/schedule cursor and create at most one work item for that event.
    /// Duplicate deliveries with the same event_id do not backlog additional work.
    pub fn activate_from_cursor(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
        cursor_key: &str,
        event_id: &str,
        work_title: &str,
    ) -> Result<(WorkActivationCursor, Option<TeamWorkItem>)> {
        validate_id(source, "source")?;
        validate_id(cursor_key, "cursor_key")?;
        validate_id(event_id, "event_id")?;
        validate_title(work_title)?;
        if source != "event" && source != "schedule" {
            return Err(StoreError::InvalidControlResource("source".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let existing = self
            .conn
            .query_row(
                "SELECT last_event_id, version FROM work_activation_cursors
                 WHERE org_id=?1 AND team_id=?2 AND source=?3 AND cursor_key=?4",
                params![org, team, source, cursor_key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((last, version)) = &existing {
            if last == event_id {
                let cursor = WorkActivationCursor {
                    org_id: org.into(),
                    team_id: team.into(),
                    source: source.into(),
                    cursor_key: cursor_key.into(),
                    last_event_id: last.clone(),
                    version: *version,
                };
                tx.commit()?;
                return Ok((cursor, None));
            }
        }
        let request_id = format!("{source}/{cursor_key}/{event_id}");
        let work_id = format!("{source}/{cursor_key}/{event_id}");
        let work = if let Some(existing_work) = self.work_item_by_request(org, team, &request_id)? {
            existing_work
        } else {
            self.conn.execute(
                "INSERT INTO team_work_items(
                    org_id,team_id,work_id,goal_id,title,status,owner_principal_id,
                    request_id,created_by,created_at,version
                 ) VALUES(?1,?2,?3,NULL,?4,'open',NULL,?5,?6,?7,1)",
                params![
                    org,
                    team,
                    work_id,
                    work_title,
                    request_id,
                    actor,
                    crate::util::now()
                ],
            )?;
            self.get_team_work_item(org, team, &work_id)?
                .ok_or(StoreError::ControlResourceConflict)?
        };
        self.conn.execute(
            "INSERT INTO work_activation_cursors(
                org_id,team_id,source,cursor_key,last_event_id,version,updated_at
             ) VALUES(?1,?2,?3,?4,?5,1,?6)
             ON CONFLICT(org_id,team_id,source,cursor_key) DO UPDATE SET
                last_event_id=excluded.last_event_id,
                version=work_activation_cursors.version+1,
                updated_at=excluded.updated_at",
            params![org, team, source, cursor_key, event_id, crate::util::now()],
        )?;
        let cursor = self
            .conn
            .query_row(
                "SELECT org_id,team_id,source,cursor_key,last_event_id,version
                 FROM work_activation_cursors
                 WHERE org_id=?1 AND team_id=?2 AND source=?3 AND cursor_key=?4",
                params![org, team, source, cursor_key],
                |r| {
                    Ok(WorkActivationCursor {
                        org_id: r.get(0)?,
                        team_id: r.get(1)?,
                        source: r.get(2)?,
                        cursor_key: r.get(3)?,
                        last_event_id: r.get(4)?,
                        version: r.get(5)?,
                    })
                },
            )?;
        tx.commit()?;
        Ok((cursor, Some(work)))
    }

    /// Authorized temporary child under a parent work item. Child budget cannot
    /// exceed the parent allocation; stop scope is inherited. Cross-team deny
    /// discloses no parent title or private fields.
    pub fn create_work_delegation(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        delegation_id: &str,
        parent_work_id: &str,
        child_work_id: &str,
        child_title: &str,
        request_id: &str,
        parent_budget_tokens: i64,
        child_budget_tokens: i64,
        stop_scope: &str,
        peer_org: Option<&str>,
        peer_team: Option<&str>,
    ) -> Result<WorkDelegation> {
        validate_id(delegation_id, "delegation_id")?;
        validate_id(parent_work_id, "parent_work_id")?;
        validate_id(child_work_id, "child_work_id")?;
        validate_id(request_id, "request_id")?;
        validate_title(child_title)?;
        validate_id(stop_scope, "stop_scope")?;
        if parent_budget_tokens < 0 || child_budget_tokens < 0 {
            return Err(StoreError::InvalidControlResource("budget".into()));
        }
        if child_budget_tokens > parent_budget_tokens {
            return Err(StoreError::InvalidControlResource("budget".into()));
        }
        if let (Some(p_org), Some(p_team)) = (peer_org, peer_team) {
            if p_org != org || p_team != team {
                // Same opaque denial as outsider access; do not leak parent context.
                return Err(StoreError::ControlAccessDenied);
            }
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if let Some(existing) = self.delegation_by_request(org, team, request_id)? {
            if existing.delegation_id != delegation_id
                || existing.parent_work_id != parent_work_id
                || existing.child_work_id != child_work_id
                || existing.child_budget_tokens != child_budget_tokens
            {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        let parent = self
            .get_team_work_item(org, team, parent_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if parent.status == "parked" {
            return Err(StoreError::ControlAccessDenied);
        }
        let inherited_stop = if stop_scope == "inherit" {
            format!("work/{parent_work_id}")
        } else if stop_scope.starts_with(&format!("work/{parent_work_id}"))
            || stop_scope == format!("goal/{}", parent.goal_id.as_deref().unwrap_or(""))
        {
            stop_scope.to_string()
        } else {
            return Err(StoreError::InvalidControlResource("stop_scope".into()));
        };
        let child_request = format!("{request_id}/child");
        if let Some(existing_child) = self.work_item_by_request(org, team, &child_request)? {
            if existing_child.work_id != child_work_id || existing_child.title != child_title {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            self.conn.execute(
                "INSERT INTO team_work_items(
                    org_id,team_id,work_id,goal_id,title,status,owner_principal_id,
                    request_id,created_by,created_at,version
                 ) VALUES(?1,?2,?3,?4,?5,'open',NULL,?6,?7,?8,1)",
                params![
                    org,
                    team,
                    child_work_id,
                    parent.goal_id,
                    child_title,
                    child_request,
                    actor,
                    crate::util::now()
                ],
            )?;
        }
        self.conn.execute(
            "INSERT INTO work_delegations(
                org_id,team_id,delegation_id,parent_work_id,child_work_id,goal_id,
                payer_principal_id,stop_scope,parent_budget_tokens,child_budget_tokens,
                request_id,status,created_by,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'open',?12,?13)",
            params![
                org,
                team,
                delegation_id,
                parent_work_id,
                child_work_id,
                parent.goal_id,
                actor,
                inherited_stop,
                parent_budget_tokens,
                child_budget_tokens,
                request_id,
                actor,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_work_delegation(org, team, delegation_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn delegation_by_request(
        &self,
        org: &str,
        team: &str,
        request_id: &str,
    ) -> Result<Option<WorkDelegation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,delegation_id,parent_work_id,child_work_id,goal_id,
                        payer_principal_id,stop_scope,parent_budget_tokens,child_budget_tokens,
                        request_id,status,created_by
                 FROM work_delegations WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request_id],
                Self::map_delegation,
            )
            .optional()?)
    }

    pub fn get_work_delegation(
        &self,
        org: &str,
        team: &str,
        delegation_id: &str,
    ) -> Result<Option<WorkDelegation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,delegation_id,parent_work_id,child_work_id,goal_id,
                        payer_principal_id,stop_scope,parent_budget_tokens,child_budget_tokens,
                        request_id,status,created_by
                 FROM work_delegations WHERE org_id=?1 AND team_id=?2 AND delegation_id=?3",
                params![org, team, delegation_id],
                Self::map_delegation,
            )
            .optional()?)
    }

    /// Look up the delegation that created a child work item, if any.
    pub fn work_delegation_for_child(
        &self,
        org: &str,
        team: &str,
        child_work_id: &str,
    ) -> Result<Option<WorkDelegation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,delegation_id,parent_work_id,child_work_id,goal_id,
                        payer_principal_id,stop_scope,parent_budget_tokens,child_budget_tokens,
                        request_id,status,created_by
                 FROM work_delegations WHERE org_id=?1 AND team_id=?2 AND child_work_id=?3",
                params![org, team, child_work_id],
                Self::map_delegation,
            )
            .optional()?)
    }

    fn map_delegation(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkDelegation> {
        Ok(WorkDelegation {
            org_id: r.get(0)?,
            team_id: r.get(1)?,
            delegation_id: r.get(2)?,
            parent_work_id: r.get(3)?,
            child_work_id: r.get(4)?,
            goal_id: r.get(5)?,
            payer_principal_id: r.get(6)?,
            stop_scope: r.get(7)?,
            parent_budget_tokens: r.get(8)?,
            child_budget_tokens: r.get(9)?,
            request_id: r.get(10)?,
            status: r.get(11)?,
            created_by: r.get(12)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrganizationRole, TeamRow};

    fn primed() -> Store {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.register_control_principal("bob").unwrap();
        db.set_organization_member("org", "bob", OrganizationRole::Member)
            .unwrap();
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
        db.add_team_member("org", "team", "bob").unwrap();
        db
    }

    #[test]
    fn quick_task_does_not_require_a_huddle_and_retries_are_idempotent() {
        let db = primed();
        let first = db
            .create_team_work_item(
                "alice",
                "org",
                "team",
                "w1",
                "Ship the preview",
                "req-1",
                None,
            )
            .unwrap();
        let again = db
            .create_team_work_item(
                "alice",
                "org",
                "team",
                "w1",
                "Ship the preview",
                "req-1",
                None,
            )
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(first.status, "open");
        assert!(db
            .create_team_work_item(
                "alice",
                "org",
                "team",
                "w2",
                "Different",
                "req-1",
                None,
            )
            .is_err());
    }

    #[test]
    fn accepted_huddle_creates_work_idempotently_and_outsider_is_denied() {
        let db = primed();
        db.create_team_goal("alice", "org", "team", "g1", "Stay online")
            .unwrap();
        let proposal = HuddleProposal {
            org_id: "org".into(),
            team_id: "team".into(),
            huddle_id: "h1".into(),
            proposal_version: 1,
            status: "accepted".into(),
            request_id: "huddle-req".into(),
            created_by: "alice".into(),
            work_titles: vec!["Watch logs".into(), "Page on-call".into()],
        };
        let first = db.accept_huddle_proposal("alice", &proposal).unwrap();
        assert_eq!(first.len(), 2);
        let again = db.accept_huddle_proposal("bob", &proposal).unwrap();
        assert_eq!(first, again);
        let listed = db.list_team_work_items("bob", "org", "team").unwrap();
        assert_eq!(listed.len(), 2);
        db.register_control_principal("eve").unwrap();
        assert!(db
            .accept_huddle_proposal("eve", &proposal)
            .is_err());
        let parked = db
            .park_team_work_item("alice", "org", "team", "huddle/h1/1")
            .unwrap();
        assert_eq!(parked.status, "parked");
        let open = listed
            .iter()
            .filter(|w| w.status == "open" || w.work_id == "huddle/h1/2")
            .count();
        assert!(open >= 1);
    }

    #[test]
    fn parked_work_survives_and_independent_work_stays_open() {
        let db = primed();
        db.create_team_work_item("alice", "org", "team", "a", "A", "ra", None)
            .unwrap();
        db.create_team_work_item("alice", "org", "team", "b", "B", "rb", None)
            .unwrap();
        db.park_team_work_item("alice", "org", "team", "a").unwrap();
        let listed = db.list_team_work_items("alice", "org", "team").unwrap();
        assert_eq!(
            listed.iter().find(|w| w.work_id == "a").unwrap().status,
            "parked"
        );
        assert_eq!(
            listed.iter().find(|w| w.work_id == "b").unwrap().status,
            "open"
        );
        let running = db
            .activate_team_work_item("alice", "org", "team", "b", "attempt-b", "run-b")
            .unwrap();
        assert_eq!(running.status, "running");
        assert_eq!(running.attempt_id.as_deref(), Some("attempt-b"));
        assert_eq!(running.run_id.as_deref(), Some("run-b"));
        assert_eq!(
            db.activate_team_work_item("alice", "org", "team", "b", "attempt-b", "run-b")
                .unwrap()
                .attempt_id
                .as_deref(),
            Some("attempt-b")
        );
        assert!(db
            .activate_team_work_item("alice", "org", "team", "b", "other", "run-b")
            .is_err());
        let resumed = db.resume_team_work_item("alice", "org", "team", "a").unwrap();
        assert_eq!(resumed.status, "open");
        assert!(resumed.attempt_id.is_none());
        assert!(resumed.run_id.is_none());
    }

    #[test]
    fn event_cursor_duplicates_do_not_backlog_work() {
        let db = primed();
        let (cursor, first) = db
            .activate_from_cursor(
                "alice",
                "org",
                "team",
                "event",
                "inbox",
                "e1",
                "Handle e1",
            )
            .unwrap();
        assert_eq!(cursor.last_event_id, "e1");
        assert_eq!(first.unwrap().work_id, "event/inbox/e1");
        let (again_cursor, again_work) = db
            .activate_from_cursor(
                "alice",
                "org",
                "team",
                "event",
                "inbox",
                "e1",
                "Handle e1",
            )
            .unwrap();
        assert_eq!(again_cursor.last_event_id, "e1");
        assert!(again_work.is_none());
        assert_eq!(db.list_team_work_items("alice", "org", "team").unwrap().len(), 1);
        let (_, next) = db
            .activate_from_cursor(
                "alice",
                "org",
                "team",
                "schedule",
                "nightly",
                "tick-2",
                "Nightly",
            )
            .unwrap();
        assert!(next.is_some());
        assert_eq!(db.list_team_work_items("alice", "org", "team").unwrap().len(), 2);
    }

    #[test]
    fn delegation_inherits_budget_and_stop_and_cross_team_is_opaque() {
        let db = primed();
        db.create_team_goal("alice", "org", "team", "g1", "Goal")
            .unwrap();
        db.create_team_work_item("alice", "org", "team", "parent", "Parent", "rp", Some("g1"))
            .unwrap();
        assert!(db
            .create_work_delegation(
                "alice",
                "org",
                "team",
                "d1",
                "parent",
                "child",
                "Help",
                "del-1",
                100,
                150,
                "inherit",
                None,
                None,
            )
            .is_err());
        let first = db
            .create_work_delegation(
                "alice",
                "org",
                "team",
                "d1",
                "parent",
                "child",
                "Help",
                "del-1",
                100,
                40,
                "inherit",
                None,
                None,
            )
            .unwrap();
        assert_eq!(first.child_budget_tokens, 40);
        assert_eq!(first.stop_scope, "work/parent");
        assert_eq!(first.goal_id.as_deref(), Some("g1"));
        let again = db
            .create_work_delegation(
                "alice",
                "org",
                "team",
                "d1",
                "parent",
                "child",
                "Help",
                "del-1",
                100,
                40,
                "inherit",
                None,
                None,
            )
            .unwrap();
        assert_eq!(first, again);
        let err = db
            .create_work_delegation(
                "alice",
                "org",
                "team",
                "d2",
                "parent",
                "peer-child",
                "Leak",
                "del-2",
                100,
                10,
                "inherit",
                Some("other-org"),
                Some("other-team"),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::ControlAccessDenied));
        assert!(!format!("{err}").contains("Parent"));
        assert!(!format!("{err}").contains("g1"));
    }
}
