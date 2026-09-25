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
            self.add_team_member(org, team, subject)?;
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
}
