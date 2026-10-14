use tetonic_domain::{work_scope::WorkScope, DataClass};
use tetonic_transaction::{StagedCheckResult, WorkspaceTransactionService, WorkspaceTxnConfig};

fn stage(svc: &WorkspaceTransactionService, content: &str) {
    svc.with_active(|t| t.stage_write_file("result.txt", content, false))
        .unwrap();
}
fn check(svc: &WorkspaceTransactionService, good: &str) -> StagedCheckResult {
    svc.run_staged_check(
        &svc.staged_check_binding().unwrap(),
        "fixture check",
        &WorkScope::default().cancellation_signal(),
        |view| StagedCheckResult {
            success: std::fs::read_to_string(view.join("result.txt")).unwrap() == good,
            output: "checked exact bytes".into(),
        },
    )
    .unwrap()
}

#[test]
fn failed_check_can_be_repaired_and_fresh_edits_must_be_checked_again() {
    let dir = tempfile::tempdir().unwrap();
    let svc = WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    stage(&svc, "broken");
    assert!(!check(&svc, "ready").success);
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
    assert!(!dir.path().join("result.txt").exists());
    stage(&svc, "ready");
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
    assert!(check(&svc, "ready").success);
    stage(&svc, "changed after checking");
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
    assert!(check(&svc, "changed after checking").success);
    let receipt = svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .unwrap()
        .unwrap();
    assert!(receipt.artifact.verification_artifact_id.is_some());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "changed after checking"
    );
}

#[test]
fn checks_reject_stale_binding_cancellation_source_rewrites_and_changed_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), "original").unwrap();
    let svc = WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    stage(&svc, "draft");
    let stale = svc.staged_check_binding().unwrap();
    stage(&svc, "ready");
    let scope = WorkScope::default();
    assert!(svc
        .run_staged_check(&stale, "stale", &scope.cancellation_signal(), |_| panic!(
            "stale check ran"
        ))
        .is_err());
    scope.cancel();
    assert!(svc
        .run_staged_check(
            &svc.staged_check_binding().unwrap(),
            "canceled",
            &scope.cancellation_signal(),
            |_| panic!("canceled check ran")
        )
        .is_err());
    let result = svc
        .run_staged_check(
            &svc.staged_check_binding().unwrap(),
            "rewriter",
            &WorkScope::default().cancellation_signal(),
            |view| {
                std::fs::write(view.join("result.txt"), "command rewrote the source").unwrap();
                std::fs::write(view.join("generated.txt"), "not a deliverable").unwrap();
                StagedCheckResult {
                    success: true,
                    output: "process exited zero".into(),
                }
            },
        )
        .unwrap();
    assert!(!result.success);
    assert!(!dir.path().join("generated.txt").exists());
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
    assert!(
        check(&svc, "ready").success,
        "check reruns on the real patch, not command edits"
    );
    std::fs::write(dir.path().join("input.txt"), "peer changed dependency").unwrap();
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
    assert!(!dir.path().join("result.txt").exists());
}

#[test]
fn verification_copy_excludes_internal_credentials_and_refuses_oversized_sources() {
    let dir = tempfile::tempdir().unwrap();
    let svc = WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    std::fs::write(dir.path().join(".env"), "private").unwrap();
    stage(&svc, "ready");
    svc.run_staged_check(
        &svc.staged_check_binding().unwrap(),
        "inspect",
        &WorkScope::default().cancellation_signal(),
        |view| {
            assert!(!view.join(".env").exists());
            assert!(!view.join(".lokai").exists());
            StagedCheckResult {
                success: true,
                output: "ok".into(),
            }
        },
    )
    .unwrap();
    let oversized = std::fs::File::create(dir.path().join("large.dat")).unwrap();
    oversized.set_len(257 * 1024 * 1024).unwrap();
    assert!(svc
        .run_staged_check(
            &svc.staged_check_binding().unwrap(),
            "large",
            &WorkScope::default().cancellation_signal(),
            |_| panic!("oversized check ran")
        )
        .is_err());
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
}

#[test]
fn changed_staging_bytes_are_refused_before_the_check_runs() {
    let dir = tempfile::tempdir().unwrap();
    let svc = WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
    stage(&svc, "approved bytes");
    let binding = svc.staged_check_binding().unwrap();
    svc.with_active(|txn| {
        let path = txn.preview().operations[0]
            .new_content_path
            .clone()
            .unwrap();
        std::fs::write(path, "substituted bytes").unwrap();
        Ok(())
    })
    .unwrap();
    let result = svc.run_staged_check(
        &binding,
        "tampered",
        &WorkScope::default().cancellation_signal(),
        |_| panic!("tampered bytes must not run"),
    );
    assert!(result.is_err());
    assert!(svc
        .commit_active_if_any("test", DataClass::RepositorySource)
        .is_err());
}

#[test]
fn independent_workers_check_in_parallel_without_seeing_each_others_pending_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut services = Vec::new();
    for name in ["alpha", "beta"] {
        let service =
            WorkspaceTransactionService::new(dir.path(), WorkspaceTxnConfig::default()).unwrap();
        service
            .with_active(|t| t.stage_write_file(name, name, false))
            .unwrap();
        services.push((name, service));
    }
    let (started, events) = std::sync::mpsc::channel();
    let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let threads: Vec<_> = services
        .into_iter()
        .map(|(name, service)| {
            let started = started.clone();
            let release = release.clone();
            std::thread::spawn(move || {
                let binding = service.staged_check_binding().unwrap();
                let outcome = service
                    .run_staged_check(
                        &binding,
                        "concurrent check",
                        &WorkScope::default().cancellation_signal(),
                        |view| {
                            assert!(view.join(name).exists());
                            assert!(!view
                                .join(if name == "alpha" { "beta" } else { "alpha" })
                                .exists());
                            started.send(()).unwrap();
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(5);
                            while !release.load(std::sync::atomic::Ordering::Acquire) {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "checks were serialized across agents"
                                );
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            StagedCheckResult {
                                success: true,
                                output: "checked private pending files".into(),
                            }
                        },
                    )
                    .unwrap();
                assert!(outcome.success);
                service
            })
        })
        .collect();
    for _ in 0..2 {
        events
            .recv_timeout(std::time::Duration::from_secs(4))
            .unwrap();
    }
    release.store(true, std::sync::atomic::Ordering::Release);
    for thread in threads {
        thread
            .join()
            .unwrap()
            .commit_active_if_any("test", DataClass::RepositorySource)
            .unwrap();
    }
    for name in ["alpha", "beta"] {
        assert_eq!(
            std::fs::read_to_string(dir.path().join(name)).unwrap(),
            name
        );
    }
}
