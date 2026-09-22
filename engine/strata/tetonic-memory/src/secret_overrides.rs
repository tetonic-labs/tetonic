//! Durable scoped secret fingerprint overrides (R12 / schema v25).

use rusqlite::{params, OptionalExtension};

use crate::{new_id, now, Result, Store, StoreError};

/// Scope for a fingerprint override (session / project / global).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretOverrideScope {
    Global,
    Session(String),
    Project(String),
}

impl SecretOverrideScope {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Session(_) => "session",
            Self::Project(_) => "project",
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Session(s) | Self::Project(s) => Some(s.as_str()),
        }
    }

    pub fn parse(kind: &str, id: Option<&str>) -> Result<Self> {
        match kind {
            "global" => Ok(Self::Global),
            "session" => {
                let id = id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    StoreError::InvalidDataClass("session override requires scope_id".into())
                })?;
                Ok(Self::Session(id.to_string()))
            }
            "project" => {
                let id = id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    StoreError::InvalidDataClass("project override requires scope_id".into())
                })?;
                Ok(Self::Project(id.to_string()))
            }
            other => Err(StoreError::InvalidDataClass(format!(
                "unknown secret override scope: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecretOverrideRow {
    pub id: String,
    pub fingerprint: String,
    pub scope: SecretOverrideScope,
    pub durable: bool,
    pub revoked: bool,
    pub granted_at: String,
    pub revoked_at: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SecretOverrideAuditRow {
    pub id: String,
    pub override_id: String,
    pub action: String,
    pub fingerprint: String,
    pub scope_kind: String,
    pub scope_id: Option<String>,
    pub recorded_at: String,
    pub actor: Option<String>,
}

impl Store {
    pub(crate) fn migrate_secret_overrides_v25(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 25 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scanner_hmac_key (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                hmac_key TEXT NOT NULL,
                created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS secret_overrides (
                id TEXT PRIMARY KEY,
                fingerprint TEXT NOT NULL,
                scope_kind TEXT NOT NULL,
                scope_id TEXT,
                durable INTEGER NOT NULL DEFAULT 1,
                revoked INTEGER NOT NULL DEFAULT 0,
                granted_at TEXT NOT NULL,
                revoked_at TEXT,
                actor TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_secret_overrides_active
               ON secret_overrides(fingerprint, revoked);
             CREATE TABLE IF NOT EXISTS secret_override_audit (
                id TEXT PRIMARY KEY,
                override_id TEXT NOT NULL,
                action TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                scope_kind TEXT NOT NULL,
                scope_id TEXT,
                recorded_at TEXT NOT NULL,
                actor TEXT
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (25, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    /// Stable HMAC key for secret fingerprints across process restarts (R12).
    pub fn ensure_scanner_hmac_key(&self) -> Result<String> {
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT hmac_key FROM scanner_hmac_key WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(k) = existing {
            return Ok(k);
        }
        let key = new_id("hmac");
        self.conn.execute(
            "INSERT INTO scanner_hmac_key(id, hmac_key, created_at) VALUES (1, ?1, ?2)",
            params![key, now()],
        )?;
        Ok(key)
    }

    /// Grant a fingerprint override. Durable grants persist; ephemeral ones are
    /// session-process only (still audited when `actor` is set via caller).
    pub fn grant_secret_override(
        &self,
        fingerprint: &str,
        scope: SecretOverrideScope,
        durable: bool,
        actor: Option<&str>,
    ) -> Result<SecretOverrideRow> {
        if fingerprint.trim().is_empty() {
            return Err(StoreError::InvalidDataClass("empty fingerprint".into()));
        }
        // Revoke any prior active row for same fingerprint+scope, then insert.
        self.conn.execute(
            "UPDATE secret_overrides SET revoked = 1, revoked_at = ?1
             WHERE fingerprint = ?2 AND scope_kind = ?3
               AND IFNULL(scope_id, '') = IFNULL(?4, '') AND revoked = 0",
            params![now(), fingerprint, scope.kind(), scope.id()],
        )?;
        let id = new_id("sov");
        let granted_at = now();
        self.conn.execute(
            "INSERT INTO secret_overrides(
                id, fingerprint, scope_kind, scope_id, durable, revoked, granted_at, revoked_at, actor
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, NULL, ?7)",
            params![
                id,
                fingerprint,
                scope.kind(),
                scope.id(),
                if durable { 1 } else { 0 },
                granted_at,
                actor,
            ],
        )?;
        self.append_override_audit(&id, "grant", fingerprint, &scope, actor)?;
        Ok(SecretOverrideRow {
            id,
            fingerprint: fingerprint.to_string(),
            scope,
            durable,
            revoked: false,
            granted_at,
            revoked_at: None,
            actor: actor.map(|s| s.to_string()),
        })
    }

    pub fn revoke_secret_override(
        &self,
        fingerprint: &str,
        scope: SecretOverrideScope,
        actor: Option<&str>,
    ) -> Result<bool> {
        let row: Option<(String,)> = self
            .conn
            .query_row(
                "SELECT id FROM secret_overrides
                 WHERE fingerprint = ?1 AND scope_kind = ?2
                   AND IFNULL(scope_id, '') = IFNULL(?3, '') AND revoked = 0
                 LIMIT 1",
                params![fingerprint, scope.kind(), scope.id()],
                |r| Ok((r.get(0)?,)),
            )
            .optional()?;
        let Some((id,)) = row else {
            return Ok(false);
        };
        let revoked_at = now();
        self.conn.execute(
            "UPDATE secret_overrides SET revoked = 1, revoked_at = ?1 WHERE id = ?2",
            params![revoked_at, id],
        )?;
        self.append_override_audit(&id, "revoke", fingerprint, &scope, actor)?;
        Ok(true)
    }

    fn append_override_audit(
        &self,
        override_id: &str,
        action: &str,
        fingerprint: &str,
        scope: &SecretOverrideScope,
        actor: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO secret_override_audit(
                id, override_id, action, fingerprint, scope_kind, scope_id, recorded_at, actor
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                new_id("soa"),
                override_id,
                action,
                fingerprint,
                scope.kind(),
                scope.id(),
                now(),
                actor,
            ],
        )?;
        Ok(())
    }

    /// Active durable overrides (survive restart). Ephemeral grants are not listed.
    pub fn list_active_durable_secret_overrides(&self) -> Result<Vec<SecretOverrideRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, fingerprint, scope_kind, scope_id, durable, revoked, granted_at, revoked_at, actor
             FROM secret_overrides WHERE revoked = 0 AND durable = 1
             ORDER BY granted_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let kind: String = r.get(2)?;
            let scope_id: Option<String> = r.get(3)?;
            let scope = match SecretOverrideScope::parse(&kind, scope_id.as_deref()) {
                Ok(s) => s,
                Err(e) => {
                    return Err(rusqlite::Error::InvalidColumnType(
                        2,
                        e.to_string(),
                        rusqlite::types::Type::Text,
                    ))
                }
            };
            Ok(SecretOverrideRow {
                id: r.get(0)?,
                fingerprint: r.get(1)?,
                scope,
                durable: r.get::<_, i64>(4)? != 0,
                revoked: r.get::<_, i64>(5)? != 0,
                granted_at: r.get(6)?,
                revoked_at: r.get(7)?,
                actor: r.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_secret_override_audit(&self) -> Result<Vec<SecretOverrideAuditRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, override_id, action, fingerprint, scope_kind, scope_id, recorded_at, actor
             FROM secret_override_audit ORDER BY recorded_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SecretOverrideAuditRow {
                id: r.get(0)?,
                override_id: r.get(1)?,
                action: r.get(2)?,
                fingerprint: r.get(3)?,
                scope_kind: r.get(4)?,
                scope_id: r.get(5)?,
                recorded_at: r.get(6)?,
                actor: r.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn durable_round_trip_survives_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("r12.db");
        let key1 = {
            let store = Store::open(&path).unwrap();
            let key = store.ensure_scanner_hmac_key().unwrap();
            store
                .grant_secret_override("fp_abc", SecretOverrideScope::Global, true, Some("test"))
                .unwrap();
            key
        };
        let store2 = Store::open(&path).unwrap();
        assert_eq!(store2.ensure_scanner_hmac_key().unwrap(), key1);
        let active = store2.list_active_durable_secret_overrides().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].fingerprint, "fp_abc");
        assert_eq!(active[0].scope, SecretOverrideScope::Global);
    }

    #[test]
    fn revoke_removes_active_and_audits() {
        let store = Store::open(":memory:").unwrap();
        store
            .grant_secret_override(
                "fp_x",
                SecretOverrideScope::Session("s1".into()),
                true,
                Some("u"),
            )
            .unwrap();
        assert!(store
            .revoke_secret_override("fp_x", SecretOverrideScope::Session("s1".into()), Some("u"),)
            .unwrap());
        assert!(store
            .list_active_durable_secret_overrides()
            .unwrap()
            .is_empty());
        let audit = store.list_secret_override_audit().unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[0].action, "grant");
        assert_eq!(audit[1].action, "revoke");
        assert!(!audit.iter().any(|a| a.fingerprint.contains("secret")));
    }
}
