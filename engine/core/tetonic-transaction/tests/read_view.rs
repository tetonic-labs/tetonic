use tetonic_domain::DataClass;
use tetonic_transaction::{WorkspaceTransactionService, WorkspaceTxnConfig};

#[test]
fn pending_reads_do_not_rebase_a_conflicting_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.md");
    std::fs::write(&path, "original").unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("result.md", "pending", true).unwrap();
    std::fs::write(&path, "another agent's committed work").unwrap();
    assert_eq!(
        txn.read_staged_file("./result.md").unwrap().as_deref(),
        Some("pending")
    );
    txn.record_read("result.md").unwrap();
    assert!(matches!(
        txn.commit("test", DataClass::RepositorySource),
        Err(tetonic_transaction::TransactionError::Conflict(_))
    ));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "another agent's committed work"
    );
}

#[test]
fn pending_deletion_and_rename_do_not_expose_the_old_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("source.md"), "source").unwrap();
    std::fs::write(dir.path().join("deleted.md"), "deleted").unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_rename("source.md", "destination.md").unwrap();
    txn.stage_delete("deleted.md").unwrap();
    assert!(txn.read_staged_file("source.md").is_err());
    assert!(txn.read_staged_file("deleted.md").is_err());
    assert_eq!(
        txn.read_staged_file("destination.md").unwrap().as_deref(),
        Some("source")
    );
    assert!(txn.read_staged_file("../outside.md").is_err());
    assert!(txn.read_staged_file(".lokai/staging/private").is_err());
    assert!(dir.path().join("source.md").exists());
    assert!(dir.path().join("deleted.md").exists());
    assert!(!dir.path().join("destination.md").exists());
}

#[test]
fn tampered_staged_content_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("result.md", "expected", false)
        .unwrap();
    let preview = txn.preview();
    let staged = preview.operations[0].new_content_path.as_ref().unwrap();
    std::fs::write(staged, "unexpected").unwrap();
    assert!(txn.read_staged_file("result.md").is_err());
}
