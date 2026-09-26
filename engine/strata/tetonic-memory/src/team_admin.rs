use crate::{ControlPermission, Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};

impl Store {
    pub(crate) fn migrate_team_admin_v33(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=33)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn
            .execute_batch("ALTER TABLE control_admin_events ADD COLUMN team_id TEXT;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(33,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Verified actor only. Organization administrators and current owners may
    /// manage explicit team membership, not organization membership or ownership.
    pub fn administer_team_member(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        subject: &str,
        present: bool,
    ) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        if present {
            let member: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM organization_members m
                 JOIN control_principals p ON p.principal_id=m.principal_id AND p.enabled=1
                 WHERE m.org_id=?1 AND m.principal_id=?2)",
                params![org, subject],
                |r| r.get(0),
            )?;
            if !member {
                return Err(StoreError::ControlAccessDenied);
            }
            self.add_team_member(org, team, subject)?;
            self.ensure_team_participation_context(org, team, subject)?;
        } else {
            self.remove_team_member(org, team, subject)?;
        }
        let action = if present {
            "add_team_member"
        } else {
            "remove_team_member"
        };
        self.conn.execute("INSERT INTO control_admin_events(actor_kind,actor_principal_id,action,org_id,team_id,subject_principal_id,at) VALUES('authenticated_principal',?1,?2,?3,?4,?5,?6)", params![actor,action,org,team,subject,crate::util::now()])?;
        tx.commit()?;
        Ok(())
    }

    /// The caller's private working context for a team they own or belong to.
    /// A missing team and a non-participant are both denied. The context starts
    /// empty when it is first created here.
    pub fn ensure_actor_team_participation(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<String> {
        let id = crate::team_participation_context_id(org, team, actor)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let participates: bool = self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM organization_members m
                JOIN control_principals p ON p.principal_id=m.principal_id AND p.enabled=1
                WHERE m.org_id=?1 AND m.principal_id=?2
                  AND EXISTS(SELECT 1 FROM teams t WHERE t.org_id=?1 AND t.team_id=?3)
                  AND (
                    EXISTS(SELECT 1 FROM teams t WHERE t.org_id=?1 AND t.team_id=?3 AND t.owner_principal_id=?2)
                    OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.org_id=?1 AND tm.team_id=?3 AND tm.principal_id=?2)
                  )
            )",
            params![org, actor, team],
            |r| r.get(0),
        )?;
        if !participates {
            return Err(StoreError::ControlAccessDenied);
        }
        self.ensure_team_participation_context(org, team, actor)?;
        tx.commit()?;
        Ok(id)
    }

    /// A private working context for this principal's participation on the team.
    /// It starts empty. It is not their other private history and not the shared
    /// team context. Removing membership does not delete it.
    pub(crate) fn ensure_team_participation_context(
        &self,
        org: &str,
        team: &str,
        subject: &str,
    ) -> Result<()> {
        let id = crate::team_participation_context_id(org, team, subject)?;
        self.conn.execute(
            "INSERT INTO information_contexts(context_id,kind,org_id,owner_principal_id,team_id)
             VALUES(?1,'private',?2,?3,NULL)
             ON CONFLICT(context_id) DO NOTHING",
            params![id, org, subject],
        )?;
        let same: bool = self.conn.query_row(
            "SELECT kind='private' AND org_id=?2 AND owner_principal_id=?3 AND team_id IS NULL
             FROM information_contexts WHERE context_id=?1",
            params![id, org, subject],
            |r| r.get(0),
        )?;
        if same {
            Ok(())
        } else {
            Err(StoreError::ControlResourceConflict)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrganizationRole, TeamRow};

    #[test]
    fn upgrade_preserves_existing_admin_audit_and_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v32.db");
        {
            let db = Store::open(&path).unwrap();
            db.bootstrap_control("admin", "a", "A").unwrap();
            db.remove_context_schema_for_test();
            db.conn.execute_batch("DROP TABLE agent_identity_revisions; ALTER TABLE control_admin_events DROP COLUMN team_id; DELETE FROM schema_versions WHERE version>=33;").unwrap();
        }
        let db = Store::open(&path).unwrap();
        assert!(db
            .control_access("admin", ControlPermission::ManageOrganization, "a", "")
            .unwrap());
        let row: (String, Option<String>) = db
            .conn
            .query_row("SELECT action,team_id FROM control_admin_events", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, ("bootstrap".into(), None));
        drop(db);
        Store::open(&path).unwrap();
    }

    #[test]
    fn team_grants_enforce_scope_owner_and_atomic_audit() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "a", "A").unwrap();
        for id in ["owner", "member", "outsider"] {
            db.register_control_principal(id).unwrap();
        }
        for id in ["owner", "member"] {
            db.set_organization_member("a", id, OrganizationRole::Member)
                .unwrap();
        }
        for (team, owner) in [("one", "owner"), ("two", "admin")] {
            db.create_team(&TeamRow {
                org_id: "a".into(),
                team_id: team.into(),
                name: team.into(),
                owner_principal_id: owner.into(),
            })
            .unwrap();
        }
        assert!(db
            .administer_team_member("owner", "a", "two", "member", true)
            .is_err());
        assert!(db
            .administer_team_member("owner", "a", "one", "outsider", true)
            .is_err());
        db.administer_team_member("owner", "a", "one", "member", true)
            .unwrap();
        assert!(db
            .control_access("member", ControlPermission::ReadTeam, "a", "one")
            .unwrap());
        assert!(db
            .administer_team_member("member", "a", "one", "owner", false)
            .is_err());
        db.conn.execute_batch("CREATE TRIGGER reject_team BEFORE INSERT ON control_admin_events BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(db
            .administer_team_member("owner", "a", "one", "member", false)
            .is_err());
        assert!(db
            .control_access("member", ControlPermission::ReadTeam, "a", "one")
            .unwrap());
        db.conn.execute_batch("DROP TRIGGER reject_team;").unwrap();
        db.administer_team_member("owner", "a", "one", "member", false)
            .unwrap();
        assert!(!db
            .control_access("member", ControlPermission::ReadTeam, "a", "one")
            .unwrap());
        db.remove_organization_member("a", "owner").unwrap();
        assert!(db
            .administer_team_member("owner", "a", "one", "member", true)
            .is_err());
        let count: i64 = db.conn.query_row("SELECT count(*) FROM control_admin_events WHERE actor_principal_id='owner' AND team_id='one' AND subject_principal_id='member'", [], |r|r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn joining_a_team_creates_an_empty_private_working_context() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        for id in ["member", "other"] {
            db.register_control_principal(id).unwrap();
            db.set_organization_member("org", id, OrganizationRole::Member)
                .unwrap();
        }
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "admin".into(),
        })
        .unwrap();
        db.create_information_context(
            "member",
            "member-private",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("member", "member-private", "notes")
            .unwrap();
        db.append_context_message(
            "member",
            "member-private",
            "notes",
            "m1",
            "PRIVATECANARY",
        )
        .unwrap();
        db.administer_team_member("admin", "org", "team", "member", true)
            .unwrap();
        db.administer_team_member("admin", "org", "team", "member", true)
            .unwrap();
        let working = crate::team_participation_context_id("org", "team", "member").unwrap();
        assert!(db.context_access("member", &working).unwrap());
        assert!(!db.context_access("admin", &working).unwrap());
        assert!(!db.context_access("other", &working).unwrap());
        let sessions: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 0);
        let messages: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN sessions s ON s.id=m.session_id
                 WHERE s.context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(messages, 0);
        assert_eq!(
            db.scoped_transcript("member", "member-private", "notes", 10)
                .unwrap()[0]
                .2,
            "PRIVATECANARY"
        );
        let contexts: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM information_contexts WHERE context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(contexts, 1);
        db.administer_team_member("admin", "org", "team", "member", false)
            .unwrap();
        assert!(db.context_access("member", &working).unwrap());
        assert!(db.context_access("member", "member-private").unwrap());
    }

    #[test]
    fn creating_a_team_creates_the_owners_empty_working_context() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        db.register_control_principal("other").unwrap();
        db.set_organization_member("org", "other", OrganizationRole::Member)
            .unwrap();
        db.create_information_context(
            "admin",
            "owner-private",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("admin", "owner-private", "notes")
            .unwrap();
        db.append_context_message("admin", "owner-private", "notes", "m1", "PRIVATECANARY")
            .unwrap();
        let row = TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "admin".into(),
        };
        db.create_team(&row).unwrap();
        db.create_team(&row).unwrap();
        let working = crate::team_participation_context_id("org", "team", "admin").unwrap();
        assert!(db.context_access("admin", &working).unwrap());
        assert!(!db.context_access("other", &working).unwrap());
        let sessions: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 0);
        let contexts: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM information_contexts WHERE context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(contexts, 1);
        assert_eq!(
            db.scoped_transcript("admin", "owner-private", "notes", 10)
                .unwrap()[0]
                .2,
            "PRIVATECANARY"
        );
        assert!(db
            .recall_context_messages("other", &working, "PRIVATECANARY", 5)
            .is_err());
    }

    #[test]
    fn a_participant_uses_the_working_context_without_private_history() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        db.register_control_principal("member").unwrap();
        db.set_organization_member("org", "member", OrganizationRole::Member)
            .unwrap();
        db.create_information_context(
            "admin",
            "owner-private",
            &crate::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("admin", "owner-private", "notes")
            .unwrap();
        db.append_context_message("admin", "owner-private", "notes", "m1", "PRIVATECANARY")
            .unwrap();
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "admin".into(),
        })
        .unwrap();
        db.add_team_member("org", "team", "member").unwrap();
        assert!(db
            .ensure_actor_team_participation("member", "org", "missing")
            .is_err());
        assert!(db
            .ensure_actor_team_participation("other", "org", "team")
            .is_err());
        let working = db
            .ensure_actor_team_participation("member", "org", "team")
            .unwrap();
        assert_eq!(
            db.ensure_actor_team_participation("member", "org", "team")
                .unwrap(),
            working
        );
        assert_eq!(
            working,
            crate::team_participation_context_id("org", "team", "member").unwrap()
        );
        assert!(db.context_access("member", &working).unwrap());
        assert!(!db.context_access("admin", &working).unwrap());
        let messages: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN sessions s ON s.id=m.session_id
                 WHERE s.context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(messages, 0);
        let contexts: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM information_contexts WHERE context_id=?1",
                [&working],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(contexts, 1);
        assert!(db
            .recall_context_messages("member", &working, "PRIVATECANARY", 5)
            .unwrap()
            .is_empty());
        assert_eq!(
            db.scoped_transcript("admin", "owner-private", "notes", 10)
                .unwrap()[0]
                .2,
            "PRIVATECANARY"
        );
        let owner = db
            .ensure_actor_team_participation("admin", "org", "team")
            .unwrap();
        assert_ne!(owner, working);
        assert!(db
            .recall_context_messages("admin", &owner, "PRIVATECANARY", 5)
            .unwrap()
            .is_empty());
    }
}
