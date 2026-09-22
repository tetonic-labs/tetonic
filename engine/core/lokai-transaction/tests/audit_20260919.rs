//! Regression coverage for transaction ownership and crash recovery.
//! All filesystem effects stay inside owned temp fixtures.
use lokai_domain::TransactionState;
use lokai_transaction::{
    apply_journal_operation, build_journal, recover_at_startup, CommitJournal, TransactionMeta,
    WorkspaceTransactionService, WorkspaceTxnConfig,
};

#[cfg(not(unix))]
#[test]
fn unsupported_mode_change_does_not_advance_durable_journal() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    std::fs::write(root.join("file.txt"), "unchanged").unwrap();
    let service = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_mode_change("file.txt", 0o755).unwrap();
    let lokai = root.join(".lokai");
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(
        &mut journal,
        root,
        &lokai.join("backups").join(&txn.id.0),
        &txn.rw_sets(),
    )
    .unwrap();
    let error = apply_journal_operation(root, &mut journal, 0, &lokai).unwrap_err();
    assert!(error.to_string().contains("unsupported"));
    let durable = CommitJournal::load(&CommitJournal::path(&lokai, &txn.id)).unwrap();
    for state in [&journal, &durable] {
        assert_eq!(state.state, TransactionState::Committing);
        assert_eq!(state.progress, 0);
        assert_eq!(state.operations[0].started, Some(true));
        assert!(!state.operations[0].completed);
    }
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "unchanged"
    );
}

#[test]
fn recovery_ignores_untrusted_staging_path() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("workspace");
    let victim = fixture.path().join("unrelated-owned-fixture");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(
        victim.join("sentinel.txt"),
        "must survive workspace startup",
    )
    .unwrap();
    let service = WorkspaceTransactionService::new(&root, WorkspaceTxnConfig::default()).unwrap();
    let txn = service.begin().unwrap();
    let lokai = root.join(".lokai");
    let mut meta = TransactionMeta::load(&TransactionMeta::path(&lokai, &txn.id)).unwrap();
    meta.staging_dir = victim.display().to_string();
    meta.persist(&lokai).unwrap();
    drop(txn);
    let _second = WorkspaceTransactionService::new(&root, WorkspaceTxnConfig::default()).unwrap();
    assert!(
        victim.exists(),
        "untrusted metadata must not control deletion"
    );
}

#[test]
fn missing_rollback_backup_keeps_operation_incomplete_and_retryable() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    std::fs::write(root.join("file.txt"), "before").unwrap();
    let service = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("file.txt", "after", true).unwrap();
    let lokai = root.join(".lokai");
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(
        &mut journal,
        root,
        &lokai.join("backups").join(&txn.id.0),
        &txn.rw_sets(),
    )
    .unwrap();
    apply_journal_operation(root, &mut journal, 0, &lokai).unwrap();
    let backup = std::path::PathBuf::from(journal.operations[0].backup_path.as_ref().unwrap());
    let retained = backup.with_extension("retained");
    std::fs::rename(&backup, &retained).unwrap();
    assert!(lokai_transaction::commit::rollback_journal(root, &mut journal, &lokai).is_err());
    assert!(!journal.operations[0].rolled_back);
    let durable = CommitJournal::load(&CommitJournal::path(&lokai, &txn.id)).unwrap();
    assert!(!durable.operations[0].rolled_back);
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "after"
    );
    std::fs::rename(&retained, &backup).unwrap();
    lokai_transaction::commit::rollback_journal(root, &mut journal, &lokai).unwrap();
    assert!(journal.operations[0].rolled_back);
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "before"
    );
}

#[test]
fn opening_second_service_preserves_live_transaction() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let service = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("new.txt", "staged", false).unwrap();
    let meta_path = TransactionMeta::path(&root.join(".lokai"), &txn.id);
    let before = TransactionMeta::load(&meta_path).unwrap();
    assert_eq!(before.pid, std::process::id());
    assert!(std::path::Path::new(&before.staging_dir).exists());
    let _second = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap();
    assert!(std::path::Path::new(&before.staging_dir).exists());
    assert_eq!(
        TransactionMeta::load(&meta_path).unwrap().state,
        TransactionState::Staged
    );
    assert_eq!(txn.state, TransactionState::Staged);
}

#[test]
fn unpersisted_completion_is_reconciled_and_rolled_back() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    std::fs::write(root.join("file.txt"), "before").unwrap();
    let service = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap();
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("file.txt", "after", true).unwrap();
    let lokai = root.join(".lokai");
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(
        &mut journal,
        root,
        &lokai.join("backups").join(&txn.id.0),
        &txn.rw_sets(),
    )
    .unwrap();
    journal.operations[0].started = Some(true);
    journal.persist(&lokai).unwrap();
    let pre_apply_journal = std::fs::read(CommitJournal::path(&lokai, &txn.id)).unwrap();
    apply_journal_operation(root, &mut journal, 0, &lokai).unwrap();
    // Restore precisely the durable journal that exists if the process dies
    // between apply_one() and mark_completed()/persist(). No production hook.
    std::fs::write(CommitJournal::path(&lokai, &txn.id), pre_apply_journal).unwrap();
    let id = txn.id.clone();
    drop(txn);
    let report = recover_at_startup(root).unwrap();
    assert!(report.recovered.iter().any(|r| r.contains("rolled back")));
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "before"
    );
    assert_eq!(
        CommitJournal::load(&CommitJournal::path(&lokai, &id))
            .unwrap()
            .state,
        TransactionState::Aborted
    );
    // Older journals do not prove whether an unrecorded mutation occurred.
    journal.state = TransactionState::Committing;
    journal.operations[0].started = None;
    journal.operations[0].completed = false;
    journal.operations[0].rolled_back = false;
    std::fs::write(root.join("file.txt"), "ambiguous").unwrap();
    journal.persist(&lokai).unwrap();
    let report = recover_at_startup(root).unwrap();
    assert_eq!(report.requires_manual, vec![id.0.clone()]);
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "ambiguous"
    );
    assert!(lokai_transaction::recovery::resume_recovery(root, &id.0, false).is_err());
}
