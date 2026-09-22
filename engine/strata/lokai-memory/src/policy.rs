//! Policy settings + session data class persistence (D1).

use rusqlite::{params, OptionalExtension};

use crate::{now, Result, Store, StoreError};

const KEY_POLICY_MODE: &str = "policy_mode";
const KEY_VERIFY_ALLOWED: &str = "policy_verify_allowed";
const KEY_MUTATIONS_ALLOWED: &str = "policy_mutations_allowed";
const KEY_ALLOW_SENSITIVE_TO_OWNER_ESTATE: &str = "placement_allow_sensitive_to_owner_estate";
const KEY_ALLOW_REPOSITORY_TO_ADMIN_MANAGED: &str = "placement_allow_repository_to_admin_managed";
const KEY_REQUIRED_VERIFICATION: &str = "placement_required_verification";

const VALID_POLICY_MODES: &[&str] = &["estate_stub", "full"];
const VALID_DATA_CLASSES: &[&str] = &[
    "public",
    "repository_source",
    "sensitive_source",
    "secret",
    "private",
    "personal",
    "circle_ok",
];

fn validate_policy_mode(mode: &str) -> Result<()> {
    if VALID_POLICY_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(StoreError::InvalidPolicyMode(mode.to_string()))
    }
}

fn validate_data_class(class: &str) -> Result<()> {
    if VALID_DATA_CLASSES.contains(&class) {
        Ok(())
    } else {
        Err(StoreError::InvalidDataClass(class.to_string()))
    }
}

fn canonical_data_class(class: &str) -> Result<String> {
    lokai_policy::normalize_data_class_name(class)
        .ok_or_else(|| StoreError::InvalidDataClass(class.to_string()))
}

impl Store {
    pub fn migrate_policy_v6(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 6 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS policy_settings (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN data_class TEXT NOT NULL DEFAULT 'personal'",
            [],
        );
        self.conn.execute(
            "INSERT OR IGNORE INTO policy_settings(key, value) VALUES (?1, ?2)",
            params![KEY_POLICY_MODE, "estate_stub"],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO policy_settings(key, value) VALUES (?1, ?2)",
            params![KEY_VERIFY_ALLOWED, "true"],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO policy_settings(key, value) VALUES (?1, ?2)",
            params![KEY_MUTATIONS_ALLOWED, "true"],
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (6, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    fn get_policy_setting(&self, key: &str, default: &str) -> Result<String> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM policy_settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| default.to_string()))
    }

    fn set_policy_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO policy_settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_policy_mode(&self) -> Result<String> {
        self.get_policy_setting(KEY_POLICY_MODE, "estate_stub")
    }

    pub fn set_policy_mode(&self, mode: &str) -> Result<()> {
        validate_policy_mode(mode)?;
        self.set_policy_setting(KEY_POLICY_MODE, mode)
    }

    pub fn get_policy_verify_allowed(&self) -> Result<bool> {
        Ok(self.get_policy_setting(KEY_VERIFY_ALLOWED, "true")? == "true")
    }

    pub fn set_policy_verify_allowed(&self, allowed: bool) -> Result<()> {
        self.set_policy_setting(KEY_VERIFY_ALLOWED, if allowed { "true" } else { "false" })
    }

    pub fn get_policy_mutations_allowed(&self) -> Result<bool> {
        Ok(self.get_policy_setting(KEY_MUTATIONS_ALLOWED, "true")? == "true")
    }

    pub fn set_policy_mutations_allowed(&self, allowed: bool) -> Result<()> {
        self.set_policy_setting(
            KEY_MUTATIONS_ALLOWED,
            if allowed { "true" } else { "false" },
        )
    }

    pub fn get_project_placement_policy(&self) -> Result<lokai_domain::ProjectPlacementPolicy> {
        let req_ver = self.get_policy_setting(KEY_REQUIRED_VERIFICATION, "")?;
        let required_verification = if req_ver.trim().is_empty() || req_ver == "none" {
            None
        } else {
            Some(req_ver)
        };
        Ok(lokai_domain::ProjectPlacementPolicy {
            allow_sensitive_to_owner_estate: self
                .get_policy_setting(KEY_ALLOW_SENSITIVE_TO_OWNER_ESTATE, "true")?
                == "true",
            allow_repository_to_admin_managed: self
                .get_policy_setting(KEY_ALLOW_REPOSITORY_TO_ADMIN_MANAGED, "false")?
                == "true",
            required_verification,
        })
    }

    pub fn set_project_placement_policy(
        &self,
        policy: &lokai_domain::ProjectPlacementPolicy,
    ) -> Result<()> {
        self.set_policy_setting(
            KEY_ALLOW_SENSITIVE_TO_OWNER_ESTATE,
            if policy.allow_sensitive_to_owner_estate {
                "true"
            } else {
                "false"
            },
        )?;
        self.set_policy_setting(
            KEY_ALLOW_REPOSITORY_TO_ADMIN_MANAGED,
            if policy.allow_repository_to_admin_managed {
                "true"
            } else {
                "false"
            },
        )?;
        self.set_policy_setting(
            KEY_REQUIRED_VERIFICATION,
            policy.required_verification.as_deref().unwrap_or(""),
        )?;
        Ok(())
    }

    pub fn set_session_data_class(&self, session_id: &str, class: &str) -> Result<()> {
        validate_data_class(class)?;
        let canonical = canonical_data_class(class)?;
        self.conn.execute(
            "UPDATE sessions SET data_class = ?2 WHERE id = ?1",
            params![session_id, canonical],
        )?;
        Ok(())
    }

    pub fn session_data_class(&self, session_id: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT data_class FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    /// Audited explicit reclassification (M2-2 downgrade / false-positive review).
    pub fn reclassify_session(
        &self,
        session_id: &str,
        new_class: &str,
        reason: &str,
        source: &str,
    ) -> Result<String> {
        validate_data_class(new_class)?;
        if reason.trim().is_empty() {
            return Err(StoreError::InvalidDataClass(
                "reclassification reason required".into(),
            ));
        }
        let canonical = canonical_data_class(new_class)?;
        let previous = self.session_data_class(session_id)?;
        self.conn.execute(
            "UPDATE sessions SET data_class = ?2 WHERE id = ?1",
            params![session_id, canonical],
        )?;
        self.conn.execute(
            "INSERT INTO classification_audit(session_id, previous_class, new_class, reason, source, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                previous,
                canonical,
                reason,
                source,
                now()
            ],
        )?;
        Ok(canonical)
    }

    /// Recent sessions for one workspace (newest first), excluding the current session.
    pub fn list_recent_sessions_for_workspace(
        &self,
        workspace_root: &std::path::Path,
        exclude_session_id: &str,
        limit: u32,
    ) -> Result<Vec<crate::SessionRow>> {
        let ws = self.normalize_workspace_path(workspace_root);
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.status, s.model, s.workspace_root, s.started_at,
                    (SELECT COUNT(*) FROM messages   m WHERE m.session_id = s.id),
                    (SELECT COUNT(*) FROM tool_calls  t WHERE t.session_id = s.id),
                    (SELECT COUNT(*) FROM file_changes f WHERE f.session_id = s.id)
             FROM sessions s
             WHERE s.workspace_root = ?1 AND s.id != ?2
             ORDER BY s.started_at DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![ws, exclude_session_id, limit], |r| {
                Ok(crate::SessionRow {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    model: r.get(2)?,
                    workspace_root: r.get(3)?,
                    started_at: r.get(4)?,
                    messages: r.get(5)?,
                    tool_calls: r.get(6)?,
                    file_changes: r.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Distinct paths edited recently in this workspace.
    pub fn recent_touched_paths(
        &self,
        workspace_root: &std::path::Path,
        limit: u32,
    ) -> Result<Vec<String>> {
        let ws = self.normalize_workspace_path(workspace_root);
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT fc.path
             FROM file_changes fc JOIN sessions s ON fc.session_id = s.id
             WHERE s.workspace_root = ?1
             ORDER BY fc.id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![ws, limit], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_mode_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        assert_eq!(store.get_policy_mode().unwrap(), "estate_stub");
        store.set_policy_mode("full").unwrap();
        assert_eq!(store.get_policy_mode().unwrap(), "full");
    }

    #[test]
    fn policy_mode_rejects_invalid() {
        let store = Store::open(":memory:").unwrap();
        assert!(store.set_policy_mode("strict").is_err());
    }

    #[test]
    fn policy_toggles_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        assert!(store.get_policy_verify_allowed().unwrap());
        assert!(store.get_policy_mutations_allowed().unwrap());
        store.set_policy_verify_allowed(false).unwrap();
        store.set_policy_mutations_allowed(false).unwrap();
        assert!(!store.get_policy_verify_allowed().unwrap());
        assert!(!store.get_policy_mutations_allowed().unwrap());
    }

    #[test]
    fn project_placement_policy_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        let policy = lokai_domain::ProjectPlacementPolicy {
            allow_sensitive_to_owner_estate: false,
            allow_repository_to_admin_managed: true,
            required_verification: Some("redundant:2".into()),
        };
        store.set_project_placement_policy(&policy).unwrap();
        let loaded = store.get_project_placement_policy().unwrap();
        assert_eq!(loaded, policy);
    }

    #[test]
    fn session_data_class_persisted() {
        let store = Store::open(":memory:").unwrap();
        let sid = store.start_session("/tmp/ws", "m", "m").unwrap();
        store.set_session_data_class(&sid, "circle_ok").unwrap();
        assert_eq!(
            store.session_data_class(&sid).unwrap().as_deref(),
            Some("sensitive_source")
        );
    }

    #[test]
    fn session_data_class_rejects_invalid() {
        let store = Store::open(":memory:").unwrap();
        let sid = store.start_session("/tmp/ws", "m", "m").unwrap();
        assert!(store.set_session_data_class(&sid, "work").is_err());
    }
}
