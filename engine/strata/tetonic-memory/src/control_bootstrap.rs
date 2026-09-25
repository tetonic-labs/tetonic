//! One-time local provisioning, serialized with other control writes.
use crate::{Result, Store, StoreError};
use rusqlite::{params, Transaction, TransactionBehavior};

impl Store {
    pub(crate) fn migrate_control_bootstrap_v32(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS control_admin_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            actor_kind TEXT NOT NULL,
            actor_principal_id TEXT,
            action TEXT NOT NULL,
            org_id TEXT NOT NULL,
            subject_principal_id TEXT NOT NULL,
            at TEXT NOT NULL
        );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(32,?1)",
            params![crate::util::now()],
        )?;
        Ok(())
    }

    /// Only a trusted local operator with database access may call this. It
    /// cannot be used to recover or replace an existing administrator.
    pub fn bootstrap_control(&self, principal: &str, org: &str, name: &str) -> Result<()> {
        for value in [principal, org, name] {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(StoreError::InvalidControlResource("bootstrap".into()));
            }
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let occupied: bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM control_principals) OR EXISTS(SELECT 1 FROM control_admin_events WHERE action='bootstrap')",[],|r|r.get(0))?;
        if occupied {
            return Err(StoreError::ControlResourceConflict);
        }
        self.put_control_principal(principal, true, true)?;
        self.conn.execute(
            "INSERT INTO organizations(org_id,name) VALUES(?1,?2)",
            params![org, name],
        )?;
        self.set_organization_member(org, principal, crate::OrganizationRole::Administrator)?;
        self.conn.execute("INSERT INTO control_admin_events(actor_kind,action,org_id,subject_principal_id,at) VALUES('local_operator','bootstrap',?1,?2,?3)",params![org,principal,crate::util::now()])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_rejects_takeover_and_rolls_back_failed_audit() {
        let db = Store::open(":memory:").unwrap();
        db.conn.execute_batch("CREATE TRIGGER reject_bootstrap BEFORE INSERT ON control_admin_events BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(db.bootstrap_control("local/admin", "a", "A").is_err());
        assert!(db.get_organization("a").unwrap().is_none());
        db.conn
            .execute_batch("DROP TRIGGER reject_bootstrap;")
            .unwrap();
        db.bootstrap_control("local/admin", "a", "A").unwrap();
        assert!(matches!(
            db.bootstrap_control("local/intruder", "b", "B"),
            Err(StoreError::ControlResourceConflict)
        ));
        assert!(db
            .control_access("local/admin", crate::ControlPermission::CreateTeam, "a", "")
            .unwrap());
        assert!(db.get_organization("b").unwrap().is_none());
        assert!(!db
            .control_access(
                "local/intruder",
                crate::ControlPermission::CreateOrganization,
                "",
                ""
            )
            .unwrap());
    }

    #[test]
    fn concurrent_bootstrap_has_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bootstrap.db");
        Store::open(&path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["alice", "bob"]
            .into_iter()
            .map(|id| {
                let db = Store::open(&path).unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    db.bootstrap_control(id, id, id)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(StoreError::ControlResourceConflict)))
                .count(),
            1
        );
        let db = Store::open(path).unwrap();
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM control_admin_events", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
