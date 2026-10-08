//! Membership changes serialize authority, mutation and audit in one transaction.
use crate::{ControlPermission, OrganizationRole, Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};

impl Store {
    /// Trusted local provisioning only. Retry cannot re-enable or promote an identity.
    pub fn register_control_principal(&self, principal: &str) -> Result<()> {
        if principal.trim().is_empty() || principal.contains('\0') {
            return Err(StoreError::InvalidControlResource("principal".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let inserted = self.conn.execute(
            "INSERT INTO control_principals VALUES(?1,1,0) ON CONFLICT DO NOTHING",
            [principal],
        )?;
        if inserted == 1 {
            self.conn.execute("INSERT INTO control_admin_events(actor_kind,action,org_id,subject_principal_id,at) VALUES('local_operator','register_principal','',?1,?2)", params![principal,crate::util::now()])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Actor must originate from verified authentication. Recheck durable admin
    /// authority under the write lock; this does not revalidate a bearer token.
    /// None removes membership (and cascades explicit team memberships).
    pub fn administer_organization_member(
        &self,
        actor: &str,
        org: &str,
        subject: &str,
        role: Option<OrganizationRole>,
    ) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(actor, ControlPermission::ManageOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        // Never let this door remove the final enabled administrator.
        if role != Some(OrganizationRole::Administrator) {
            let last: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM organization_members m JOIN control_principals p USING(principal_id) WHERE m.org_id=?1 AND m.principal_id=?2 AND m.role='administrator' AND p.enabled=1) AND NOT EXISTS(SELECT 1 FROM organization_members m JOIN control_principals p USING(principal_id) WHERE m.org_id=?1 AND m.principal_id<>?2 AND m.role='administrator' AND p.enabled=1)",params![org,subject],|r|r.get(0))?;
            if last {
                return Err(StoreError::LastOrganizationAdministrator);
            }
        }
        let action = if let Some(role) = role {
            self.set_organization_member(org, subject, role)?;
            format!("set_organization_member:{}", role.as_str())
        } else {
            self.remove_organization_member(org, subject)?;
            "remove_organization_member".into()
        };
        self.conn.execute("INSERT INTO control_admin_events(actor_kind,actor_principal_id,action,org_id,subject_principal_id,at) VALUES('authenticated_principal',?1,?2,?3,?4,?5)",params![actor,action,org,subject,crate::util::now()])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_admin_removals_cannot_remove_both_administrators() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("admins.db");
        let db = Store::open(&path).unwrap();
        db.bootstrap_control("alice", "a", "A").unwrap();
        db.register_control_principal("bob").unwrap();
        db.administer_organization_member(
            "alice",
            "a",
            "bob",
            Some(OrganizationRole::Administrator),
        )
        .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [("alice", "bob"), ("bob", "alice")]
            .into_iter()
            .map(|(actor, subject)| {
                let db = Store::open(&path).unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    db.administer_organization_member(actor, "a", subject, None)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(StoreError::ControlAccessDenied)))
                .count(),
            1
        );
        let count: i64 = db.conn.query_row("SELECT count(*) FROM organization_members WHERE org_id='a' AND role='administrator'", [], |r|r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn admin_changes_deny_escalation_and_preserve_last_enabled_admin() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "a", "A").unwrap();
        db.register_control_principal("bob").unwrap();
        let admin = Some(OrganizationRole::Administrator);
        assert!(matches!(
            db.administer_organization_member("bob", "a", "bob", admin),
            Err(StoreError::ControlAccessDenied)
        ));
        assert!(matches!(
            db.administer_organization_member("admin", "b", "bob", admin),
            Err(StoreError::ControlAccessDenied)
        ));
        assert!(matches!(
            db.administer_organization_member("admin", "a", "admin", None),
            Err(StoreError::LastOrganizationAdministrator)
        ));
        db.administer_organization_member("admin", "a", "bob", admin)
            .unwrap();
        db.administer_organization_member("bob", "a", "admin", None)
            .unwrap();
        assert!(matches!(
            db.administer_organization_member("admin", "a", "admin", admin),
            Err(StoreError::ControlAccessDenied)
        ));
        db.put_control_principal("admin", false, false).unwrap();
        db.register_control_principal("admin").unwrap();
        assert!(!db
            .control_access("admin", ControlPermission::CreateOrganization, "", "")
            .unwrap());
        let events: i64 = db.conn.query_row("SELECT count(*) FROM control_admin_events WHERE actor_kind='authenticated_principal'", [], |r|r.get(0)).unwrap();
        assert_eq!(events, 2);
    }

    #[test]
    fn audit_failure_rolls_back_membership_and_registration() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("admin", "a", "A").unwrap();
        db.register_control_principal("bob").unwrap();
        db.conn.execute_batch("CREATE TRIGGER reject_admin BEFORE INSERT ON control_admin_events BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(db
            .administer_organization_member(
                "admin",
                "a",
                "bob",
                Some(OrganizationRole::Administrator)
            )
            .is_err());
        assert!(!db
            .control_access("bob", ControlPermission::ReadOrganization, "a", "")
            .unwrap());
        assert!(db.register_control_principal("charlie").is_err());
        let count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM control_principals WHERE principal_id='charlie'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
