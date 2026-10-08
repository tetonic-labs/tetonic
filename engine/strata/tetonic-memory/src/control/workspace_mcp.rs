//! Scoped connection definitions. Credentials stay in KeyStorage; this table
//! contains only opaque references and explicitly reviewed tool manifests.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize, Serialize)]
pub struct WorkspaceMcpConnection {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub auth: String,
    #[serde(skip)]
    pub secret_ref: Option<String>,
    pub revision: u64,
    pub binding_epoch: u64,
    pub enabled: bool,
    pub approved_manifests: Vec<Value>,
}

impl Store {
    pub(crate) fn migrate_workspace_mcp_v69(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspace_mcp (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, id TEXT NOT NULL,
            revision INTEGER NOT NULL, payload TEXT NOT NULL, secret_ref TEXT,
            PRIMARY KEY(org_id,team_id,id),
            FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id));",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions(version,applied_at) VALUES(69,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    fn require_mcp_owner(&self, actor: &str, org: &str, team: &str) -> Result<()> {
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

    pub fn workspace_mcp_connections(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<WorkspaceMcpConnection>> {
        self.require_mcp_owner(actor, org, team)?;
        let mut stmt = self.conn.prepare("SELECT payload,secret_ref FROM workspace_mcp WHERE org_id=?1 AND team_id=?2 ORDER BY id")?;
        let rows = stmt.query_map(params![org, team], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        rows.map(|row| {
            let (json, reference) = row?;
            let mut record: WorkspaceMcpConnection =
                serde_json::from_str(&json).map_err(|_| invalid())?;
            record.secret_ref = reference;
            Ok(record)
        })
        .collect()
    }

    pub fn workspace_mcp_connection(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
    ) -> Result<Option<WorkspaceMcpConnection>> {
        self.require_mcp_owner(actor, org, team)?;
        let row = self.conn.query_row("SELECT payload,secret_ref FROM workspace_mcp WHERE org_id=?1 AND team_id=?2 AND id=?3", params![org,team,id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).optional()?;
        row.map(|(json, reference)| {
            let mut record: WorkspaceMcpConnection =
                serde_json::from_str(&json).map_err(|_| invalid())?;
            record.secret_ref = reference;
            Ok(record)
        })
        .transpose()
    }

    pub fn save_workspace_mcp(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        record: &WorkspaceMcpConnection,
        expected_revision: u64,
    ) -> Result<()> {
        self.require_mcp_owner(actor, org, team)?;
        if record.id.is_empty()
            || record.id.len() > 20
            || !record
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || record.name.trim().is_empty()
            || record.name.len() > 80
            || record.name.chars().any(char::is_control)
            || record.endpoint.len() > 2048
            || record.endpoint.is_empty()
            || !matches!(record.auth.as_str(), "none" | "bearer")
            || record.approved_manifests.len() > 32
            || record.revision != expected_revision.saturating_add(1)
            || record.binding_epoch == 0
            || (record.enabled && record.auth == "bearer" && record.secret_ref.is_none())
        {
            return Err(invalid());
        }
        let payload = serde_json::to_string(record).map_err(|_| invalid())?;
        if payload.len() > 524_288 {
            return Err(invalid());
        }
        let transaction = self.conn.unchecked_transaction()?;
        let records = self.workspace_mcp_connections(actor, org, team)?;
        let old = records.iter().find(|r| r.id == record.id);
        if old.map_or(0, |r| r.revision) != expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        if let Some(old) = old {
            if old.endpoint != record.endpoint
                || record.binding_epoch < old.binding_epoch
                || (old.secret_ref != record.secret_ref || old.auth != record.auth)
                    && (record.binding_epoch <= old.binding_epoch
                        || !record.approved_manifests.is_empty())
            {
                return Err(invalid());
            }
        } else if records.len() >= 32 {
            return Err(invalid());
        }
        self.conn.execute("INSERT INTO workspace_mcp(org_id,team_id,id,revision,payload,secret_ref) VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(org_id,team_id,id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload,secret_ref=excluded.secret_ref",
            params![org,team,record.id,record.revision,payload,record.secret_ref])?;
        transaction.commit()?;
        Ok(())
    }
}
fn invalid() -> StoreError {
    StoreError::InvalidControlResource(
        "Invalid MCP connection settings or changed credential binding".into(),
    )
}

#[cfg(test)]
mod tests;
