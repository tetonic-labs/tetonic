//! Adversarial containment tests (M2-3).

use std::path::PathBuf;
use std::time::Duration;

use lokai_domain::execution::ProcessClass;
use lokai_sandbox::{
    apply_executable, platform_backend, profile_for_class, ProcessMode, SandboxError,
    SandboxedProcess, SyncLongLivedService,
};

fn runner_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(if cfg!(windows) {
            "lokai-sandbox-adv.exe"
        } else {
            "lokai-sandbox-adv"
        })
}

fn workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lokai-sandbox-adv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn run_scenario(
    class: ProcessClass,
    scenario: &str,
    limit: Duration,
) -> Result<lokai_sandbox::SandboxRunResult, SandboxError> {
    let ws = workspace();
    let mut req = profile_for_class(class, &ws);
    req.runtime_limit = limit;
    req.mode = ProcessMode::OneShot;
    let runner = runner_path();
    req = apply_executable(req, &runner.display().to_string(), &[scenario.to_string()]);
    let backend = platform_backend();
    match backend.execute(req).await? {
        SandboxedProcess::Completed(r) => Ok(r),
        SandboxedProcess::Service(_) => panic!("unexpected service"),
    }
}

#[tokio::test]
async fn adversarial_infinite_loop_terminated() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "infinite_loop",
        Duration::from_secs(3),
    )
    .await;
    assert!(
        r.is_err() || !r.as_ref().unwrap().success,
        "loop should timeout or fail"
    );
}

#[tokio::test]
async fn adversarial_output_flood_truncated() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "output_flood",
        Duration::from_secs(5),
    )
    .await;
    assert!(
        matches!(r, Err(SandboxError::Timeout))
            || r.as_ref()
                .map(|x| x.truncated || !x.success)
                .unwrap_or(false),
        "output flood must be bounded or timed out"
    );
}

#[tokio::test]
async fn adversarial_secret_env_not_inherited() {
    std::env::set_var("LOKAI_TEST_SECRET", "TOP_SECRET_VALUE");
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    req.runtime_limit = Duration::from_secs(10);
    req.mode = ProcessMode::OneShot;
    #[cfg(windows)]
    let script = "echo %LOKAI_TEST_SECRET%";
    #[cfg(not(windows))]
    let script = "echo $LOKAI_TEST_SECRET";
    #[cfg(windows)]
    let req = lokai_sandbox::apply_shell(req, "cmd", script);
    #[cfg(not(windows))]
    let req = lokai_sandbox::apply_shell(req, "sh", script);
    let backend = platform_backend();
    let result = backend.execute(req).await.unwrap();
    std::env::remove_var("LOKAI_TEST_SECRET");
    if let SandboxedProcess::Completed(r) = result {
        assert!(
            !r.stdout.contains("TOP_SECRET_VALUE"),
            "secret leaked: {}",
            r.stdout
        );
    }
}

#[tokio::test]
async fn adversarial_build_profile_reports_partial_network() {
    let backend = platform_backend();
    let caps = backend.capabilities();
    let ws = workspace();
    let req = profile_for_class(ProcessClass::BuildVerification, &ws);
    assert!(matches!(req.network, lokai_sandbox::NetworkPolicy::DenyAll));
    if !caps.network_denial {
        let r = run_scenario(
            ProcessClass::BuildVerification,
            "network_attempt",
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                r.report.outcome,
                lokai_sandbox::IsolationOutcome::BrokeredWithWarning { .. }
                    | lokai_sandbox::IsolationOutcome::Sandboxed { .. }
            ),
            "must not silently claim full sandbox when network denied: {:?}",
            r.report.outcome
        );
    }
}

#[tokio::test]
async fn adversarial_fork_bomb_contained() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "fork_bomb",
        Duration::from_secs(5),
    )
    .await;
    assert!(
        r.is_err() || !r.as_ref().unwrap().success || r.as_ref().unwrap().truncated,
        "fork bomb should be contained or time out: {:?}",
        r.as_ref().map(|x| (&x.success, x.exit_code))
    );
}

#[tokio::test]
async fn capabilities_detected_at_startup() {
    let caps = platform_backend().capabilities();
    assert!(caps.process_tree_containment);
    assert!(caps.runtime_enforcement);
    assert!(caps.environment_filtering);
    assert!(!caps.supported_process_classes.is_empty());
}

#[tokio::test]
async fn long_lived_service_start_and_stop() {
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::InternalService, &ws);
    req.runtime_limit = Duration::from_secs(30);
    req.mode = ProcessMode::LongLived;
    #[cfg(windows)]
    {
        req = apply_executable(
            req,
            &std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()),
            &["/C".into(), "ping -n 60 127.0.0.1".into()],
        );
    }
    #[cfg(not(windows))]
    {
        req = apply_executable(req, "sleep", &["60".into()]);
    }
    let backend = platform_backend();
    let result = backend.execute(req).await.expect("start service");
    if let SandboxedProcess::Service(mut handle) = result {
        assert!(handle.is_alive());
        handle.force_terminate().await.expect("force terminate");
        assert!(!handle.is_alive());
    } else {
        panic!("expected long-lived service handle");
    }
}

#[tokio::test]
async fn long_lived_graceful_stop_after_ignore_shutdown() {
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::InternalService, &ws);
    req.runtime_limit = Duration::from_secs(30);
    req.mode = ProcessMode::OneShot;
    req = apply_executable(
        req,
        &runner_path().display().to_string(),
        &["ignore_shutdown".into()],
    );
    let backend = platform_backend();
    let result = backend.execute(req).await;
    let completed_bad = match &result {
        Ok(SandboxedProcess::Completed(r)) => !r.success,
        Ok(SandboxedProcess::Service(_)) => false,
        Err(_) => true,
    };
    assert!(
        completed_bad,
        "ignore_shutdown must not complete successfully within limit"
    );

    let mut req = profile_for_class(ProcessClass::InternalService, &ws);
    req.mode = ProcessMode::LongLived;
    #[cfg(windows)]
    {
        req = apply_executable(
            req,
            &std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()),
            &["/C".into(), "ping -n 120 127.0.0.1".into()],
        );
    }
    #[cfg(not(windows))]
    {
        req = apply_executable(req, "sleep", &["120".into()]);
    }
    if let SandboxedProcess::Service(mut handle) = backend.execute(req).await.unwrap() {
        assert!(handle.is_alive());
        handle
            .graceful_stop(Duration::from_millis(100))
            .await
            .expect("graceful then force");
        assert!(!handle.is_alive());
    } else {
        panic!("expected service");
    }
}

#[tokio::test]
async fn sync_long_lived_restart_preserves_execution_id() {
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::InternalService, &ws);
    #[cfg(windows)]
    {
        req = apply_executable(
            req,
            &std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()),
            &["/C".into(), "ping -n 30 127.0.0.1".into()],
        );
    }
    #[cfg(not(windows))]
    {
        req = apply_executable(req, "sleep", &["30".into()]);
    }
    let mut svc =
        SyncLongLivedService::spawn_with_execution_id(req.clone(), "lsp_test_restart".into())
            .expect("spawn");
    assert!(svc.is_alive());
    assert_eq!(svc.accounting.execution_id, "lsp_test_restart");
    svc.restart(req).expect("restart");
    assert_eq!(svc.accounting.execution_id, "lsp_test_restart");
    assert_eq!(svc.accounting.restart_count, 1);
    assert!(svc.is_alive());
    svc.force_terminate().expect("stop");
}

#[tokio::test]
async fn adversarial_child_breakaway_contained() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "breakaway",
        Duration::from_secs(5),
    )
    .await
    .expect("breakaway scenario must finish within runtime limit");
    assert!(
        r.exit_code.is_some(),
        "breakaway parent must exit; descendants reaped by job teardown"
    );
}

#[tokio::test]
async fn adversarial_memory_exhaust_contained() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "memory_exhaust",
        Duration::from_secs(5),
    )
    .await;
    assert!(
        matches!(r, Err(SandboxError::Timeout)) || r.as_ref().map(|x| !x.success).unwrap_or(true),
        "memory exhaustion must timeout or fail: {:?}",
        r.as_ref().map(|x| (x.success, x.exit_code))
    );
}

#[tokio::test]
async fn adversarial_read_secret_not_leaked() {
    let r = run_scenario(
        ProcessClass::ModelRequestedShell,
        "read_secret",
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert!(
        !r.stdout.contains("BEGIN") && !r.stdout.contains("SECRET:-----"),
        "secret file content leaked: {}",
        r.stdout
    );
}

#[tokio::test]
async fn adversarial_write_outside_reports_or_blocks() {
    let target = std::env::temp_dir().join("lokai_sandbox_escape.txt");
    let _ = std::fs::remove_file(&target);
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "write_outside",
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    let caps = platform_backend().capabilities();
    if !caps.filesystem_write_restrictions {
        assert!(
            matches!(
                r.report.outcome,
                lokai_sandbox::IsolationOutcome::BrokeredWithWarning { .. }
            ),
            "write outside roots must not claim full sandbox without OS fs enforcement"
        );
    } else {
        assert!(!target.exists(), "write escaped sandbox");
    }
}

#[tokio::test]
async fn adversarial_env_exfiltration_blocked() {
    std::env::set_var("LOKAI_AGENT_SECRET_TOKEN", "must_not_leak");
    let r = run_scenario(
        ProcessClass::ModelRequestedShell,
        "env_exfil",
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    std::env::remove_var("LOKAI_AGENT_SECRET_TOKEN");
    assert!(
        !r.stdout.contains("must_not_leak"),
        "secret env leaked via exfil scenario: {}",
        r.stdout
    );
}

#[tokio::test]
async fn adversarial_symlink_escape_reports_or_blocks() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "symlink_escape",
        Duration::from_secs(15),
    )
    .await
    .unwrap();
    let caps = platform_backend().capabilities();
    if !caps.filesystem_read_restrictions {
        assert!(
            matches!(
                r.report.outcome,
                lokai_sandbox::IsolationOutcome::BrokeredWithWarning { .. }
            ),
            "symlink escape must not claim full sandbox without OS fs enforcement"
        );
    } else {
        assert!(!r.stdout.contains("LINK:"), "symlink read escaped");
    }
}

#[tokio::test]
async fn adversarial_survive_parent_tree_killed() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "survive_parent",
        Duration::from_secs(3),
    )
    .await;
    // Parent (scenario) exits quickly; job containment should reap descendants.
    assert!(
        r.is_ok(),
        "survive_parent scenario should complete: {:?}",
        r.as_ref().err()
    );
}

#[tokio::test]
async fn adversarial_background_children_contained() {
    let r = run_scenario(
        ProcessClass::BuildVerification,
        "background_children",
        Duration::from_secs(5),
    )
    .await
    .expect("background_children must finish within runtime limit");
    assert!(
        r.exit_code.is_some(),
        "parent must exit; background children must not wedge the sandbox run"
    );
}
