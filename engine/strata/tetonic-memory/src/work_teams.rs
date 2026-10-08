//! Reusable agent rosters inside an existing security team. These are not grants.
//! Versions are immutable so editing a roster cannot retarget accepted work.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTeam {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub agent_keys: Vec<String>,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkTeamSelection {
    pub id: String,
    pub revision: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkTeam {
    pub id: String,
    pub request_id: String,
    pub expected_revision: i64,
    pub name: String,
    pub purpose: String,
    pub agent_keys: Vec<String>,
}

impl Store {
    pub(crate) fn migrate_work_teams_v68(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS work_team_versions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, id TEXT NOT NULL,
            revision INTEGER NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,id,revision), UNIQUE(org_id,team_id,request_id),
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id)
        );
        CREATE TABLE IF NOT EXISTS work_team_bindings (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
            roster_id TEXT, revision INTEGER,
            PRIMARY KEY(org_id,team_id,work_id),
            FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id),
            FOREIGN KEY(org_id,team_id,roster_id,revision) REFERENCES work_team_versions(org_id,team_id,id,revision),
            CHECK ((roster_id IS NULL AND revision IS NULL) OR (roster_id IS NOT NULL AND revision IS NOT NULL))
        );")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions(version,applied_at) VALUES(68,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn work_teams(&self, actor: &str, org: &str, team: &str) -> Result<Vec<WorkTeam>> {
        self.require_team_participant(actor, org, team)?;
        let mut query = self.conn.prepare("SELECT payload FROM work_team_versions v WHERE org_id=?1 AND team_id=?2 AND revision=(SELECT MAX(revision) FROM work_team_versions WHERE org_id=v.org_id AND team_id=v.team_id AND id=v.id) ORDER BY id")?;
        let rows = query.query_map(params![org, team], |r| r.get::<_, String>(0))?;
        rows.map(|row| decode(&row?)).collect()
    }

    pub fn save_work_team(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        request: &SaveWorkTeam,
    ) -> Result<WorkTeam> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map_or(true, |t| t.owner_principal_id != actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        if request.id.is_empty()
            || request.id.len() > 128
            || request.id.contains('\0')
            || request.request_id.is_empty()
            || request.request_id.len() > 128
            || request.request_id.contains('\0')
            || request.name.trim().is_empty()
            || request.name.len() > 80
            || request.name.contains('\0')
            || request.purpose.len() > 1000
            || request.purpose.contains('\0')
            || request.agent_keys.is_empty()
            || request.agent_keys.len() > 24
            || request
                .agent_keys
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != request.agent_keys.len()
            || request.expected_revision < 0
            || request.expected_revision == i64::MAX
        {
            return Err(StoreError::InvalidControlResource(
                "Choose a name and 1–24 distinct saved agents for the team.".into(),
            ));
        }
        let row = WorkTeam {
            id: request.id.clone(),
            name: request.name.trim().into(),
            purpose: request.purpose.trim().into(),
            agent_keys: request.agent_keys.clone(),
            revision: request.expected_revision + 1,
        };
        let old: Option<String> = self.conn.query_row("SELECT payload FROM work_team_versions WHERE org_id=?1 AND team_id=?2 AND request_id=?3", params![org,team,request.request_id], |r|r.get(0)).optional()?;
        if let Some(old) = old {
            if decode(&old)? != row {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(row);
        }
        let current: i64 = self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM work_team_versions WHERE org_id=?1 AND team_id=?2 AND id=?3", params![org,team,request.id], |r|r.get(0))?;
        if current != request.expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        for key in &request.agent_keys {
            if self.registered_agent_unchecked(org, key)?.is_none() {
                return Err(StoreError::InvalidControlResource(
                    "Choose existing agents from this workspace.".into(),
                ));
            }
        }
        self.conn.execute(
            "INSERT INTO work_team_versions VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                org,
                team,
                row.id,
                row.revision,
                request.request_id,
                serde_json::to_string(&row).map_err(|_| StoreError::ControlResourceConflict)?
            ],
        )?;
        tx.commit()?;
        Ok(row)
    }

    pub fn work_team_for_work(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Option<WorkTeam>> {
        self.require_team_participant(actor, org, team)?;
        let payload: Option<String> = self.conn.query_row("SELECT v.payload FROM work_team_bindings b JOIN work_team_versions v ON v.org_id=b.org_id AND v.team_id=b.team_id AND v.id=b.roster_id AND v.revision=b.revision WHERE b.org_id=?1 AND b.team_id=?2 AND b.work_id=?3", params![org,team,work], |r|r.get(0)).optional()?;
        payload.map(|p| decode(&p)).transpose()
    }

    /// Work and its roster (including no roster) are committed together. A lost
    /// response cannot change the recipient on retry. Follow-ups inherit exactly.
    pub fn create_rostered_work(
        &self,
        command: crate::CreateTeamWorkItem<'_>,
        input: Option<&str>,
        purpose: crate::WorkPurpose,
        selection: Option<&WorkTeamSelection>,
        parent: Option<&str>,
    ) -> Result<crate::TeamWorkItem> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (actor, org, team, id) = (command.actor, command.org, command.team, command.work_id);
        self.require_team_participant(actor, org, team)?;
        let existing = self.get_team_work_item(org, team, id)?.is_some();
        let pinned = self.work_team_for_work(actor, org, team, id)?;
        let inherited = if let Some(parent) = parent {
            if self.get_team_work_item(org, team, parent)?.is_none() {
                return Err(StoreError::ControlAccessDenied);
            }
            self.work_team_for_work(actor, org, team, parent)?
        } else {
            None
        };
        let selected = if existing {
            pinned
        } else if parent.is_some() {
            inherited.clone()
        } else if let Some(s) = selection {
            Some(
                self.work_teams(actor, org, team)?
                    .into_iter()
                    .find(|t| t.id == s.id && t.revision == s.revision)
                    .ok_or(StoreError::ControlResourceConflict)?,
            )
        } else {
            None
        };
        if selection.is_some_and(|s| {
            selected
                .as_ref()
                .map_or(true, |t| t.id != s.id || t.revision != s.revision)
        }) || (parent.is_some() && selected != inherited)
            || (existing && parent.is_none() && selection.is_none() && selected.is_some())
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let work = self.create_team_work_item_in_transaction(command, input, purpose)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO work_team_bindings VALUES(?1,?2,?3,?4,?5)",
            params![
                org,
                team,
                id,
                selected.as_ref().map(|t| &t.id),
                selected.as_ref().map(|t| t.revision)
            ],
        )?;
        tx.commit()?;
        Ok(work)
    }

    pub(crate) fn validate_work_team_assignments(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        content: &crate::PlanContent,
    ) -> Result<()> {
        if let Some(roster) = self.work_team_for_work(actor, org, team, work)? {
            if content
                .assignments
                .iter()
                .any(|a| !roster.agent_keys.contains(&a.agent_key))
            {
                return Err(StoreError::InvalidControlResource(
                    "This plan assigns an agent outside the selected team's saved roster.".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn inherit_work_team(
        &self,
        org: &str,
        team: &str,
        source: &str,
        target: &str,
    ) -> Result<()> {
        debug_assert!(!self.conn.is_autocommit());
        self.conn.execute("INSERT INTO work_team_bindings SELECT org_id,team_id,?4,roster_id,revision FROM work_team_bindings WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,source,target])?;
        Ok(())
    }
}

fn decode(payload: &str) -> Result<WorkTeam> {
    serde_json::from_str(payload).map_err(|_| StoreError::ControlResourceConflict)
}

#[cfg(test)]
mod tests;
