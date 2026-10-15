use tetonic_domain::DataClass;
use tetonic_transaction::{TransactionError, WorkspaceTransactionService, WorkspaceTxnConfig};

#[test]
fn a_new_file_can_be_repaired_before_it_is_published() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("result.txt", "broken\n", false)
        .unwrap();
    let (before, after) = txn
        .stage_edit_file("./result.txt", "broken", "ready")
        .unwrap();
    assert_eq!(before, "broken\n");
    assert_eq!(after, "ready\n");
    assert_eq!(txn.preview().operations.len(), 1);
    assert!(!dir.path().join("result.txt").exists());
    txn.commit("test", DataClass::RepositorySource).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "ready\n"
    );
}

#[test]
fn successive_edits_compose_in_the_pending_view() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "alpha\nbeta\n").unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_edit_file("result.txt", "alpha", "first").unwrap();
    let (before, after) = txn
        .stage_edit_file("./result.txt", "beta", "second")
        .unwrap();
    assert_eq!(before, "first\nbeta\n");
    assert_eq!(after, "first\nsecond\n");
    assert_eq!(txn.preview().operations.len(), 1);
    txn.commit("test", DataClass::RepositorySource).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        after
    );
}

#[test]
fn repairing_or_rewriting_preserves_the_first_conflict_baseline() {
    for edit in [true, false] {
        for existed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("result.txt");
            if existed {
                std::fs::write(&path, "original").unwrap();
            }
            let service =
                WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default())
                    .unwrap();
            let mut txn = service.begin().unwrap();
            txn.stage_write_file("result.txt", "pending", existed)
                .unwrap();
            let original = txn.preview().operations[0].clone();
            std::fs::write(&path, "another agent's work").unwrap();
            if edit {
                txn.stage_edit_file("./result.txt", "pending", "repaired")
                    .unwrap();
            } else {
                txn.stage_write_file("./result.txt", "repaired", true)
                    .unwrap();
            }
            let preview = txn.preview();
            assert_eq!(preview.operations.len(), 1);
            assert_eq!(preview.operations[0].base_digest, original.base_digest);
            assert_eq!(preview.operations[0].kind, original.kind);
            assert!(matches!(
                txn.commit("test", DataClass::RepositorySource),
                Err(TransactionError::Conflict(_))
            ));
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "another agent's work"
            );
        }
    }
}

#[test]
fn staged_files_with_similar_names_do_not_share_content() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("src/a.txt", "nested", false).unwrap();
    txn.stage_write_file("src_a.txt", "flat", false).unwrap();
    assert_eq!(
        txn.read_staged_file("src/a.txt").unwrap().as_deref(),
        Some("nested")
    );
    assert_eq!(
        txn.read_staged_file("src_a.txt").unwrap().as_deref(),
        Some("flat")
    );
    txn.commit("test", DataClass::RepositorySource).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a.txt")).unwrap(),
        "nested"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src_a.txt")).unwrap(),
        "flat"
    );
}

#[test]
fn edits_cannot_exceed_transaction_limits_or_replace_pending_bytes_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "original").unwrap();
    let service = WorkspaceTransactionService::new(
        dir.path(),
        WorkspaceTxnConfig {
            limits: tetonic_transaction::security::SecurityLimits {
                max_file_bytes: 12,
                max_total_bytes: 18,
            },
            ..WorkspaceTxnConfig::default()
        },
    )
    .unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("result.txt", "pending", true).unwrap();
    for replacement in ["too large for this file", "twelve bytes"] {
        assert!(matches!(
            txn.stage_edit_file("result.txt", "pending", replacement),
            Err(TransactionError::SizeLimit(_))
        ));
        assert_eq!(
            txn.read_staged_file("result.txt").unwrap().as_deref(),
            Some("pending")
        );
    }
    txn.stage_edit_file("result.txt", "pending", "ready")
        .unwrap();
    txn.commit("test", DataClass::RepositorySource).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "ready"
    );
}

#[test]
fn editing_tampered_staged_content_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "original").unwrap();
    let service =
        WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("result.txt", "pending", true).unwrap();
    let preview = txn.preview();
    std::fs::write(
        preview.operations[0].new_content_path.as_ref().unwrap(),
        "unexpected",
    )
    .unwrap();
    let error = txn
        .stage_edit_file("result.txt", "original", "repaired")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("staged content changed unexpectedly"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "original"
    );
}
