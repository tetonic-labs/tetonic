//! Local control credentials. Only hashes are persisted; mutation doors are
//! trusted provisioning operations, not unauthenticated API handlers.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

pub struct ControlCredentialRow {
    pub credential_id: String,
    pub principal_id: String,
    pub audience: String,
    pub secret_hash: Vec<u8>,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl Store {
    pub(crate) fn migrate_control_credentials_v31(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=31)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS control_credentials (
            credential_id TEXT PRIMARY KEY NOT NULL,
            principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
            audience TEXT NOT NULL,
            secret_hash BLOB NOT NULL UNIQUE CHECK(length(secret_hash)=32),
            issued_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL CHECK(expires_at>issued_at),
            revoked_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS control_credential_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            credential_id TEXT NOT NULL REFERENCES control_credentials(credential_id),
            principal_id TEXT NOT NULL,
            action TEXT NOT NULL CHECK(action IN ('issued','revoked')),
            at INTEGER NOT NULL
        );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(31,?1)",
            params![crate::util::now()],
        )?;
        Ok(())
    }

    pub fn issue_control_credential(&self, row: &ControlCredentialRow) -> Result<()> {
        if row.credential_id.trim().is_empty()
            || row.audience.trim().is_empty()
            || row.secret_hash.len() != 32
            || row.issued_at < 0
            || row.expires_at <= row.issued_at
        {
            return Err(StoreError::InvalidControlResource("credential".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let inserted = self.conn.execute(
            "INSERT INTO control_credentials
            (credential_id,principal_id,audience,secret_hash,issued_at,expires_at)
            SELECT ?1,?2,?3,?4,?5,?6 FROM control_principals WHERE principal_id=?2 AND enabled=1",
            params![
                row.credential_id,
                row.principal_id,
                row.audience,
                row.secret_hash,
                row.issued_at,
                row.expires_at
            ],
        )?;
        if inserted != 1 {
            return Err(StoreError::InvalidControlResource("principal".into()));
        }
        self.conn.execute("INSERT INTO control_credential_events(credential_id,principal_id,action,at) VALUES(?1,?2,'issued',?3)",params![row.credential_id,row.principal_id,row.issued_at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn control_credential_principal(
        &self,
        secret_hash: &[u8],
        audience: &str,
        now: i64,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT c.principal_id FROM control_credentials c
            JOIN control_principals p ON p.principal_id=c.principal_id
            WHERE c.secret_hash=?1 AND c.audience=?2 AND c.issued_at<=?3 AND c.expires_at>?3
            AND c.revoked_at IS NULL AND p.enabled=1",
                params![secret_hash, audience, now],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Idempotent revoke, with one audit event on the first state transition.
    pub fn revoke_control_credential(
        &self,
        credential_id: &str,
        audience: &str,
        now: i64,
    ) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.conn.execute(
            "INSERT INTO control_credential_events(credential_id,principal_id,action,at)
            SELECT credential_id,principal_id,'revoked',?3 FROM control_credentials
            WHERE credential_id=?1 AND audience=?2 AND revoked_at IS NULL",
            params![credential_id, audience, now],
        )?;
        self.conn.execute("UPDATE control_credentials SET revoked_at=?3 WHERE credential_id=?1 AND audience=?2 AND revoked_at IS NULL",params![credential_id,audience,now])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row() -> ControlCredentialRow {
        ControlCredentialRow {
            credential_id: "credential".into(),
            principal_id: "local/alice".into(),
            audience: "engine-a".into(),
            secret_hash: vec![42; 32],
            issued_at: 100,
            expires_at: 200,
        }
    }
    #[test]
    fn expiry_scope_revocation_and_principal_disablement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.db");
        {
            let db = Store::open(&path).unwrap();
            assert!(db.issue_control_credential(&row()).is_err());
            db.put_control_principal("local/alice", true, false)
                .unwrap();
            db.issue_control_credential(&row()).unwrap();
            assert!(db.issue_control_credential(&row()).is_err());
        }
        let db = Store::open(path).unwrap();
        for (hash, audience, time) in [
            (vec![42; 32], "engine-a", 99),
            (vec![42; 32], "engine-a", 200),
            (vec![42; 32], "engine-b", 150),
            (vec![1; 32], "engine-a", 150),
        ] {
            assert!(db
                .control_credential_principal(&hash, audience, time)
                .unwrap()
                .is_none());
        }
        assert_eq!(
            db.control_credential_principal(&row().secret_hash, "engine-a", 100)
                .unwrap()
                .as_deref(),
            Some("local/alice")
        );
        db.put_control_principal("local/alice", false, false)
            .unwrap();
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_none());
        db.put_control_principal("local/alice", true, false)
            .unwrap();
        db.revoke_control_credential("credential", "engine-b", 150)
            .unwrap();
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_some());
        db.revoke_control_credential("credential", "engine-a", 150)
            .unwrap();
        db.revoke_control_credential("credential", "engine-a", 151)
            .unwrap();
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_none());
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM control_credential_events", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            2
        );
    }

    #[test]
    fn audit_failure_rolls_back_issuance_and_revocation() {
        let db = Store::open(":memory:").unwrap();
        db.put_control_principal("local/alice", true, false)
            .unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON control_credential_events BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(db.issue_control_credential(&row()).is_err());
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_none());
        db.conn.execute_batch("DROP TRIGGER fail_audit;").unwrap();
        db.issue_control_credential(&row()).unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON control_credential_events BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(db
            .revoke_control_credential("credential", "engine-a", 150)
            .is_err());
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_some());
    }

    #[test]
    fn migration_from_v30_preserves_principals_without_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
        {
            let db = Store::open(&path).unwrap();
            db.put_control_principal("local/alice", true, true).unwrap();
            db.conn.execute_batch("DROP TABLE agent_identity_revisions; DROP TABLE control_admin_events; DROP TABLE control_credential_events; DROP TABLE control_credentials; DELETE FROM schema_versions WHERE version>=31;").unwrap();
        }
        let db = Store::open(path).unwrap();
        assert!(db
            .control_access(
                "local/alice",
                crate::ControlPermission::CreateOrganization,
                "",
                ""
            )
            .unwrap());
        assert!(db
            .control_credential_principal(&row().secret_hash, "engine-a", 150)
            .unwrap()
            .is_none());
        db.issue_control_credential(&row()).unwrap();
    }
}
