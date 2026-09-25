//! Episodic recall over the audit log (T8 / M8 v1 — FTS5 on messages + tool results).

use std::path::Path;

use rusqlite::{params, OptionalExtension};

use crate::{now, Result, Store};

#[derive(Debug, Clone)]
pub struct RecallHit {
    pub session_id: String,
    pub started_at: String,
    pub kind: String,
    pub label: String,
    pub snippet: String,
}

impl Store {
    pub fn migrate_recall_v8(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 8 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS recall_fts USING fts5(
                 workspace_root UNINDEXED,
                 session_id UNINDEXED,
                 started_at UNINDEXED,
                 kind UNINDEXED,
                 label UNINDEXED,
                 body,
                 tokenize='unicode61'
             );",
        )?;
        self.backfill_recall_fts()?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (8, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    pub(crate) fn backfill_recall_fts(&self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM recall_fts", [], |r| r.get(0))
            .unwrap_or(0);
        if count > 0 {
            return Ok(());
        }
        self.conn.execute_batch(
            "INSERT INTO recall_fts(workspace_root, session_id, started_at, kind, label, body)
             SELECT s.workspace_root, m.session_id, s.started_at, 'message', m.role, m.content
             FROM messages m JOIN sessions s ON m.session_id = s.id
             WHERE length(trim(m.content)) > 0 AND m.role != 'system';",
        )?;
        self.conn.execute_batch(
            "INSERT INTO recall_fts(workspace_root, session_id, started_at, kind, label, body)
             SELECT s.workspace_root, t.session_id, s.started_at, 'tool', t.tool,
                    COALESCE(NULLIF(t.result_summary, ''), t.args_json)
             FROM tool_calls t JOIN sessions s ON t.session_id = s.id
             WHERE t.status = 'ok';",
        )?;
        Ok(())
    }

    pub(crate) fn index_recall_message(
        &self,
        workspace_root: &str,
        session_id: &str,
        started_at: &str,
        role: &str,
        content: &str,
    ) -> Result<()> {
        if role == "system" || content.trim().is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO recall_fts(workspace_root, session_id, started_at, kind, label, body)
             VALUES (?1, ?2, ?3, 'message', ?4, ?5)",
            params![workspace_root, session_id, started_at, role, content],
        )?;
        Ok(())
    }

    pub(crate) fn index_recall_tool(
        &self,
        workspace_root: &str,
        session_id: &str,
        started_at: &str,
        tool: &str,
        body: &str,
    ) -> Result<()> {
        if body.trim().is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO recall_fts(workspace_root, session_id, started_at, kind, label, body)
             VALUES (?1, ?2, ?3, 'tool', ?4, ?5)",
            params![workspace_root, session_id, started_at, tool, body],
        )?;
        Ok(())
    }

    pub(crate) fn session_started_at(&self, session_id: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT started_at FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    pub fn session_workspace(&self, session_id: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT workspace_root FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    /// Keyword search over prior sessions in this workspace (excludes current session).
    pub fn recall_history(
        &self,
        workspace_root: &Path,
        query: &str,
        limit: u32,
        exclude_session_id: Option<&str>,
    ) -> Result<Vec<RecallHit>> {
        let ws = self.normalize_workspace_path(workspace_root);
        let fts = fts_term(query);
        if fts == "\"\"" {
            return Ok(Vec::new());
        }
        let lim = limit.clamp(1, 30);
        let mut sql = String::from(
            "SELECT session_id, started_at, kind, label,
                    snippet(recall_fts, 5, '[', ']', '…', 24) AS snip
             FROM recall_fts
             WHERE recall_fts MATCH ?1 AND workspace_root = ?2 AND label != 'system' AND EXISTS(SELECT 1 FROM sessions scoped WHERE scoped.id=recall_fts.session_id AND scoped.context_id='legacy-local')",
        );
        if exclude_session_id.is_some() {
            sql.push_str(" AND session_id != ?4");
        }
        sql.push_str(" ORDER BY rank LIMIT ?3");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(ex) = exclude_session_id {
            stmt.query_map(params![fts, ws, lim, ex], map_recall_row)?
        } else {
            stmt.query_map(params![fts, ws, lim], map_recall_row)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Recent `finish` summaries from prior sessions (briefing v4).
    pub fn recent_finish_outcomes(
        &self,
        workspace_root: &Path,
        exclude_session_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<(String, String)>> {
        let ws = self.normalize_workspace_path(workspace_root);
        let lim = limit.clamp(1, 5);
        let mut sql = String::from(
            "SELECT s.started_at, t.result_summary
             FROM tool_calls t
             JOIN sessions s ON t.session_id = s.id
             WHERE s.workspace_root = ?1 AND s.context_id='legacy-local' AND t.tool = 'finish' AND t.status = 'ok'
               AND length(trim(t.result_summary)) > 0",
        );
        if exclude_session_id.is_some() {
            sql.push_str(" AND s.id != ?3");
        }
        sql.push_str(" ORDER BY t.settled_at DESC LIMIT ?2");

        let mut stmt = self.conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(String, String)> {
            Ok((r.get(0)?, r.get(1)?))
        };
        let rows = if let Some(ex) = exclude_session_id {
            stmt.query_map(params![ws, lim, ex], map)?
        } else {
            stmt.query_map(params![ws, lim], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Count egress log rows for a session (daemon mirror verification).
    pub fn egress_count_for_session(&self, session_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM egress_log WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?)
    }
}

/// Hook after a message row is inserted (recall FTS index).
pub(crate) fn after_message_insert(store: &Store, session_id: &str, role: &str, content: &str) {
    let Ok(Some(ws)) = store.session_workspace(session_id) else {
        return;
    };
    let Ok(Some(started)) = store.session_started_at(session_id) else {
        return;
    };
    if let Err(e) = store.index_recall_message(&ws, session_id, &started, role, content) {
        tracing::warn!(%session_id, error = %e, "recall FTS index failed for message");
    }
}

/// Hook after a successful tool call is recorded (recall FTS index).
pub(crate) fn after_tool_insert(store: &Store, session_id: &str, tool: &str, body: &str) {
    let Ok(Some(ws)) = store.session_workspace(session_id) else {
        return;
    };
    let Ok(Some(started)) = store.session_started_at(session_id) else {
        return;
    };
    if let Err(e) = store.index_recall_tool(&ws, session_id, &started, tool, body) {
        tracing::warn!(%session_id, tool, error = %e, "recall FTS index failed for tool");
    }
}

fn map_recall_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RecallHit> {
    Ok(RecallHit {
        session_id: r.get(0)?,
        started_at: r.get(1)?,
        kind: r.get(2)?,
        label: r.get(3)?,
        snippet: r.get(4)?,
    })
}

fn fts_term(q: &str) -> String {
    let cleaned: String = q
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.is_empty() {
        "\"\"".to_string()
    } else {
        tokens
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recall_finds_prior_session_message() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("lokai.db");
        let store = Store::open(&db).unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let ws = root.to_string_lossy().to_string();

        let sid1 = store.start_session(&ws, "single-agent", "m").unwrap();
        store
            .append_message(
                &sid1,
                "user",
                "",
                "implement auth retries with backoff",
                None,
            )
            .unwrap();
        store
            .record_tool_call(
                "tc1",
                &sid1,
                "finish",
                r#"{"summary":"added exponential backoff"}"#,
                true,
                "added exponential backoff",
                None,
            )
            .unwrap();
        store.end_session(&sid1, "ok", None).unwrap();

        let sid2 = store.start_session(&ws, "single-agent", "m").unwrap();
        let hits = store
            .recall_history(&root, "backoff auth", 5, Some(&sid2))
            .unwrap();
        assert!(
            hits.iter()
                .any(|h| h.snippet.contains("backoff") || h.snippet.contains("auth")),
            "expected recall hit, got: {hits:?}"
        );
    }

    #[test]
    fn recall_excludes_system_messages() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("lokai.db");
        let store = Store::open(&db).unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let ws = root.to_string_lossy().to_string();

        let sid1 = store.start_session(&ws, "single-agent", "m").unwrap();
        store
            .append_message(&sid1, "system", "", "SECRET_SYSTEM_PROMPT_TOKEN", None)
            .unwrap();
        store.end_session(&sid1, "ok", None).unwrap();

        let sid2 = store.start_session(&ws, "single-agent", "m").unwrap();
        let hits = store
            .recall_history(&root, "SECRET_SYSTEM_PROMPT", 5, Some(&sid2))
            .unwrap();
        assert!(
            hits.is_empty(),
            "system messages must not appear in recall, got: {hits:?}"
        );
    }
}
