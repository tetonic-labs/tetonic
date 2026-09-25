//! Trusted storage operations. Actor IDs must come from verified credentials.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextOwner {
    /// The authenticated actor owns the context; callers cannot name another owner.
    Private {
        org_id: String,
    },
    Team {
        org_id: String,
        team_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrganizationRole, TeamRow};
    #[test]
    fn private_and_team_content_require_participation_not_administration() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        for actor in ["alice", "bob"] {
            db.register_control_principal(actor).unwrap();
            db.set_organization_member("org", actor, OrganizationRole::Member)
                .unwrap();
        }
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
        let private = ContextOwner::Private {
            org_id: "org".into(),
        };
        let team = ContextOwner::Team {
            org_id: "org".into(),
            team_id: "team".into(),
        };
        db.create_information_context("alice", "private-a", &private)
            .unwrap();
        db.create_information_context("alice", "private-a", &private)
            .unwrap();
        db.create_information_context("alice", "shared", &team)
            .unwrap();
        assert!(db
            .create_information_context("bob", "private-a", &private)
            .is_err());
        assert!(db
            .create_information_context("admin", "admin-shared", &team)
            .is_err());
        for (id, context, content) in [
            ("p", "private-a", "PRIVATECANARY"),
            ("t", "shared", "team discussion"),
        ] {
            db.conn.execute("INSERT INTO sessions(id,workspace_root,mode,model,status,started_at,context_id) VALUES(?1,'same-workspace','test','test','ok','t',?2)",params![id,context]).unwrap();
            db.append_message(id, "user", "", content, None).unwrap();
        }
        assert_eq!(
            db.scoped_transcript("alice", "private-a", "p", 10).unwrap()[0].2,
            "PRIVATECANARY"
        );
        for actor in ["admin", "bob"] {
            assert!(db.scoped_transcript(actor, "private-a", "p", 10).is_err());
            assert!(db.scoped_transcript(actor, "shared", "t", 10).is_err());
        }
        db.append_message("t", "assistant", "rolled-back", "discarded branch", None)
            .unwrap();
        db.record_spawn_rollback("t", "rolled-back").unwrap();
        db.add_team_member("org", "team", "bob").unwrap();
        assert_eq!(
            db.scoped_transcript("bob", "shared", "t", 10).unwrap()[0].2,
            "team discussion"
        );
        assert!(db.scoped_transcript("bob", "shared", "p", 10).is_err());
        assert!(db
            .scoped_transcript("bob", "shared", "missing", 10)
            .is_err());
        assert_eq!(
            db.scoped_transcript("bob", "shared", "t", 200)
                .unwrap()
                .len(),
            1
        );
        db.remove_team_member("org", "team", "bob").unwrap();
        assert!(db.scoped_transcript("bob", "shared", "t", 10).is_err());
        db.remove_organization_member("org", "alice").unwrap();
        assert!(db.scoped_transcript("alice", "private-a", "p", 10).is_err());
        assert!(db.scoped_transcript("alice", "shared", "t", 10).is_err());
        assert!(!db.context_access("admin", "legacy-local").unwrap());
    }
}

impl Store {
    pub fn context_session_access(
        &self,
        actor: &str,
        context: &str,
        session: &str,
    ) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let allowed = self.context_access(actor, context)?
            && self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id=?2)",
                params![session, context],
                |r| r.get::<_, bool>(0),
            )?;
        tx.commit()?;
        Ok(allowed)
    }

    /// Content access is deliberately distinct from metadata administration.
    pub fn context_access(&self, actor: &str, context: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT EXISTS(
            SELECT 1 FROM information_contexts c
            JOIN organization_members m ON m.org_id=c.org_id AND m.principal_id=?1
            JOIN control_principals p ON p.principal_id=m.principal_id AND p.enabled=1
            WHERE c.context_id=?2 AND (
              (c.kind='private' AND c.owner_principal_id=?1) OR
              (c.kind='team' AND (
                EXISTS(SELECT 1 FROM teams t WHERE t.org_id=c.org_id AND t.team_id=c.team_id AND t.owner_principal_id=?1) OR
                EXISTS(SELECT 1 FROM team_members tm WHERE tm.org_id=c.org_id AND tm.team_id=c.team_id AND tm.principal_id=?1)
              ))))", params![actor,context], |r|r.get(0))?)
    }

    pub fn create_information_context(
        &self,
        actor: &str,
        id: &str,
        owner: &ContextOwner,
    ) -> Result<()> {
        if id.trim().is_empty() || id.contains('\0') || id.len() > 256 || id == "legacy-local" {
            return Err(StoreError::InvalidControlResource("context_id".into()));
        }
        let (org, team) = match owner {
            ContextOwner::Private { org_id } => (org_id.as_str(), None),
            ContextOwner::Team { org_id, team_id } => (org_id.as_str(), Some(team_id.as_str())),
        };
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let allowed: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM organization_members m JOIN control_principals p USING(principal_id) WHERE m.org_id=?1 AND m.principal_id=?2 AND p.enabled=1 AND (?3 IS NULL OR EXISTS(SELECT 1 FROM teams t WHERE t.org_id=?1 AND t.team_id=?3 AND t.owner_principal_id=?2) OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.org_id=?1 AND tm.team_id=?3 AND tm.principal_id=?2)))", params![org,actor,team], |r|r.get(0))?;
        if !allowed {
            return Err(StoreError::ControlAccessDenied);
        }
        let kind = if team.is_some() { "team" } else { "private" };
        let principal = if team.is_some() { None } else { Some(actor) };
        self.conn.execute("INSERT INTO information_contexts(context_id,kind,org_id,owner_principal_id,team_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(context_id) DO NOTHING", params![id,kind,org,principal,team])?;
        // No disclosure that an inaccessible context exists.
        if !self.context_access(actor, id)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let same: bool = self.conn.query_row("SELECT kind=?2 AND org_id=?3 AND owner_principal_id IS ?4 AND team_id IS ?5 FROM information_contexts WHERE context_id=?1", params![id,kind,org,principal,team], |r|r.get(0))?;
        if !same {
            return Err(StoreError::ControlResourceConflict);
        }
        tx.commit()?;
        Ok(())
    }

    /// Authorization and bounded history share one database snapshot. Never
    /// falls back to the legacy transcript API. Excludes rolled-back branches.
    pub fn scoped_transcript(
        &self,
        actor: &str,
        context: &str,
        session: &str,
        limit: u32,
    ) -> Result<Vec<(i64, String, String)>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        if !self.context_access(actor, context)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let bound: Option<String> = self
            .conn
            .query_row(
                "SELECT context_id FROM sessions WHERE id=?1",
                [session],
                |r| r.get(0),
            )
            .optional()?;
        if bound.as_deref() != Some(context) {
            return Err(StoreError::ControlAccessDenied);
        }
        let rows = {
            let mut stmt = self.conn.prepare("SELECT m.seq,m.role,m.content FROM messages m WHERE m.session_id=?1 AND NOT EXISTS(SELECT 1 FROM spawn_rollbacks r WHERE r.session_id=m.session_id AND r.agent_id=m.agent_id) ORDER BY m.seq DESC LIMIT ?2")?;
            let rows = stmt
                .query_map(params![session, limit.clamp(1, 200)], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        tx.commit()?;
        Ok(rows.into_iter().rev().collect())
    }
}
