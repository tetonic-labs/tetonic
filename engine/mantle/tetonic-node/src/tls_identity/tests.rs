use super::*;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};
use tetonic_domain::key_storage::KeyStorageError;

#[derive(Default)]
pub(crate) struct MemoryKeys {
    entries: Mutex<HashMap<String, Vec<u8>>>,
    seq: AtomicUsize,
    fail_create: AtomicBool,
    fail_delete: AtomicBool,
    corrupt_create: AtomicBool,
}
impl KeyStorage for MemoryKeys {
    fn create(&self, secret: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(KeyStorageError::Unavailable);
        }
        let reference = format!("test:v1:{}", self.seq.fetch_add(1, Ordering::SeqCst));
        self.entries.lock().unwrap().insert(
            reference.clone(),
            if self.corrupt_create.load(Ordering::SeqCst) {
                b"wrong key".to_vec()
            } else {
                secret.to_vec()
            },
        );
        Ok(SecretKeyRef(reference))
    }
    fn read(&self, reference: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        self.entries
            .lock()
            .unwrap()
            .get(&reference.0)
            .cloned()
            .map(SecretBytes::new)
            .ok_or(KeyStorageError::Missing)
    }
    fn delete(&self, reference: &SecretKeyRef) -> Result<(), KeyStorageError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(KeyStorageError::Unavailable);
        }
        self.entries.lock().unwrap().remove(&reference.0);
        Ok(())
    }
}
fn stored_ref(store: &WorkerStore) -> SecretKeyRef {
    match store.tls_identity().unwrap().unwrap().key {
        WorkerTlsKey::Stored(r) => r,
        _ => panic!("expected protected reference"),
    }
}
fn legacy_database(path: &std::path::Path) -> TlsIdentity {
    let identity = generate().unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE schema_versions(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
        INSERT INTO schema_versions VALUES(6,'legacy');
        CREATE TABLE worker_tls(id INTEGER PRIMARY KEY CHECK(id=1), cert_der BLOB NOT NULL, key_der BLOB NOT NULL);").unwrap();
    conn.execute(
        "INSERT INTO worker_tls VALUES(1,?1,?2)",
        rusqlite::params![identity.certificate, identity.private_key.as_ref()],
    )
    .unwrap();
    identity
}
fn assert_no_key_bytes(dir: &std::path::Path, key: &[u8]) {
    for file in std::fs::read_dir(dir).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes.windows(key.len()).any(|w| w == key),
                "private DER in {}",
                path.display()
            );
        }
    }
}

#[test]
fn new_identity_is_reference_only_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    let keys = MemoryKeys::default();
    let store = WorkerStore::open(&path).unwrap();
    let first = load_or_create_tls_identity(&store, &keys).unwrap();
    assert_no_key_bytes(dir.path(), first.private_key.as_ref());
    drop(store);
    let reopened = WorkerStore::open(&path).unwrap();
    let second = load_or_create_tls_identity(&reopened, &keys).unwrap();
    assert_eq!(first.certificate, second.certificate);
    assert_eq!(first.private_key.as_ref(), second.private_key.as_ref());
    assert_eq!(keys.entries.lock().unwrap().len(), 1);
}

#[test]
fn legacy_migration_preserves_identity_and_scrubs_live_database_and_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    let legacy = legacy_database(&path);
    assert!(std::fs::read(&path)
        .unwrap()
        .windows(legacy.private_key.as_ref().len())
        .any(|w| w == legacy.private_key.as_ref()));
    let keys = MemoryKeys::default();
    let store = WorkerStore::open(&path).unwrap();
    let migrated = load_or_create_tls_identity(&store, &keys).unwrap();
    assert_eq!(legacy.certificate, migrated.certificate);
    assert_eq!(legacy.private_key.as_ref(), migrated.private_key.as_ref());
    assert!(!store.tls_identity().unwrap().unwrap().scrub_pending);
    assert_no_key_bytes(dir.path(), legacy.private_key.as_ref());
    drop(store);
    let reopened = WorkerStore::open(&path).unwrap();
    assert_eq!(
        load_or_create_tls_identity(&reopened, &keys)
            .unwrap()
            .certificate,
        legacy.certificate
    );
}

#[test]
fn unavailable_vault_preserves_legacy_identity_and_refuses_new_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let legacy = legacy_database(&path);
    let keys = MemoryKeys::default();
    keys.fail_create.store(true, Ordering::SeqCst);
    let store = WorkerStore::open(&path).unwrap();
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    match store.tls_identity().unwrap().unwrap().key {
        WorkerTlsKey::Legacy(key) => assert_eq!(key.as_ref(), legacy.private_key.as_ref()),
        _ => panic!("migration must preserve the original on vault failure"),
    }
    let empty = WorkerStore::open(dir.path().join("new.db")).unwrap();
    assert!(load_or_create_tls_identity(&empty, &keys).is_err());
    assert!(empty.tls_identity().unwrap().is_none());
}

#[test]
fn missing_or_mismatched_stored_key_never_silently_regenerates() {
    let dir = tempfile::tempdir().unwrap();
    let store = WorkerStore::open(dir.path().join("worker.db")).unwrap();
    let keys = MemoryKeys::default();
    let identity = load_or_create_tls_identity(&store, &keys).unwrap();
    let reference = stored_ref(&store);
    let another = generate().unwrap();
    keys.entries
        .lock()
        .unwrap()
        .insert(reference.0.clone(), another.private_key.as_ref().to_vec());
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    keys.delete(&reference).unwrap();
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    assert_eq!(keys.seq.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.tls_identity().unwrap().unwrap().cert_der,
        identity.certificate
    );
}

#[test]
fn protected_backup_restores_with_vault_but_not_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    let store = WorkerStore::open(&path).unwrap();
    let keys = MemoryKeys::default();
    let first = load_or_create_tls_identity(&store, &keys).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let backup = dir.path().join("backup.db");
    std::fs::copy(&path, &backup).unwrap();
    let old_reference = stored_ref(&store);
    let rotated = rotate_tls_identity(&store, &keys).unwrap();
    assert_ne!(rotated.certificate, first.certificate);
    let restored = WorkerStore::open(&backup).unwrap();
    assert_eq!(
        load_or_create_tls_identity(&restored, &keys)
            .unwrap()
            .certificate,
        first.certificate
    );
    assert_no_key_bytes(dir.path(), first.private_key.as_ref());
    assert_no_key_bytes(dir.path(), rotated.private_key.as_ref());
    keys.delete(&old_reference).unwrap();
    assert!(load_or_create_tls_identity(&restored, &keys).is_err());
    assert!(load_or_create_tls_identity(&store, &keys).is_ok());
}

#[test]
fn revocation_fails_closed_and_key_deletion_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let store = WorkerStore::open(dir.path().join("worker.db")).unwrap();
    let keys = MemoryKeys::default();
    load_or_create_tls_identity(&store, &keys).unwrap();
    let reference = stored_ref(&store);
    keys.fail_delete.store(true, Ordering::SeqCst);
    assert!(revoke_tls_identity(&store, &keys).is_err());
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    assert!(matches!(
        store.tls_identity().unwrap().unwrap().key,
        WorkerTlsKey::Revoked(_)
    ));
    keys.fail_delete.store(false, Ordering::SeqCst);
    revoke_tls_identity(&store, &keys).unwrap();
    assert!(matches!(
        keys.read(&reference),
        Err(KeyStorageError::Missing)
    ));
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    rotate_tls_identity(&store, &keys).unwrap();
    assert!(load_or_create_tls_identity(&store, &keys).is_ok());
}

#[test]
fn concurrent_initializers_converge_and_discard_unpublished_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    // The migration itself is separately transactional; this forces publication races.
    drop(WorkerStore::open(&path).unwrap());
    let keys = Arc::new(MemoryKeys::default());
    let barrier = Arc::new(std::sync::Barrier::new(6));
    let workers: Vec<_> = (0..6)
        .map(|_| {
            let path = path.clone();
            let keys = keys.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = WorkerStore::open(path).unwrap();
                barrier.wait();
                load_or_create_tls_identity(&store, keys.as_ref())
                    .unwrap()
                    .certificate
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(results.iter().all(|r| r == &results[0]));
    assert_eq!(keys.entries.lock().unwrap().len(), 1);
}

#[test]
fn publication_failure_never_writes_plaintext_or_loses_a_published_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    let store = WorkerStore::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER deny_tls BEFORE INSERT ON worker_tls BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    let keys = MemoryKeys::default();
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    assert!(store.tls_identity().unwrap().is_none());
    // Ambiguous publish errors preserve the vault entry for recovery, not destructive cleanup.
    for key in keys.entries.lock().unwrap().values() {
        assert_no_key_bytes(dir.path(), key);
    }
}

#[test]
fn blocked_legacy_checkpoint_is_retryable_and_never_reports_ready() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.db");
    let legacy = legacy_database(&path);
    let store = WorkerStore::open(&path).unwrap();
    let reader = rusqlite::Connection::open(&path).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM worker_tls;")
        .unwrap();
    let keys = MemoryKeys::default();
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    let row = store.tls_identity().unwrap().unwrap();
    assert!(matches!(row.key, WorkerTlsKey::Stored(_)));
    assert!(row.scrub_pending);
    reader.execute_batch("ROLLBACK").unwrap();
    drop(reader);
    assert_eq!(
        load_or_create_tls_identity(&store, &keys)
            .unwrap()
            .certificate,
        legacy.certificate
    );
    assert_no_key_bytes(dir.path(), legacy.private_key.as_ref());
    assert_eq!(keys.entries.lock().unwrap().len(), 1);
}

#[test]
fn incorrect_vault_readback_prevents_reference_publication() {
    let dir = tempfile::tempdir().unwrap();
    let store = WorkerStore::open(dir.path().join("worker.db")).unwrap();
    let keys = MemoryKeys::default();
    keys.corrupt_create.store(true, Ordering::SeqCst);
    assert!(load_or_create_tls_identity(&store, &keys).is_err());
    assert!(store.tls_identity().unwrap().is_none());
}
