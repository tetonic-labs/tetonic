//! TLS persistence: public certificate, versioned opaque reference, and CAS revision.
//! OS key custody and TLS validation are deliberately outside this adapter.
use crate::worker_store::{WorkerResult, WorkerStore, WorkerStoreError};
use rusqlite::{params, OptionalExtension};
use tetonic_domain::key_storage::{SecretBytes, SecretKeyRef};

#[derive(Debug)]
pub enum WorkerTlsKey {
    Legacy(SecretBytes),
    Stored(SecretKeyRef),
    Revoked(Option<SecretKeyRef>),
}
#[derive(Debug)]
pub struct WorkerTlsIdentity {
    pub cert_der: Vec<u8>,
    pub revision: i64,
    pub key: WorkerTlsKey,
    pub scrub_pending: bool,
}

impl WorkerStore {
    pub fn tls_identity(&self) -> WorkerResult<Option<WorkerTlsIdentity>> {
        let row = self.conn.query_row(
            "SELECT cert_der, key_der, key_ref, revision, revoked, scrub_pending FROM worker_tls WHERE id = 1",
            [], |r| Ok((r.get::<_,Vec<u8>>(0)?, SecretBytes::new(r.get::<_,Vec<u8>>(1)?),
                r.get::<_,Option<String>>(2)?, r.get::<_,i64>(3)?, r.get::<_,bool>(4)?, r.get::<_,bool>(5)?))
        ).optional()?;
        let Some((cert_der, legacy, reference, revision, revoked, scrub_pending)) = row else {
            return Ok(None);
        };
        let key = match (revoked, reference, legacy.as_ref().is_empty()) {
            (true, reference, true) => WorkerTlsKey::Revoked(reference.map(SecretKeyRef)),
            (false, Some(reference), true) if !reference.is_empty() => {
                WorkerTlsKey::Stored(SecretKeyRef(reference))
            }
            (false, None, false) => WorkerTlsKey::Legacy(legacy),
            _ => {
                return Err(WorkerStoreError::InvalidIdentity(
                    "inconsistent key state; refusing regeneration",
                ))
            }
        };
        Ok(Some(WorkerTlsIdentity {
            cert_der,
            revision,
            key,
            scrub_pending,
        }))
    }

    /// Publish only after the key adapter has durably stored and verified the key.
    /// Returns false on a concurrent change; no private bytes can enter this API.
    pub fn publish_tls_identity(
        &self,
        expected_revision: Option<i64>,
        cert: &[u8],
        reference: &SecretKeyRef,
    ) -> WorkerResult<bool> {
        if cert.is_empty() || reference.0.is_empty() {
            return Err(WorkerStoreError::InvalidIdentity(
                "empty certificate or reference",
            ));
        }
        let changed = if let Some(revision) = expected_revision {
            self.conn.execute(
                "UPDATE worker_tls SET cert_der=?1, key_der=x'', key_ref=?2, revision=revision+1, revoked=0,
                 scrub_pending=CASE WHEN length(key_der)>0 THEN 1 ELSE scrub_pending END
                 WHERE id=1 AND revision=?3", params![cert, reference.0, revision])?
        } else {
            self.conn.execute("INSERT OR IGNORE INTO worker_tls(id,cert_der,key_der,key_ref,revision,revoked,scrub_pending)
                 VALUES(1,?1,x'',?2,1,0,0)", params![cert, reference.0])?
        };
        Ok(changed == 1)
    }

    /// Tombstone before deleting from the OS store. Restore of an old reference
    /// cannot silently generate a new identity when that key is gone.
    pub fn revoke_tls_identity(&self, expected_revision: i64) -> WorkerResult<bool> {
        Ok(self.conn.execute(
            "UPDATE worker_tls SET revoked=1, key_der=x'', revision=revision+1,
            scrub_pending=CASE WHEN length(key_der)>0 THEN 1 ELSE scrub_pending END
            WHERE id=1 AND revision=?1",
            [expected_revision],
        )? == 1)
    }

    /// Remove legacy live-database/WAL remnants. Existing external backups and
    /// storage snapshots are outside this operation's ownership.
    pub fn scrub_legacy_tls_key(&self) -> WorkerResult<()> {
        let pending: bool = self.conn.query_row(
            "SELECT COALESCE(MAX(scrub_pending),0) FROM worker_tls",
            [],
            |r| r.get(0),
        )?;
        if !pending {
            return Ok(());
        }
        // secure_delete was enabled before any row replacement. Checkpointing
        // flushes the scrubbed pages; TRUNCATE removes old WAL frames.
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 {
            return Err(WorkerStoreError::InvalidIdentity(
                "legacy key cleanup blocked by another database reader; stop old workers and retry",
            ));
        }
        self.conn.execute(
            "UPDATE worker_tls SET scrub_pending=0 WHERE scrub_pending<>0",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_revision_cannot_replace_or_revoke_newer_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkerStore::open(dir.path().join("worker.db")).unwrap();
        let first = SecretKeyRef("test:first".into());
        let second = SecretKeyRef("test:second".into());
        assert!(store
            .publish_tls_identity(None, b"first-cert", &first)
            .unwrap());
        let revision = store.tls_identity().unwrap().unwrap().revision;
        assert!(store
            .publish_tls_identity(Some(revision), b"second-cert", &second)
            .unwrap());
        assert!(!store
            .publish_tls_identity(Some(revision), b"first-cert", &first)
            .unwrap());
        assert!(!store.revoke_tls_identity(revision).unwrap());
        assert_eq!(
            store.tls_identity().unwrap().unwrap().cert_der,
            b"second-cert"
        );
    }

    #[test]
    fn schema_failure_rolls_back_columns_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE schema_versions(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL);
            INSERT INTO schema_versions VALUES(6,'old');
            CREATE TABLE worker_tls(id INTEGER PRIMARY KEY,cert_der BLOB NOT NULL,key_der BLOB NOT NULL);
            CREATE TRIGGER fail_marker BEFORE INSERT ON schema_versions WHEN NEW.version=7 BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
        assert!(WorkerStore::open(&path).is_err());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('worker_tls') WHERE name='key_ref'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 6);
        conn.execute_batch("DROP TRIGGER fail_marker").unwrap();
        assert!(WorkerStore::open(&path).is_ok());
    }

    #[test]
    fn newer_worker_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_versions(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL);
            INSERT INTO schema_versions VALUES(99,'future');",
        )
        .unwrap();
        assert!(WorkerStore::open(&path).is_err());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='worker_tls'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
