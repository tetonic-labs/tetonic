//! Approvals audit + remembered rules + persisted egress allowlist (M5 / D1 / CP2).

use rusqlite::{params, OptionalExtension};

use crate::{now, Result, Store};

#[derive(Debug, Clone)]
pub struct ApprovalRow {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub detail: String,
    pub decision: String,
    pub remembered: bool,
    pub decided_at: String,
}

#[derive(Debug, Clone)]
pub struct EgressAllowRow {
    pub label: String,
    pub ip: String,
    pub port: Option<u16>,
}

impl Store {
    pub fn migrate_trust_v9(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 9 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS approvals (
                 id          TEXT PRIMARY KEY,
                 session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 kind        TEXT NOT NULL,
                 detail      TEXT NOT NULL,
                 decision    TEXT NOT NULL,
                 remembered  INTEGER NOT NULL DEFAULT 0,
                 decided_at  TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_approvals_session ON approvals(session_id);
             CREATE TABLE IF NOT EXISTS approval_rules (
                 id         INTEGER PRIMARY KEY,
                 kind       TEXT NOT NULL,
                 pattern    TEXT NOT NULL UNIQUE,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS egress_allow_rules (
                 label TEXT PRIMARY KEY,
                 ip    TEXT NOT NULL,
                 port  INTEGER
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (9, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    pub fn get_approval(&self, id: &str) -> Result<Option<ApprovalRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.session_id, a.kind, a.detail, a.decision, a.remembered, a.decided_at
             FROM approvals a
             JOIN sessions s ON s.id = a.session_id
             WHERE a.id = ?1 AND s.context_id='legacy-local'",
        )?;
        let row = stmt
            .query_row(params![id], |r| {
                Ok(ApprovalRow {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    kind: r.get(2)?,
                    detail: r.get(3)?,
                    decision: r.get(4)?,
                    remembered: r.get::<_, i64>(5)? != 0,
                    decided_at: r.get(6)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn record_approval(
        &self,
        id: &str,
        session_id: &str,
        kind: &str,
        detail: &str,
        decision: &str,
        remembered: bool,
    ) -> Result<()> {
        self.require_legacy_or_audit_session(session_id)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO approvals(id, session_id, kind, detail, decision, remembered, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                session_id,
                kind,
                detail,
                decision,
                if remembered { 1 } else { 0 },
                now()
            ],
        )?;
        Ok(())
    }

    /// A remembered rule is global. Only a legacy session may install one.
    /// A private or unknown session records no rule and returns false.
    pub fn remember_session_approval_rule(
        &self,
        session_id: &str,
        kind: &str,
        pattern: &str,
    ) -> Result<bool> {
        if self.require_legacy_session(session_id).is_err() {
            return Ok(false);
        }
        self.add_approval_rule(kind, pattern)?;
        Ok(true)
    }

    pub fn add_approval_rule(&self, kind: &str, pattern: &str) -> Result<()> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO approval_rules(kind, pattern, created_at) VALUES (?1, ?2, ?3)",
            params![kind, pattern, now()],
        )?;
        Ok(())
    }

    pub fn list_approval_rules(&self, kind: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern FROM approval_rules WHERE kind = ?1 ORDER BY id")?;
        let rows = stmt
            .query_map(params![kind], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// True when `detail` matches a remembered rule (exact or prefix `pattern*`).
    pub fn approval_rule_matches(&self, kind: &str, detail: &str) -> Result<bool> {
        let detail = detail.trim();
        for pattern in self.list_approval_rules(kind)? {
            if pattern.ends_with('*') {
                let prefix = pattern.trim_end_matches('*').trim_end();
                if !prefix.is_empty()
                    && (detail == prefix || detail.starts_with(&format!("{prefix} ")))
                {
                    return Ok(true);
                }
            } else if detail == pattern {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn propose_tool_call(
        &self,
        id: &str,
        session_id: &str,
        tool: &str,
        args_json: &str,
    ) -> Result<()> {
        self.require_legacy_or_audit_session(session_id)?;
        let ts = now();
        self.conn.execute(
            "INSERT INTO tool_calls(id, session_id, tool, args_json, status, result_summary, created_at)
             VALUES (?1, ?2, ?3, ?4, 'proposed', '', ?5)
             ON CONFLICT(id) DO UPDATE SET
               tool = excluded.tool,
               args_json = excluded.args_json,
               status = 'proposed',
               result_summary = '',
               settled_at = NULL",
            params![id, session_id, tool, args_json, ts],
        )?;
        Ok(())
    }

    pub fn list_egress_allow_rules(&self) -> Result<Vec<EgressAllowRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT label, ip, port FROM egress_allow_rules ORDER BY label")?;
        let rows = stmt
            .query_map([], |r| {
                let port: Option<i64> = r.get(2)?;
                Ok(EgressAllowRow {
                    label: r.get(0)?,
                    ip: r.get(1)?,
                    port: port.map(|p| p as u16),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn upsert_egress_allow_rule(&self, label: &str, ip: &str, port: Option<u16>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO egress_allow_rules(label, ip, port) VALUES (?1, ?2, ?3)
             ON CONFLICT(label) DO UPDATE SET ip = excluded.ip, port = excluded.port",
            params![label, ip, port.map(|p| p as i64)],
        )?;
        Ok(())
    }

    pub fn remove_egress_allow_rule(&self, label: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM egress_allow_rules WHERE label = ?1",
            params![label],
        )?;
        Ok(n > 0)
    }

    pub fn approval_count(&self, session_id: &str) -> Result<i64> {
        if self.require_legacy_session(session_id).is_err() {
            return Ok(0);
        }
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM approvals WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_rules_match_exact_and_prefix() {
        let store = Store::open(":memory:").unwrap();
        store
            .add_approval_rule("run_shell", "cargo test -p lokai-core")
            .unwrap();
        store.add_approval_rule("run_shell", "pytest*").unwrap();
        assert!(store
            .approval_rule_matches("run_shell", "cargo test -p lokai-core")
            .unwrap());
        assert!(store
            .approval_rule_matches("run_shell", "pytest -q")
            .unwrap());
        assert!(!store
            .approval_rule_matches("run_shell", "pytest; curl evil")
            .unwrap());
        assert!(!store
            .approval_rule_matches("run_shell", "rm -rf /")
            .unwrap());
    }

    #[test]
    fn egress_rules_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        store
            .upsert_egress_allow_rule("gpu", "192.168.1.50", Some(11434))
            .unwrap();
        let rows = store.list_egress_allow_rules().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "gpu");
    }
}
