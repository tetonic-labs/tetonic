//! Owner-approved local folders. Agent revisions still own individual tool grants.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFolders {
    pub revision: i64,
    pub paths: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkspaceFolders {
    pub request_id: String,
    pub expected_revision: i64,
    pub paths: Vec<String>,
}

impl Store {
    pub(crate) fn migrate_workspace_folders_v76(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS workspace_folder_versions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, revision INTEGER NOT NULL,
            request_id TEXT NOT NULL, payload TEXT NOT NULL, actor TEXT NOT NULL, created_at TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,revision), UNIQUE(org_id,team_id,request_id),
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id));
            CREATE TRIGGER IF NOT EXISTS workspace_folders_no_update BEFORE UPDATE ON workspace_folder_versions
            BEGIN SELECT RAISE(ABORT,'folder approvals are immutable'); END;
            CREATE TRIGGER IF NOT EXISTS workspace_folders_no_delete BEFORE DELETE ON workspace_folder_versions
            BEGIN SELECT RAISE(ABORT,'folder approvals are immutable'); END;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(76,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn workspace_folders(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<WorkspaceFolders> {
        self.require_folder_owner(actor, org, team)?;
        let payload: Option<String> = self.conn.query_row(
            "SELECT payload FROM workspace_folder_versions WHERE org_id=?1 AND team_id=?2 ORDER BY revision DESC LIMIT 1",
            params![org, team], |r| r.get(0)).optional()?;
        payload
            .map(|p| serde_json::from_str(&p).map_err(|_| StoreError::ControlResourceConflict))
            .transpose()
            .map(|r| r.unwrap_or_default())
    }

    fn require_folder_owner(&self, actor: &str, org: &str, team: &str) -> Result<()> {
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map_or(true, |t| t.owner_principal_id != actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }

    /// Paths must be validated by the host before publication. No agent receives
    /// access merely because its owner adds a folder to this catalog.
    pub fn save_workspace_folders(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        input: &SaveWorkspaceFolders,
    ) -> Result<WorkspaceFolders> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_folder_owner(actor, org, team)?;
        if input.expected_revision < 0
            || input.expected_revision == i64::MAX
            || uuid::Uuid::parse_str(&input.request_id).is_err()
            || input.paths.len() > 32
            || input
                .paths
                .iter()
                .any(|p| p.is_empty() || p.len() > 4096 || p.chars().any(char::is_control))
            || input.paths.windows(2).any(|p| p[0] >= p[1])
        {
            return Err(StoreError::InvalidControlResource(
                "Invalid folder selection".into(),
            ));
        }
        let row = WorkspaceFolders {
            revision: input.expected_revision + 1,
            paths: input.paths.clone(),
        };
        let payload =
            serde_json::to_string(&row).map_err(|_| StoreError::ControlResourceConflict)?;
        let prior: Option<String> = self.conn.query_row(
            "SELECT payload FROM workspace_folder_versions WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
            params![org,team,input.request_id], |r|r.get(0)).optional()?;
        if let Some(prior) = prior {
            if prior != payload {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(row);
        }
        if self.workspace_folders(actor, org, team)?.revision != input.expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute(
            "INSERT INTO workspace_folder_versions VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                team,
                row.revision,
                input.request_id,
                payload,
                actor,
                crate::util::now()
            ],
        )?;
        tx.commit()?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests;
