//! Host-owned credential references. Plaintext keys never enter SQLite.
//! These methods are for the local owner composition, not employee resources.
use crate::{Result, Store};
use rusqlite::{params, OptionalExtension};

impl Store {
    pub(crate) fn migrate_local_provider_keys_v51(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS local_provider_keys (
                provider TEXT PRIMARY KEY,
                key_reference TEXT NOT NULL
            );",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions(version, applied_at) VALUES (51, ?1)",
            params![crate::util::now()],
        )?;
        Ok(())
    }

    pub fn local_provider_key(&self, provider: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT key_reference FROM local_provider_keys WHERE provider = ?1",
                params![provider],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_local_provider_key(&self, provider: &str, reference: Option<&str>) -> Result<()> {
        if let Some(reference) = reference {
            self.conn.execute(
                "INSERT INTO local_provider_keys(provider, key_reference) VALUES (?1, ?2)
                 ON CONFLICT(provider) DO UPDATE SET key_reference = excluded.key_reference",
                params![provider, reference],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM local_provider_keys WHERE provider = ?1",
                params![provider],
            )?;
        }
        Ok(())
    }
}
