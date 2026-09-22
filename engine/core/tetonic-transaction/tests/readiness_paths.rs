//! Readiness V1-05: protected names must survive filesystem aliasing.
use tetonic_transaction::security::validate_path;
use tetonic_transaction::{WorkspaceTransactionService, WorkspaceTxnConfig};

#[test]
fn protected_names_are_components_not_prefixes() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        ".LOKAI/authority.json",
        "nested/.LoKaI",
        "nested/.SSH/key",
        "nested/.AWS/config",
        "nested/LOKAI.DB",
        "nested/lokai.db-wal",
        ".lokai./authority.json",
        "nested/.lokai /authority.json",
        "nested\\.LOKAI\\authority.json",
        "../escape",
    ] {
        assert!(validate_path(root.path(), path).is_err(), "accepted {path}");
    }
    for path in [
        ".lokai-example/readme.md",
        ".ssh-notes",
        "nested/lokai.db.txt",
        "notes..txt",
        "src/file.rs",
    ] {
        assert!(validate_path(root.path(), path).is_ok(), "rejected {path}");
    }
}

#[test]
fn mutations_reject_internal_directory_aliases() {
    let root = tempfile::tempdir().unwrap();
    let service =
        WorkspaceTransactionService::new(root.path(), WorkspaceTxnConfig::default()).unwrap();
    let sentinel = root.path().join(".lokai/authority.json");
    std::fs::write(&sentinel, "original").unwrap();
    std::fs::write(root.path().join("ordinary.txt"), "original").unwrap();
    let mut txn = service.begin().unwrap();
    let alias = ".LOKAI/authority.json";
    assert!(txn.stage_write_file(alias, "replacement", true).is_err());
    assert!(txn
        .stage_edit_file(alias, "original", "replacement")
        .is_err());
    assert!(txn.stage_delete(alias).is_err());
    assert!(txn.stage_rename(alias, "moved.txt").is_err());
    assert!(txn.stage_rename("ordinary.txt", alias).is_err());
    assert!(txn.stage_mode_change(alias, 0o600).is_err());
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "original");
    assert!(txn.rw_sets().writes.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_streams_and_ambiguous_components_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "ordinary.txt:stream",
        ".lokai::$INDEX_ALLOCATION/authority.json",
        "folder./file",
        "folder /file",
    ] {
        assert!(validate_path(root.path(), path).is_err(), "accepted {path}");
    }
}
