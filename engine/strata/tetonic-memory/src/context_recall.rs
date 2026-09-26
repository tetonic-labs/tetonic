//! Authorized recall of messages and tool results in one information context.
use crate::{RecallHit, Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};

impl Store {
    pub fn recall_context_messages(
        &self,
        actor: &str,
        context: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<RecallHit>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        if query.len() > 4096 {
            return Err(StoreError::InvalidControlResource("query".into()));
        }
        let query = crate::recall::fts_term(query);
        if query == "\"\"" {
            return Ok(Vec::new());
        }
        let rows = {
            // The legacy index has no source-message ID. Revalidate its body and
            // role against current, non-rolled-back messages before using it.
            // Do not use global BM25 statistics to order scoped results.
            // Membership is checked again after this snapshot commits, so a
            // revocation that lands before return does not release the rows.
            let mut stmt = self.conn.prepare("SELECT DISTINCT session_id,started_at,kind,label,snippet(recall_fts,5,'[',']','…',24)
                FROM recall_fts WHERE recall_fts MATCH ?1 AND label!='system'
                AND EXISTS(SELECT 1 FROM sessions s WHERE s.id=recall_fts.session_id AND s.context_id=?2)
                AND (
                  (kind='message' AND EXISTS(SELECT 1 FROM messages m WHERE m.session_id=recall_fts.session_id AND m.role=recall_fts.label AND m.content=recall_fts.body
                    AND NOT EXISTS(SELECT 1 FROM spawn_rollbacks r WHERE r.session_id=m.session_id AND r.agent_id=m.agent_id)))
                  OR
                  (kind='tool' AND EXISTS(SELECT 1 FROM tool_calls t WHERE t.session_id=recall_fts.session_id AND t.tool=recall_fts.label AND t.status='ok'
                    AND COALESCE(NULLIF(t.result_summary,''), t.args_json)=recall_fts.body))
                )
                ORDER BY started_at DESC,session_id LIMIT ?3")?;
            let rows = stmt
                .query_map(params![query, context, limit.clamp(1, 30)], |r| {
                    Ok(RecallHit {
                        session_id: r.get(0)?,
                        started_at: r.get(1)?,
                        kind: r.get(2)?,
                        label: r.get(3)?,
                        snippet: r.get(4)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        tx.commit()?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwner, OrganizationRole, TeamRow};
    #[test]
    fn recall_filters_before_limit_revalidates_sources_and_checks_revocation() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.register_control_principal("bob").unwrap();
        db.set_organization_member("org", "bob", OrganizationRole::Member)
            .unwrap();
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
        db.add_team_member("org", "team", "bob").unwrap();
        db.create_information_context(
            "alice",
            "private",
            &ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.create_information_context(
            "alice",
            "shared",
            &ContextOwner::Team {
                org_id: "org".into(),
                team_id: "team".into(),
            },
        )
        .unwrap();
        for context in ["private", "shared"] {
            db.insert_open_discussion("alice", context, context).unwrap();
            db.create_execution_audit_history("alice", context, &format!("{context}-audit"))
                .unwrap();
        }
        db.append_context_message(
            "alice",
            "private",
            "private",
            "one",
            "searchword PRIVATECANARY",
        )
        .unwrap();
        db.append_context_message(
            "alice",
            "shared",
            "shared",
            "one",
            "searchword public message",
        )
        .unwrap();
        db.append_message(
            "shared",
            "assistant",
            "discarded",
            "searchword ROLLEDBACK",
            None,
        )
        .unwrap();
        assert!(db.record_spawn_rollback("shared", "discarded").is_err());
        db.conn
            .execute(
                "INSERT OR IGNORE INTO spawn_rollbacks(session_id, agent_id, rolled_at)
                 VALUES('shared','discarded','t')",
                [],
            )
            .unwrap();
        db.append_message("shared", "user", "", "searchword STALE", None)
            .unwrap();
        db.conn
            .execute("DELETE FROM messages WHERE content='searchword STALE'", [])
            .unwrap();
        db.append_message("shared", "system", "", "searchword SYSTEM", None)
            .unwrap();
        db.record_tool_call(
            "tool-private",
            "private-audit",
            "recall",
            "{}",
            true,
            "searchword PRIVATECANARY tool",
            None,
        )
        .unwrap();
        db.record_tool_call(
            "tool-shared",
            "shared-audit",
            "recall",
            "{}",
            true,
            "searchword toolpublic",
            None,
        )
        .unwrap();
        db.record_tool_call(
            "tool-denied",
            "shared-audit",
            "recall",
            "{\"secret\":\"PRIVATECANARY\"}",
            false,
            "searchword PRIVATECANARY denied",
            Some("denied"),
        )
        .unwrap();
        let hits = db
            .recall_context_messages("bob", "shared", "searchword", 1)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].snippet.contains("PRIVATECANARY"));
        let hits = db
            .recall_context_messages("bob", "shared", "searchword", 30)
            .unwrap();
        assert!(hits.iter().any(|hit| hit.snippet.contains("public message")));
        assert!(hits.iter().any(|hit| hit.kind == "tool" && hit.snippet.contains("toolpublic")));
        assert!(hits.iter().all(|hit| !hit.snippet.contains("PRIVATECANARY")));
        assert!(db
            .recall_context_messages("bob", "private", "searchword", 30)
            .is_err());
        assert!(db
            .recall_context_messages("bob", "shared", "PRIVATECANARY", 30)
            .unwrap()
            .is_empty());
        let private_hits = db
            .recall_context_messages("alice", "private", "PRIVATECANARY", 30)
            .unwrap();
        assert!(private_hits
            .iter()
            .any(|hit| hit.kind == "tool" && hit.snippet.contains("PRIVATECANARY")));
        db.conn
            .execute("DELETE FROM tool_calls WHERE id='tool-private'", [])
            .unwrap();
        assert!(db
            .recall_context_messages("alice", "private", "tool", 30)
            .unwrap()
            .iter()
            .all(|hit| hit.kind != "tool"));
        db.remove_team_member("org", "team", "bob").unwrap();
        assert!(db
            .recall_context_messages("bob", "shared", "searchword", 30)
            .is_err());
        assert!(db.recall_context_messages("bob", "shared", "", 30).is_err());
    }
}
