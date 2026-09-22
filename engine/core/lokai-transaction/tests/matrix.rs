//! M2-4 required test matrix — workspace-versioned transactions.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use lokai_domain::{DataClass, PatchApproval, TransactionState, WorkspaceVersionScheme};
use lokai_transaction::{
    apply_journal, apply_journal_operation, build_journal, detect_unexpected_mutations,
    recover_at_startup, rollback_journal, snapshot_read_digests, CommitJournal, SecurityLimits,
    TransactionError, WorkspaceTransactionService, WorkspaceTxnConfig, WriterLock,
};
use tempfile::TempDir;

fn tmp_ws(tag: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::with_prefix(format!("lokai-m24-{tag}-")).unwrap();
    let root = dir.path().to_path_buf();
    (dir, root)
}

fn svc(root: &Path) -> WorkspaceTransactionService {
    WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default()).unwrap()
}

fn git_ok() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_init(root: &Path) {
    assert!(git_ok(), "git required for git matrix tests");
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["init", "-q"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "user.email", "test@lokai.local"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "user.name", "Lokai Test"])
        .status()
        .unwrap();
}

fn git_commit_all(root: &Path, msg: &str) {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "-A"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-q", "-m", msg])
        .status()
        .unwrap();
}

fn commit_txn(
    svc: &WorkspaceTransactionService,
    stage: impl FnOnce(&mut lokai_transaction::service::WorkspaceTransaction),
) -> lokai_domain::CommitResult {
    let mut txn = svc.begin().unwrap();
    stage(&mut txn);
    txn.commit("test", DataClass::RepositorySource).unwrap()
}

#[test]
fn clean_git_repository() {
    if !git_ok() {
        return;
    }
    let (_d, root) = tmp_ws("clean-git");
    git_init(&root);
    std::fs::write(root.join("a.txt"), "one").unwrap();
    git_commit_all(&root, "init");
    let service = svc(&root);
    let v = service.begin().unwrap().base_version;
    assert_eq!(v.version_scheme, WorkspaceVersionScheme::Git);
    assert!(v.git_head.is_some());
    let result = commit_txn(&service, |txn| {
        txn.stage_write_file("b.txt", "two", false).unwrap();
    });
    assert!(root.join("b.txt").exists());
    assert_ne!(
        result.base_version.dirty_state_digest,
        result.result_version.dirty_state_digest
    );
}

#[test]
fn dirty_git_repository() {
    if !git_ok() {
        return;
    }
    let (_d, root) = tmp_ws("dirty-git");
    git_init(&root);
    std::fs::write(root.join("tracked.txt"), "v1").unwrap();
    git_commit_all(&root, "init");
    std::fs::write(root.join("tracked.txt"), "dirty").unwrap();
    let service = svc(&root);
    let v = service.begin().unwrap().base_version;
    assert_ne!(v.dirty_state_digest, v.tracked_state_digest);
    commit_txn(&service, |txn| {
        txn.stage_write_file("new.txt", "x", false).unwrap();
    });
    assert_eq!(std::fs::read_to_string(root.join("new.txt")).unwrap(), "x");
}

#[test]
fn untracked_file_conflict() {
    let (_d, root) = tmp_ws("untracked-conflict");
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("u.txt", "agent", false).unwrap();
    std::fs::write(root.join("u.txt"), "user").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
    assert_eq!(std::fs::read_to_string(root.join("u.txt")).unwrap(), "user");
}

#[test]
fn head_changes_during_staging() {
    if !git_ok() {
        return;
    }
    let (_d, root) = tmp_ws("head-change");
    git_init(&root);
    std::fs::write(root.join("f.txt"), "a").unwrap();
    git_commit_all(&root, "c1");
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("g.txt", "b", false).unwrap();
    std::fs::write(root.join("h.txt"), "c").unwrap();
    git_commit_all(&root, "c2");
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
}

#[test]
fn user_edits_touched_file_before_commit() {
    let (_d, root) = tmp_ws("touched-edit");
    std::fs::write(root.join("t.txt"), "original").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("t.txt", "agent", true).unwrap();
    std::fs::write(root.join("t.txt"), "user").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
    assert_eq!(std::fs::read_to_string(root.join("t.txt")).unwrap(), "user");
}

#[test]
fn user_edits_unrelated_file_before_commit() {
    let (_d, root) = tmp_ws("unrelated-edit");
    std::fs::write(root.join("a.txt"), "a").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("b.txt", "b", false).unwrap();
    std::fs::write(root.join("a.txt"), "changed").unwrap();
    let result = txn.commit("test", DataClass::RepositorySource).unwrap();
    assert!(root.join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "changed"
    );
    assert!(result.artifact.commit_succeeded);
}

#[test]
fn new_file_destination_appears_before_commit() {
    let (_d, root) = tmp_ws("new-dest");
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("n.txt", "agent", false).unwrap();
    std::fs::write(root.join("n.txt"), "user beat us").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
}

#[test]
fn delete_target_changes() {
    let (_d, root) = tmp_ws("delete-change");
    std::fs::write(root.join("d.txt"), "keep").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_delete("d.txt").unwrap();
    std::fs::write(root.join("d.txt"), "mutated").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
    assert!(root.join("d.txt").exists());
}

#[test]
fn rename_destination_appears() {
    let (_d, root) = tmp_ws("rename-dest");
    std::fs::write(root.join("from.txt"), "data").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_rename("from.txt", "to.txt").unwrap();
    std::fs::write(root.join("to.txt"), "blocker").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
    assert!(root.join("from.txt").exists());
}

#[test]
fn symlink_target_changes() {
    let (_d, root) = tmp_ws("symlink");
    std::fs::write(root.join("real.txt"), "secret").unwrap();
    std::fs::write(root.join("outside.txt"), "outside").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("real.txt", root.join("link.txt")).unwrap();
        let service = svc(&root);
        let mut txn = service.begin().unwrap();
        txn.stage_write_file("link.txt", "via link", true).unwrap();
        std::fs::remove_file(root.join("link.txt")).unwrap();
        symlink("../outside.txt", root.join("link.txt")).unwrap();
        let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
        assert!(matches!(err, TransactionError::Conflict(_)));
    }
}

#[test]
fn multi_file_commit_fails_after_first_file() {
    let (_d, root) = tmp_ws("multi-fail");
    std::fs::write(root.join("one.txt"), "1").unwrap();
    std::fs::write(root.join("two.txt"), "2").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("one.txt", "one-new", true).unwrap();
    txn.stage_write_file("two.txt", "two-new", true).unwrap();
    let lokai = root.join(".lokai");
    let backup = lokai.join("backups").join(&txn.id.0);
    let sets = txn.rw_sets();
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(&mut journal, &root, &backup, &sets).unwrap();
    apply_journal_operation(&root, &mut journal, 0, &lokai).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("one.txt")).unwrap(),
        "one-new"
    );
    assert_eq!(std::fs::read_to_string(root.join("two.txt")).unwrap(), "2");
    std::fs::write(root.join("two.txt"), "user-changed").unwrap();
    let err = apply_journal_operation(&root, &mut journal, 1, &lokai).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
    rollback_journal(&root, &mut journal, &lokai).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("one.txt")).unwrap(), "1");
    assert_eq!(
        std::fs::read_to_string(root.join("two.txt")).unwrap(),
        "user-changed"
    );
}

#[test]
fn crash_recovery_after_journal_steps() {
    let (_d, root) = tmp_ws("crash-steps");
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::write(root.join("b.txt"), "b").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("a.txt", "A", true).unwrap();
    txn.stage_write_file("b.txt", "B", true).unwrap();
    let lokai = root.join(".lokai");
    let backup = lokai.join("backups").join(&txn.id.0);
    let sets = txn.rw_sets();
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(&mut journal, &root, &backup, &sets).unwrap();

    drop(txn); // Simulate process death: release the OS lease.
    for step in 0..journal.operations.len() {
        std::fs::write(root.join("a.txt"), "a").unwrap();
        std::fs::write(root.join("b.txt"), "b").unwrap();
        if lokai.join("journals").exists() {
            std::fs::remove_dir_all(lokai.join("journals")).ok();
        }
        std::fs::create_dir_all(lokai.join("journals")).unwrap();

        let mut partial = journal.clone();
        for i in 0..=step {
            apply_journal_operation(&root, &mut partial, i as u32, &lokai).unwrap();
        }
        partial.state = TransactionState::Committing;
        partial.persist(&lokai).unwrap();

        let report = recover_at_startup(&root).unwrap();
        assert!(
            !report.recovered.is_empty() || !report.requires_manual.is_empty(),
            "step {step} should produce recovery activity"
        );
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "b");
        assert!(
            svc(&root).begin().is_ok(),
            "step {step} should clear recovery block"
        );
    }
}

#[test]
fn concurrent_commits_blocked() {
    let (_d, root) = tmp_ws("concurrent");
    let lokai = root.join(".lokai");
    std::fs::create_dir_all(&lokai).unwrap();
    let id1 = lokai_domain::TransactionId::new("txn_a");
    let id2 = lokai_domain::TransactionId::new("txn_b");
    let _lock = WriterLock::acquire(&lokai, &id1, "proc:a").unwrap();
    let second = WriterLock::acquire(&lokai, &id2, "proc:b");
    assert!(matches!(second, Err(TransactionError::LockContention(_))));
}

#[test]
fn concurrent_commits_cross_process() {
    if std::env::var("LOKAI_CROSS_LOCK_DIR").is_ok() {
        let dir = std::env::var("LOKAI_CROSS_LOCK_DIR").unwrap();
        let lokai = PathBuf::from(dir).join(".lokai");
        let id = lokai_domain::TransactionId::new("child_txn");
        let _lock = WriterLock::acquire(&lokai, &id, "child-proc").unwrap();
        thread::sleep(Duration::from_millis(1500));
        return;
    }

    let (_d, root) = tmp_ws("cross-proc");
    let lokai = root.join(".lokai");
    std::fs::create_dir_all(&lokai).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(exe)
        .env("LOKAI_CROSS_LOCK_DIR", root.display().to_string())
        .arg("concurrent_commits_cross_process")
        .arg("--exact")
        .arg("concurrent_commits_cross_process")
        .spawn()
        .expect("spawn cross-process lock child");
    thread::sleep(Duration::from_millis(200));
    let id = lokai_domain::TransactionId::new("parent_txn");
    let second = WriterLock::acquire(&lokai, &id, "parent-proc");
    assert!(matches!(second, Err(TransactionError::LockContention(_))));
    child.wait().unwrap();
}

#[test]
fn recovery_required_blocks_new_transactions() {
    let (_d, root) = tmp_ws("recovery-block");
    let lokai = root.join(".lokai");
    std::fs::create_dir_all(lokai.join("journals")).unwrap();
    let txn_id = lokai_domain::TransactionId::new("blocked_txn");
    let journal = CommitJournal {
        transaction_id: txn_id.clone(),
        base_version: lokai_domain::WorkspaceVersion {
            repository_id: lokai_domain::RepositoryId::new("manifest:test"),
            version_scheme: WorkspaceVersionScheme::Manifest,
            git_head: None,
            dirty_state_digest: lokai_transaction::digest_string("dirty"),
            tracked_state_digest: lokai_transaction::digest_string("tracked"),
            relevant_path_digests: Default::default(),
            index_generation: None,
        },
        patch_digest: lokai_transaction::digest_string("patch"),
        state: TransactionState::RecoveryRequired,
        operations: vec![],
        progress: 0,
        recovery_instructions: "manual".into(),
    };
    journal.persist(&lokai).unwrap();
    assert!(matches!(
        WorkspaceTransactionService::new(&root, WorkspaceTxnConfig::default()),
        Err(TransactionError::RecoveryRequired(_))
    ));
    std::fs::remove_file(lokai.join("journals").join(format!("{}.json", txn_id.0))).unwrap();
    assert!(WorkspaceTransactionService::new(&root, WorkspaceTxnConfig::default()).is_ok());
}

#[test]
fn read_set_influenced_file_conflict() {
    let (_d, root) = tmp_ws("read-conflict");
    std::fs::write(root.join("input.txt"), "input").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.record_read("input.txt").unwrap();
    txn.stage_write_file("output.txt", "out", false).unwrap();
    std::fs::write(root.join("input.txt"), "changed").unwrap();
    let err = txn.commit("test", DataClass::RepositorySource).unwrap_err();
    assert!(matches!(err, TransactionError::Conflict(_)));
}

#[test]
fn concurrent_commit_threads() {
    let (_d, root) = tmp_ws("threads");
    let service = Arc::new(svc(&root));
    let svc1 = service.clone();
    let svc2 = service.clone();
    let t1 = thread::spawn(move || {
        let mut txn = svc1.begin().unwrap();
        txn.stage_write_file("a.txt", "from1", false).unwrap();
        txn.commit("t1", DataClass::RepositorySource)
    });
    let t2 = thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(20));
        let mut txn = svc2.begin().unwrap();
        txn.stage_write_file("b.txt", "from2", false).unwrap();
        txn.commit("t2", DataClass::RepositorySource)
    });
    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    let ok_count = [r1.is_ok(), r2.is_ok()].into_iter().filter(|x| *x).count();
    assert_eq!(ok_count, 1, "exactly one concurrent commit should succeed");
}

#[test]
fn non_git_workspace() {
    let (_d, root) = tmp_ws("nongit");
    std::fs::write(root.join("m.txt"), "m").unwrap();
    let service = svc(&root);
    let v = service.begin().unwrap().base_version;
    assert_eq!(v.version_scheme, WorkspaceVersionScheme::Manifest);
    assert!(v.git_head.is_none());
    commit_txn(&service, |txn| {
        txn.stage_write_file("n.txt", "n", false).unwrap();
    });
    assert!(root.join("n.txt").exists());
}

#[test]
fn non_git_version_is_stable_across_seconds() {
    use lokai_transaction::version::capture_workspace_version;

    let (_d, root) = tmp_ws("nongit-stable");
    std::fs::write(root.join("m.txt"), "m").unwrap();
    let first = capture_workspace_version(&root, &[]).unwrap();
    thread::sleep(Duration::from_millis(1_100));
    let second = capture_workspace_version(&root, &[]).unwrap();
    assert_eq!(
        first, second,
        "unchanged non-git workspace must capture an identical version; capability binding \
         compares whole versions, so any clock-derived field denies later mutations"
    );

    std::fs::write(root.join("m.txt"), "changed").unwrap();
    let third = capture_workspace_version(&root, &[]).unwrap();
    assert_ne!(
        second, third,
        "content change must still produce a distinct version"
    );
    assert_ne!(second.index_generation, third.index_generation);
}

#[test]
fn binary_file() {
    let (_d, root) = tmp_ws("binary");
    let bytes: Vec<u8> = (0..=255).collect();
    let service = svc(&root);
    commit_txn(&service, |txn| {
        txn.stage_write_bytes("bin.dat", &bytes, false).unwrap();
    });
    assert_eq!(std::fs::read(root.join("bin.dat")).unwrap(), bytes);
}

#[test]
fn line_ending_preservation() {
    let (_d, root) = tmp_ws("crlf");
    std::fs::write(root.join("f.txt"), "a\r\nb\r\n").unwrap();
    let service = svc(&root);
    commit_txn(&service, |txn| {
        txn.stage_edit_file("f.txt", "a\r\n", "A\r\n").unwrap();
    });
    assert_eq!(std::fs::read(root.join("f.txt")).unwrap(), b"A\r\nb\r\n");
}

#[test]
fn permission_bit_change_unix() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let (_d, root) = tmp_ws("mode");
        std::fs::write(root.join("p.txt"), "p").unwrap();
        std::fs::set_permissions(root.join("p.txt"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        let service = svc(&root);
        let mut txn = service.begin().unwrap();
        txn.stage_mode_change("p.txt", 0o755).unwrap();
        txn.commit("test", DataClass::RepositorySource).unwrap();
        let mode = std::fs::metadata(root.join("p.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn large_file_and_transaction_size_limit() {
    let (_d, root) = tmp_ws("size-limit");
    let service = WorkspaceTransactionService::new(
        &root,
        WorkspaceTxnConfig {
            limits: SecurityLimits {
                max_file_bytes: 1024,
                max_total_bytes: 2048,
            },
            ..Default::default()
        },
    )
    .unwrap();
    let big = "x".repeat(2048);
    let mut txn = service.begin().unwrap();
    let err = txn.stage_write_file("big.txt", &big, false).unwrap_err();
    assert!(matches!(err, TransactionError::SizeLimit(_)));
}

#[test]
fn verification_unexpected_source_mutation() {
    let (_d, root) = tmp_ws("verify-unexpected");
    std::fs::write(root.join("src.txt"), "src").unwrap();
    std::fs::write(root.join("other.txt"), "other").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.record_read("src.txt").unwrap();
    txn.record_read("other.txt").unwrap();
    txn.stage_write_file("out.txt", "out", false).unwrap();
    let snap = snapshot_read_digests(
        &root,
        &[
            lokai_domain::WorkspacePath::new("src.txt"),
            lokai_domain::WorkspacePath::new("other.txt"),
        ],
    )
    .unwrap();
    std::fs::write(root.join("other.txt"), "mutated during verify").unwrap();
    let unexpected =
        detect_unexpected_mutations(&root, txn.staging_area(), &txn.rw_sets(), &snap).unwrap();
    assert!(!unexpected.is_empty());
}

#[test]
fn outside_workspace_rejected() {
    let (_d, root) = tmp_ws("outside");
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    let err = txn
        .stage_write_file("../escape.txt", "bad", false)
        .unwrap_err();
    assert!(matches!(err, TransactionError::OutsideWorkspace(_)));
}

#[test]
fn build_artifacts_excluded() {
    let (_d, root) = tmp_ws("artifacts");
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    let err = txn
        .stage_write_file("target/debug/out.o", "obj", false)
        .unwrap_err();
    assert!(err.to_string().contains("artifact") || err.to_string().contains("refusing"));
}

#[test]
fn approval_invalidated_on_patch_change() {
    let (_d, root) = tmp_ws("approval");
    std::fs::write(root.join("a.txt"), "a").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("a.txt", "b", true).unwrap();
    let preview = txn.preview();
    let approval = PatchApproval {
        transaction_id: txn.id.clone(),
        patch_digest: preview.patch_digest.clone(),
        base_version: txn.base_version.clone(),
        verification_required: false,
        verification_passed: true,
    };
    txn.bind_approval(approval.clone()).unwrap();
    txn.stage_write_file("a.txt", "c", true).unwrap();
    let stale = PatchApproval {
        patch_digest: preview.patch_digest,
        ..approval
    };
    assert!(txn.bind_approval(stale).is_err());
}

#[test]
fn rollback_leaves_workspace_recoverable() {
    let (_d, root) = tmp_ws("rollback");
    std::fs::write(root.join("r.txt"), "before").unwrap();
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("r.txt", "after", true).unwrap();
    let lokai = root.join(".lokai");
    let backup = lokai.join("backups").join(&txn.id.0);
    let sets = txn.rw_sets();
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: txn.preview().patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: String::new(),
    };
    build_journal(&mut journal, &root, &backup, &sets).unwrap();
    journal.persist(&lokai).unwrap();
    std::fs::write(root.join("r.txt"), "conflict").unwrap();
    let err = apply_journal(&root, &mut journal, &lokai);
    assert!(err.is_err());
    rollback_journal(&root, &mut journal, &lokai).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("r.txt")).unwrap(),
        "conflict"
    );
}

#[test]
fn committed_transaction_records_artifact() {
    let (_d, root) = tmp_ws("artifact");
    let service = svc(&root);
    let result = commit_txn(&service, |txn| {
        txn.stage_write_file("art.txt", "art", false).unwrap();
    });
    let artifact_path = root
        .join(".lokai/artifacts")
        .join(format!("{}.json", result.transaction_id.0));
    assert!(artifact_path.exists());
    let text = std::fs::read_to_string(artifact_path).unwrap();
    assert!(text.contains("result_workspace_version"));
    assert!(text.contains("patch_artifact_id"));
}

#[test]
fn kill_after_stage_before_commit_recovers() {
    let (_d, root) = tmp_ws("stage-recover");
    std::fs::write(root.join("live.txt"), "original").unwrap();
    {
        let service = svc(&root);
        service
            .with_active(|txn| {
                txn.stage_write_file("live.txt", "staged-never-committed", true)?;
                txn.stage_write_file("ghost.txt", "should-not-appear", false)?;
                Ok(())
            })
            .unwrap();
        // Drop without commit/abort — simulates process kill mid-stage (R6-3).
    }
    assert_eq!(
        std::fs::read_to_string(root.join("live.txt")).unwrap(),
        "original"
    );
    assert!(!root.join("ghost.txt").exists());

    let report = recover_at_startup(&root).unwrap();
    assert!(
        !report.aborted_staged.is_empty(),
        "startup must discard incomplete stages: {report:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("live.txt")).unwrap(),
        "original"
    );
    assert!(!root.join("ghost.txt").exists());

    let staging = root.join(".lokai/staging");
    if staging.exists() {
        assert!(
            std::fs::read_dir(&staging).unwrap().next().is_none(),
            "orphaned staging dirs must be removed"
        );
    }
    let service = svc(&root);
    assert!(service.begin().is_ok());
}

#[test]
fn abort_active_discards_staging() {
    let (_d, root) = tmp_ws("abort-active");
    std::fs::write(root.join("a.txt"), "a").unwrap();
    let service = svc(&root);
    service
        .with_active(|txn| {
            txn.stage_write_file("a.txt", "b", true)?;
            Ok(())
        })
        .unwrap();
    assert!(service.abort_active_if_any().unwrap());
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a");
    assert!(!service.abort_active_if_any().unwrap());
}

#[test]
fn cross_process_commit_errors_clearly() {
    if std::env::var("LOKAI_CROSS_COMMIT_DIR").is_ok() {
        let dir = std::env::var("LOKAI_CROSS_COMMIT_DIR").unwrap();
        let lokai = PathBuf::from(dir).join(".lokai");
        let id = lokai_domain::TransactionId::new("child_hold");
        let _lock = WriterLock::acquire(&lokai, &id, "child-proc").unwrap();
        thread::sleep(Duration::from_millis(1500));
        return;
    }

    let (_d, root) = tmp_ws("cross-commit");
    std::fs::write(root.join("c.txt"), "base").unwrap();
    // Construct service + stage before the child holds the writer lock.
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("c.txt", "conflict", true).unwrap();

    let lokai = root.join(".lokai");
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(exe)
        .env("LOKAI_CROSS_COMMIT_DIR", root.display().to_string())
        .arg("cross_process_commit_errors_clearly")
        .arg("--exact")
        .arg("cross_process_commit_errors_clearly")
        .spawn()
        .expect("spawn cross-process commit child");
    thread::sleep(Duration::from_millis(200));

    let err = txn
        .commit("parent-proc", DataClass::RepositorySource)
        .unwrap_err();
    assert!(
        matches!(err, TransactionError::LockContention(_)),
        "second writer must fail with LockContention, got: {err}"
    );
    assert_eq!(std::fs::read_to_string(root.join("c.txt")).unwrap(), "base");
    let _ = lokai;
    child.wait().unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// R28: Workspace txn adversarial batch (M2-4)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn r28_symlink_or_junction_retarget_after_stage_fails_closed() {
    let (_d, root) = tmp_ws("r28-symlink");
    std::fs::write(root.join("real.txt"), "trusted").unwrap();
    let outside_dir = TempDir::new().unwrap();
    let outside_file = outside_dir.path().join("victim.txt");
    std::fs::write(&outside_file, "victim_original").unwrap();

    let service = svc(&root);
    let mut txn = service.begin().unwrap();

    let link_path = root.join("link.txt");
    std::fs::write(&link_path, "trusted-link-body").unwrap();
    txn.stage_write_file("link.txt", "staged_payload", true)
        .unwrap();
    let _ = std::fs::remove_file(&link_path);
    let retargeted = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&outside_file, &link_path).is_ok()
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::symlink_file;
            symlink_file(&outside_file, &link_path).is_ok()
        }
    };

    if retargeted {
        let err = txn
            .commit("adversary", DataClass::RepositorySource)
            .unwrap_err();
        assert!(
            matches!(
                err,
                TransactionError::Conflict(_) | TransactionError::OutsideWorkspace(_)
            ),
            "commit must fail closed on retargeted symlink, got: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&outside_file).unwrap(),
            "victim_original",
            "outside file must not be modified"
        );
    }
}

#[test]
fn r28_concurrent_cli_and_daemon_commit_denied() {
    let (_d, root) = tmp_ws("r28-concurrent");
    std::fs::write(root.join("shared.txt"), "initial").unwrap();

    let daemon_service = WorkspaceTransactionService::new(
        &root,
        WorkspaceTxnConfig {
            owner: "daemon".into(),
            limits: SecurityLimits::default(),
        },
    )
    .unwrap();

    let cli_service = WorkspaceTransactionService::new(
        &root,
        WorkspaceTxnConfig {
            owner: "cli".into(),
            limits: SecurityLimits::default(),
        },
    )
    .unwrap();

    let mut daemon_txn = daemon_service.begin().unwrap();
    daemon_txn
        .stage_write_file("shared.txt", "daemon_write", true)
        .unwrap();

    let mut cli_txn = cli_service.begin().unwrap();
    cli_txn
        .stage_write_file("shared.txt", "cli_write", true)
        .unwrap();

    // Acquire writer lock simulating active daemon commit in progress
    let lokai_dir = root.join(".lokai");
    let daemon_lock = WriterLock::acquire(&lokai_dir, &daemon_txn.id, "daemon").unwrap();

    // CLI commit attempt must fail with LockContention
    let err = cli_txn
        .commit("cli", DataClass::RepositorySource)
        .unwrap_err();
    assert!(
        matches!(err, TransactionError::LockContention(_)),
        "concurrent commit must be denied via WriterLock, got: {err}"
    );

    drop(daemon_lock);
}

#[test]
fn r28_crash_mid_multifile_commit_recovers_consistently() {
    let (_d, root) = tmp_ws("r28-crash-multi");
    std::fs::write(root.join("f1.txt"), "v1_orig").unwrap();
    std::fs::write(root.join("f2.txt"), "v2_orig").unwrap();
    std::fs::write(root.join("f3.txt"), "v3_orig").unwrap();

    let lokai = root.join(".lokai");
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    txn.stage_write_file("f1.txt", "v1_mutated", true).unwrap();
    txn.stage_write_file("f2.txt", "v2_mutated", true).unwrap();
    txn.stage_write_file("f3.txt", "v3_mutated", true).unwrap();

    let backup = lokai.join("backups").join(&txn.id.0);
    let sets = txn.rw_sets();
    let preview = txn.preview();
    let mut journal = CommitJournal {
        transaction_id: txn.id.clone(),
        base_version: txn.base_version.clone(),
        patch_digest: preview.patch_digest,
        state: TransactionState::Committing,
        operations: vec![],
        progress: 0,
        recovery_instructions: "rollback from backups".into(),
    };
    build_journal(&mut journal, &root, &backup, &sets).unwrap();
    journal.persist(&lokai).unwrap();

    // Apply only the first operation, simulating crash before step 1 and 2
    apply_journal_operation(&root, &mut journal, 0, &lokai).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("f1.txt")).unwrap(),
        "v1_mutated"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("f2.txt")).unwrap(),
        "v2_orig"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("f3.txt")).unwrap(),
        "v3_orig"
    );

    // Simulate process death and crash recovery at startup
    drop(txn);
    let recovery_report = recover_at_startup(&root).unwrap();
    assert!(
        !recovery_report.recovered.is_empty(),
        "startup recovery must roll back incomplete multi-file commit: {recovery_report:?}"
    );

    // Verify workspace is fully consistent (f1 rolled back to v1_orig, no silent partial apply)
    assert_eq!(
        std::fs::read_to_string(root.join("f1.txt")).unwrap(),
        "v1_orig"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("f2.txt")).unwrap(),
        "v2_orig"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("f3.txt")).unwrap(),
        "v3_orig"
    );

    // Ensure new transactions can start cleanly
    let new_service = svc(&root);
    assert!(new_service.begin().is_ok());
}

#[test]
fn stage_edit_refuses_symlink_target_bytes() {
    let (_d, root) = tmp_ws("stage-edit-symlink");
    let outside_dir = TempDir::new().unwrap();
    let outside_file = outside_dir.path().join("victim.txt");
    std::fs::write(&outside_file, "victim_secret").unwrap();
    let link_path = root.join("link.txt");
    let linked = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&outside_file, &link_path).is_ok()
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::symlink_file;
            symlink_file(&outside_file, &link_path).is_ok()
        }
    };
    if !linked {
        return;
    }
    let service = svc(&root);
    let mut txn = service.begin().unwrap();
    let err = txn
        .stage_edit_file("link.txt", "victim_secret", "pwned")
        .unwrap_err();
    assert!(
        matches!(
            err,
            TransactionError::OutsideWorkspace(_) | TransactionError::Other(_)
        ),
        "stage_edit must not return symlink-target bytes, got {err}"
    );
    assert_eq!(
        std::fs::read_to_string(&outside_file).unwrap(),
        "victim_secret"
    );
}

#[test]
fn rename_rollback_restores_both_paths_and_survives_journal_reload() {
    for occupied in [false, true] {
        let (_d, root) = tmp_ws("rename-rollback");
        std::fs::write(root.join("from.txt"), "source").unwrap();
        if occupied {
            std::fs::write(root.join("to.txt"), "destination").unwrap();
        }
        let service = svc(&root);
        let mut txn = service.begin().unwrap();
        txn.stage_rename("from.txt", "to.txt").unwrap();
        let lokai = root.join(".lokai");
        let backup = lokai.join("backups").join(&txn.id.0);
        let mut journal = CommitJournal {
            transaction_id: txn.id.clone(),
            base_version: txn.base_version.clone(),
            patch_digest: txn.preview().patch_digest,
            state: TransactionState::Committing,
            operations: vec![],
            progress: 0,
            recovery_instructions: String::new(),
        };
        build_journal(&mut journal, &root, &backup, &txn.rw_sets()).unwrap();
        journal.persist(&lokai).unwrap();
        apply_journal_operation(&root, &mut journal, 0, &lokai).unwrap();
        assert!(!root.join("from.txt").exists());
        let mut journal = CommitJournal::load(&CommitJournal::path(&lokai, &txn.id)).unwrap();
        let mut legacy = journal.clone();
        legacy.operations[0].rename_destination = None;
        assert!(rollback_journal(&root, &mut legacy, &lokai).is_err());
        assert!(!root.join("from.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("to.txt")).unwrap(),
            "source"
        );
        rollback_journal(&root, &mut journal, &lokai).unwrap();
        rollback_journal(&root, &mut journal, &lokai).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("from.txt")).unwrap(),
            "source"
        );
        if occupied {
            assert_eq!(
                std::fs::read_to_string(root.join("to.txt")).unwrap(),
                "destination"
            );
        } else {
            assert!(!root.join("to.txt").exists());
        }
    }
}
