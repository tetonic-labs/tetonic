//! Versioned workspace selections. Packs are templates, never execution grants.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPack {
    pub org_id: String,
    pub team_id: String,
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub builtin: bool,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveToolPack {
    pub request_id: String,
    pub id: String,
    pub expected_revision: i64,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}

impl Store {
    pub(crate) fn migrate_tool_packs_v79(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tool_pack_versions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, id TEXT NOT NULL,
            revision INTEGER NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,id,revision), UNIQUE(org_id,team_id,request_id),
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id));
            CREATE TRIGGER IF NOT EXISTS tool_pack_no_update BEFORE UPDATE ON tool_pack_versions
            BEGIN SELECT RAISE(ABORT,'Tool pack revisions are immutable'); END;
            CREATE TRIGGER IF NOT EXISTS tool_pack_no_delete BEFORE DELETE ON tool_pack_versions
            BEGIN SELECT RAISE(ABORT,'Tool pack revisions are immutable'); END;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(79,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn tool_packs(&self, actor: &str, org: &str, team: &str) -> Result<Vec<ToolPack>> {
        self.require_team_participant(actor, org, team)?;
        let mut query = self.conn.prepare(
            "SELECT payload FROM tool_pack_versions v
            WHERE org_id=?1 AND team_id=?2 AND revision=(SELECT MAX(revision)
            FROM tool_pack_versions WHERE org_id=v.org_id AND team_id=v.team_id AND id=v.id)
            ORDER BY id",
        )?;
        let rows = query
            .query_map(params![org, team], |r| r.get::<_, String>(0))?
            .map(|row| decode(&row?))
            .collect();
        rows
    }

    pub fn tool_pack_revision(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
        revision: i64,
    ) -> Result<Option<ToolPack>> {
        self.require_team_participant(actor, org, team)?;
        self.conn.query_row("SELECT payload FROM tool_pack_versions WHERE org_id=?1 AND team_id=?2 AND id=?3 AND revision=?4",
            params![org,team,id,revision], |r| r.get::<_,String>(0)).optional()?.map(|s| decode(&s)).transpose()
    }

    pub fn save_tool_pack(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        input: &SaveToolPack,
    ) -> Result<ToolPack> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map_or(true, |t| t.owner_principal_id != actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        if uuid::Uuid::parse_str(&input.id).is_err()
            || uuid::Uuid::parse_str(&input.request_id).is_err()
            || input.expected_revision < 0
            || input.expected_revision == i64::MAX
            || input.name.trim().is_empty()
            || input.name.chars().count() > 60
            || input.name.chars().any(char::is_control)
            || input.description.len() > 1000
            || input.description.contains('\0')
            || input.tools.is_empty()
            || input.tools.len() > 128
            || input.tools.iter().any(|t| {
                t.is_empty()
                    || t.len() > 256
                    || t.chars().any(|c| c.is_control() || c.is_whitespace())
            })
        {
            return Err(StoreError::InvalidControlResource(
                "Invalid tool pack name, members or revision".into(),
            ));
        }
        let mut tools = input.tools.clone();
        tools.sort();
        tools.dedup();
        let pack = ToolPack {
            org_id: org.into(),
            team_id: team.into(),
            id: input.id.clone(),
            revision: input.expected_revision + 1,
            name: input.name.trim().into(),
            description: input.description.trim().into(),
            tools,
            builtin: false,
        };
        let prior: Option<String> = self.conn.query_row("SELECT payload FROM tool_pack_versions WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
            params![org,team,input.request_id], |r| r.get(0)).optional()?;
        if let Some(prior) = prior {
            if decode(&prior)? != pack {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(pack);
        }
        let current: i64 = self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM tool_pack_versions WHERE org_id=?1 AND team_id=?2 AND id=?3",
            params![org,team,input.id], |r| r.get(0))?;
        if current != input.expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute(
            "INSERT INTO tool_pack_versions VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                org,
                team,
                input.id,
                pack.revision,
                input.request_id,
                serde_json::to_string(&pack).map_err(|_| StoreError::ControlResourceConflict)?
            ],
        )?;
        tx.commit()?;
        Ok(pack)
    }
}

fn decode(s: &str) -> Result<ToolPack> {
    serde_json::from_str(s).map_err(|_| StoreError::ControlResourceConflict)
}

#[cfg(test)]
mod tests;
