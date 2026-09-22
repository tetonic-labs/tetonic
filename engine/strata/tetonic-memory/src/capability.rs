//! Durable issued-capability records (R6-1 / schema v24).

use rusqlite::{params, OptionalExtension};

use crate::{now, Result, Store, StoreError};
use tetonic_domain::execution::IssuedCapability;

impl Store {
    pub(crate) fn migrate_capabilities_v24(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 24 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS issued_capabilities (
                capability_id TEXT PRIMARY KEY,
                payload_json TEXT NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0,
                current_use_count INTEGER NOT NULL DEFAULT 0,
                expiration INTEGER NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_issued_caps_revoked
               ON issued_capabilities(revoked);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (24, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    /// Mark every live capability revoked (process restart / new EngineRuntime).
    pub fn revoke_all_issued_capabilities(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE issued_capabilities SET revoked = 1, updated_at = ?1 WHERE revoked = 0",
            params![now()],
        )?;
        Ok(n)
    }

    pub fn insert_new_issued_capability(&self, cap: &IssuedCapability) -> Result<()> {
        let payload = serde_json::to_string(cap)
            .map_err(|e| StoreError::InvalidDataClass(format!("capability json: {e}")))?;
        self.conn.execute(
            "INSERT INTO issued_capabilities(
                capability_id, payload_json, revoked, current_use_count, expiration, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                cap.capability_id.0,
                payload,
                if cap.revoked { 1 } else { 0 },
                cap.current_use_count as i64,
                cap.expiration as i64,
                now(),
            ],
        )?;
        Ok(())
    }

    pub fn consume_issued_capability(
        &self,
        capability_id: &str,
        max_use: u32,
        now_secs: u64,
    ) -> Result<IssuedCapability> {
        let row: Option<(String, i64, i64, i64)> = self
            .conn
            .query_row(
                "SELECT payload_json, revoked, current_use_count, expiration FROM issued_capabilities
                 WHERE capability_id = ?1",
                params![capability_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((payload, revoked, use_count, expiration)) = row else {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        };
        if revoked != 0 {
            return Err(StoreError::CapabilityRevoked);
        }
        if (expiration as u64) < now_secs {
            return Err(StoreError::CapabilityExpired);
        }
        if (use_count as u32) >= max_use {
            return Err(StoreError::CapabilityAlreadyConsumed);
        }
        let updated = self.conn.execute(
            "UPDATE issued_capabilities
             SET current_use_count = current_use_count + 1, updated_at = ?1
             WHERE capability_id = ?2 AND revoked = 0 AND current_use_count < ?3",
            params![now(), capability_id, max_use as i64],
        )?;
        if updated == 0 {
            return Err(StoreError::CapabilityAlreadyConsumed);
        }
        let mut cap: IssuedCapability = serde_json::from_str(&payload)
            .map_err(|e| StoreError::InvalidDataClass(format!("capability json: {e}")))?;
        cap.revoked = false;
        cap.current_use_count = (use_count as u32) + 1;
        Ok(cap)
    }

    pub fn upsert_issued_capability(&self, cap: &IssuedCapability) -> Result<()> {
        let payload = serde_json::to_string(cap)
            .map_err(|e| StoreError::InvalidDataClass(format!("capability json: {e}")))?;
        self.conn.execute(
            "INSERT INTO issued_capabilities(
                capability_id, payload_json, revoked, current_use_count, expiration, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(capability_id) DO UPDATE SET
                payload_json = excluded.payload_json,
                revoked = excluded.revoked,
                current_use_count = excluded.current_use_count,
                expiration = excluded.expiration,
                updated_at = excluded.updated_at",
            params![
                cap.capability_id.0,
                payload,
                if cap.revoked { 1 } else { 0 },
                cap.current_use_count as i64,
                cap.expiration as i64,
                now(),
            ],
        )?;
        Ok(())
    }

    pub fn load_issued_capability(&self, capability_id: &str) -> Result<Option<IssuedCapability>> {
        let row: Option<(String, i64, i64)> = self
            .conn
            .query_row(
                "SELECT payload_json, revoked, current_use_count FROM issued_capabilities
                 WHERE capability_id = ?1",
                params![capability_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((payload, revoked, use_count)) = row else {
            return Ok(None);
        };
        let mut cap: IssuedCapability = serde_json::from_str(&payload)
            .map_err(|e| StoreError::InvalidDataClass(format!("capability json: {e}")))?;
        cap.revoked = revoked != 0;
        cap.current_use_count = use_count as u32;
        Ok(Some(cap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::classify::DataClass;
    use tetonic_domain::execution::ActionKind;
    use tetonic_domain::ids::{CapabilityId, SessionId};

    #[test]
    fn persist_consume_and_revoke_on_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap.db");
        let store = Store::open(&path).unwrap();
        let mut cap = IssuedCapability {
            capability_id: CapabilityId::new("cap_1"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            action_kind: ActionKind::ReadFile,
            canonical_parameter_digest: "d".into(),
            workspace_version: None,
            data_classification: DataClass::RepositorySource,
            issuance_timestamp: 1,
            expiration: u64::MAX,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v".into(),
            approval_record_id: None,
            revoked: false,
        };
        store.upsert_issued_capability(&cap).unwrap();
        cap.current_use_count = 1;
        store.upsert_issued_capability(&cap).unwrap();
        let loaded = store.load_issued_capability("cap_1").unwrap().unwrap();
        assert_eq!(loaded.current_use_count, 1);
        assert_eq!(store.revoke_all_issued_capabilities().unwrap(), 1);
        let revoked = store.load_issued_capability("cap_1").unwrap().unwrap();
        assert!(revoked.revoked);
    }
}
