//! Immutable skill versions in a team's workspace. Grants remain on agent definitions.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub enabled: bool,
    pub created_at: String,
}

impl Store {
    pub(crate) fn migrate_workspace_skills_v67(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspace_skills (
                org_id TEXT NOT NULL, team_id TEXT NOT NULL, id TEXT NOT NULL,
                name TEXT NOT NULL, description TEXT NOT NULL, source TEXT NOT NULL,
                content TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                PRIMARY KEY(org_id, team_id, id),
                FOREIGN KEY(org_id, team_id) REFERENCES teams(org_id, team_id)
            );",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions(version,applied_at) VALUES(67,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    fn require_skill_owner(&self, actor: &str, org: &str, team: &str) -> Result<()> {
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map(|t| t.owner_principal_id)
            .as_deref()
            != Some(actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }

    pub fn workspace_skills(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<WorkspaceSkill>> {
        self.require_team_participant(actor, org, team)?;
        let mut query = self.conn.prepare("SELECT id,name,description,source,enabled,created_at FROM workspace_skills WHERE org_id=?1 AND team_id=?2 ORDER BY name,id")?;
        let rows = query.query_map(params![org, team], |r| {
            Ok(WorkspaceSkill {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                source: r.get(3)?,
                enabled: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn workspace_skill_content(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
        active_only: bool,
    ) -> Result<Option<String>> {
        self.require_team_participant(actor, org, team)?;
        Ok(self.conn.query_row("SELECT content FROM workspace_skills WHERE org_id=?1 AND team_id=?2 AND id=?3 AND (?4=0 OR enabled=1)", params![org, team, id, active_only], |r| r.get(0)).optional()?)
    }

    /// Retries preserve revocation. A content change always gets a different ID.
    #[allow(clippy::too_many_arguments)]
    pub fn import_workspace_skill(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        name: &str,
        description: &str,
        source: &str,
        content: &str,
    ) -> Result<String> {
        self.require_skill_owner(actor, org, team)?;
        if name.is_empty()
            || name.len() > 64
            || description.is_empty()
            || description.len() > 4096
            || source.len() > 1024
            || content.is_empty()
            || content.len() > 32768
        {
            return Err(StoreError::InvalidControlResource(
                "skill exceeds workspace limits".into(),
            ));
        }
        // Include scope so a copied ID cannot silently become a cross-team grant.
        let bytes = serde_json::to_vec(&(org, team, content))
            .map_err(|_| StoreError::InvalidControlResource("skill content".into()))?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        let id = format!("skill_{}", &digest[..48]);
        if self
            .workspace_skill_content(actor, org, team, &id, false)?
            .is_none()
        {
            let count: i64 = self.conn.query_row(
                "SELECT count(*) FROM workspace_skills WHERE org_id=?1 AND team_id=?2",
                params![org, team],
                |r| r.get(0),
            )?;
            if count >= 128 {
                return Err(StoreError::InvalidControlResource(
                    "workspace skill library is full".into(),
                ));
            }
        }
        self.conn.execute("INSERT OR IGNORE INTO workspace_skills(org_id,team_id,id,name,description,source,content,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![org, team, id, name, description, source, content, crate::util::now()])?;
        Ok(id)
    }

    pub fn revoke_workspace_skill(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
    ) -> Result<()> {
        self.require_skill_owner(actor, org, team)?;
        if self.conn.execute(
            "UPDATE workspace_skills SET enabled=0 WHERE org_id=?1 AND team_id=?2 AND id=?3",
            params![org, team, id],
        )? == 0
        {
            return Err(StoreError::InvalidControlResource("skill not found".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
