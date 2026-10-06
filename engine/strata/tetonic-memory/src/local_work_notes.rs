//! Local work notes and team metadata for workroom project tracking.
use crate::{Result, Store};
use rusqlite::{params, OptionalExtension};

#[derive(Clone, Debug, Default)]
pub struct LocalWorkData {
    pub notes: Vec<String>,
    pub status: Option<String>,
    pub lead_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
}

impl Store {
    pub fn get_local_work_data(&self, work_id: &str) -> Result<LocalWorkData> {
        let has_table: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='local_work_notes'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !has_table {
            return Ok(LocalWorkData::default());
        }

        let has_status: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('local_work_notes') WHERE name='status'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if has_status {
            let row: Option<(String, Option<String>, Option<String>, Option<String>)> = self
                .conn
                .query_row(
                    "SELECT notes_json, status, lead_id, agent_ids_json FROM local_work_notes WHERE work_id = ?1",
                    params![work_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;

            match row {
                Some((notes_s, status, lead_id, agents_s)) => {
                    let notes: Vec<String> = serde_json::from_str(&notes_s).unwrap_or_default();
                    let agent_ids: Option<Vec<String>> =
                        agents_s.and_then(|s| serde_json::from_str(&s).ok());
                    Ok(LocalWorkData {
                        notes,
                        status,
                        lead_id,
                        agent_ids,
                    })
                }
                None => Ok(LocalWorkData::default()),
            }
        } else {
            let json_str: Option<String> = self
                .conn
                .query_row(
                    "SELECT notes_json FROM local_work_notes WHERE work_id = ?1",
                    params![work_id],
                    |row| row.get(0),
                )
                .optional()?;

            match json_str {
                Some(s) => Ok(LocalWorkData {
                    notes: serde_json::from_str(&s).unwrap_or_default(),
                    status: None,
                    lead_id: None,
                    agent_ids: None,
                }),
                None => Ok(LocalWorkData::default()),
            }
        }
    }

    pub fn get_local_work_notes(&self, work_id: &str) -> Result<Vec<String>> {
        self.get_local_work_data(work_id).map(|d| d.notes)
    }

    pub fn set_local_work_data(
        &self,
        work_id: &str,
        notes: Option<&[String]>,
        status: Option<&str>,
        lead_id: Option<&str>,
        agent_ids: Option<&[String]>,
    ) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS local_work_notes (
                work_id TEXT PRIMARY KEY,
                notes_json TEXT NOT NULL DEFAULT '[]',
                status TEXT,
                lead_id TEXT,
                agent_ids_json TEXT
            );",
        )?;
        let _ = self.conn.execute_batch(
            "ALTER TABLE local_work_notes ADD COLUMN status TEXT;
             ALTER TABLE local_work_notes ADD COLUMN lead_id TEXT;
             ALTER TABLE local_work_notes ADD COLUMN agent_ids_json TEXT;",
        );

        let existing = self.get_local_work_data(work_id)?;
        let final_notes = notes.map(|n| n.to_vec()).unwrap_or(existing.notes);
        let final_status = status.map(str::to_string).or(existing.status);
        let final_lead = lead_id.map(str::to_string).or(existing.lead_id);
        let final_agents = agent_ids.map(|a| a.to_vec()).or(existing.agent_ids);

        let notes_json = serde_json::to_string(&final_notes).map_err(|e| {
            crate::StoreError::InvalidControlResource(format!("Invalid notes JSON: {e}"))
        })?;
        let agents_json = final_agents
            .as_ref()
            .and_then(|a| serde_json::to_string(a).ok());

        self.conn.execute(
            "INSERT INTO local_work_notes(work_id, notes_json, status, lead_id, agent_ids_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(work_id) DO UPDATE SET
                notes_json = excluded.notes_json,
                status = excluded.status,
                lead_id = excluded.lead_id,
                agent_ids_json = excluded.agent_ids_json",
            params![work_id, notes_json, final_status, final_lead, agents_json],
        )?;
        Ok(())
    }

    pub fn set_local_work_notes(&self, work_id: &str, notes: &[String]) -> Result<()> {
        self.set_local_work_data(work_id, Some(notes), None, None, None)
    }
}
