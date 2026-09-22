//! Runtime profile persistence (ES5) — append-only profiles + active bindings.

use rusqlite::{params, OptionalExtension};

use crate::capacity_tables::{
    CREATE_CAPACITY_BINDINGS, CREATE_RUNTIME_PROFILES, INSERT_RUNTIME_PROFILE,
    LIST_RUNTIME_PROFILES, LIST_RUNTIME_PROFILES_FOR_NODE, SELECT_CAPACITY_BINDING,
    SELECT_RUNTIME_PROFILE, UPSERT_CAPACITY_BINDING,
};
use crate::{now, Result, Store};

#[derive(Debug, Clone)]
pub struct RuntimeProfileRow {
    pub id: String,
    pub node_id: String,
    pub role: String,
    pub fingerprint: String,
    pub created_at: String,
    pub gates_passed: bool,
    pub json: String,
}

impl Store {
    pub fn migrate_capacity_v10(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 10 {
            return Ok(());
        }
        self.conn.execute_batch(&format!(
            "{CREATE_RUNTIME_PROFILES}\n{CREATE_CAPACITY_BINDINGS}"
        ))?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (10, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    /// Insert an immutable profile row. Fails if `id` already exists.
    pub fn insert_runtime_profile(&self, row: &RuntimeProfileRow) -> Result<()> {
        self.conn.execute(
            INSERT_RUNTIME_PROFILE,
            params![
                row.id,
                row.node_id,
                row.role,
                row.fingerprint,
                row.created_at,
                i32::from(row.gates_passed),
                row.json,
            ],
        )?;
        Ok(())
    }

    pub fn get_runtime_profile(&self, id: &str) -> Result<Option<RuntimeProfileRow>> {
        self.conn
            .query_row(SELECT_RUNTIME_PROFILE, params![id], |r| {
                Ok(RuntimeProfileRow {
                    id: r.get(0)?,
                    node_id: r.get(1)?,
                    role: r.get(2)?,
                    fingerprint: r.get(3)?,
                    created_at: r.get(4)?,
                    gates_passed: r.get::<_, i32>(5)? != 0,
                    json: r.get(6)?,
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn list_runtime_profiles(&self, node_id: Option<&str>) -> Result<Vec<RuntimeProfileRow>> {
        let mut out = Vec::new();
        match node_id {
            Some(nid) => {
                let mut stmt = self.conn.prepare(LIST_RUNTIME_PROFILES_FOR_NODE)?;
                let rows = stmt.query_map(params![nid], row_from_sql)?;
                for r in rows {
                    out.push(r?);
                }
            }
            None => {
                let mut stmt = self.conn.prepare(LIST_RUNTIME_PROFILES)?;
                let rows = stmt.query_map([], row_from_sql)?;
                for r in rows {
                    out.push(r?);
                }
            }
        }
        Ok(out)
    }

    pub fn set_capacity_binding(
        &self,
        node_id: &str,
        role: &str,
        profile_id: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            UPSERT_CAPACITY_BINDING,
            params![node_id, role, profile_id, now()],
        )?;
        Ok(())
    }

    pub fn get_capacity_binding(&self, node_id: &str, role: &str) -> Result<Option<String>> {
        self.conn
            .query_row(SELECT_CAPACITY_BINDING, params![node_id, role], |r| {
                r.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn migrate_capacity_v11(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 11 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS capacity_jobs (
                 id          TEXT PRIMARY KEY,
                 state       TEXT NOT NULL,
                 json        TEXT NOT NULL,
                 updated_at  TEXT NOT NULL
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (11, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    pub fn upsert_capacity_job(&self, id: &str, state: &str, json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO capacity_jobs(id, state, json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, json = excluded.json, updated_at = excluded.updated_at",
            params![id, state, json, now()],
        )?;
        Ok(())
    }

    pub fn get_capacity_job(&self, id: &str) -> Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT state, json FROM capacity_jobs WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_running_capacity_jobs_interrupted(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE capacity_jobs SET state = 'interrupted', updated_at = ?1
             WHERE state IN ('queued', 'running')",
            params![now()],
        )?;
        Ok(())
    }
}

fn row_from_sql(r: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeProfileRow> {
    Ok(RuntimeProfileRow {
        id: r.get(0)?,
        node_id: r.get(1)?,
        role: r.get(2)?,
        fingerprint: r.get(3)?,
        created_at: r.get(4)?,
        gates_passed: r.get::<_, i32>(5)? != 0,
        json: r.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn profile_round_trip() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let row = RuntimeProfileRow {
            id: "profile_test".into(),
            node_id: "local".into(),
            role: "coder".into(),
            fingerprint: "fp_abc".into(),
            created_at: now(),
            gates_passed: true,
            json: r#"{"schema_version":1}"#.into(),
        };
        store.insert_runtime_profile(&row).unwrap();
        store
            .set_capacity_binding("local", "coder", Some("profile_test"))
            .unwrap();
        let got = store.get_runtime_profile("profile_test").unwrap().unwrap();
        assert_eq!(got.id, "profile_test");
        assert_eq!(
            store.get_capacity_binding("local", "coder").unwrap(),
            Some("profile_test".into())
        );
    }
}
