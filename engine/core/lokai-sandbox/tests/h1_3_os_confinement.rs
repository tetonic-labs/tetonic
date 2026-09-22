//! H1-3: OS network denial and deny-branch reachability.

use std::path::PathBuf;
use std::time::Duration;

use lokai_domain::execution::ProcessClass;
use lokai_sandbox::{
    apply_shell, platform_backend, profile_for_class, IsolationLevel, IsolationOutcome,
    ProcessMode, SandboxControl, SandboxedProcess,
};

fn workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lokai-h13-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[tokio::test]
async fn platform_label_matches_os() {
    let caps = platform_backend().capabilities();
    #[cfg(windows)]
    assert_eq!(caps.platform, "windows");
    #[cfg(target_os = "linux")]
    assert_eq!(caps.platform, "linux");
    #[cfg(target_os = "macos")]
    assert_eq!(caps.platform, "macos");
}

#[tokio::test]
async fn deny_all_either_enforces_or_reports_missing() {
    let caps = platform_backend().capabilities();
    let ws = workspace();
    let req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    let missing = lokai_sandbox::backend::collect_missing_controls(&req, &caps);
    let has_gap = missing
        .iter()
        .any(|m| m.control == SandboxControl::NetworkDenial);
    if caps.network_denial {
        assert!(
            !has_gap,
            "enforced denial must not advertise a High network gap"
        );
    } else {
        assert!(
            has_gap,
            "without OS denial, High NetworkDenial must be reported"
        );
    }
}

#[tokio::test]
async fn network_denial_blocks_outbound_when_available() {
    let caps = platform_backend().capabilities();
    if !caps.network_denial {
        // Enforce-or-refuse: no silent claim. Skip socket proof when unavailable.
        return;
    }
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    req.runtime_limit = Duration::from_secs(15);
    req.mode = ProcessMode::OneShot;
    #[cfg(windows)]
    {
        req = apply_shell(
            req,
            "powershell",
            "try { (New-Object Net.Sockets.TcpClient).Connect('1.1.1.1',443); 'CONNECTED' } catch { 'BLOCKED' }",
        );
    }
    #[cfg(unix)]
    {
        req = apply_shell(
            req,
            "/bin/sh",
            "python3 -c \"import socket; s=socket.socket(); s.settimeout(3); s.connect(('1.1.1.1',443)); print('CONNECTED')\" 2>/dev/null || echo BLOCKED",
        );
    }
    let backend = platform_backend();
    let result = match backend.execute(req).await {
        Ok(SandboxedProcess::Completed(r)) => r,
        Ok(_) => panic!("unexpected service"),
        Err(e) => panic!("spawn failed: {e}"),
    };
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        !combined.contains("CONNECTED"),
        "outbound connect must fail under DenyAll; got {combined:?} report={:?}",
        result.report.outcome
    );
}

#[tokio::test]
async fn standard_profile_still_brokers_high_gap() {
    let caps = platform_backend().capabilities();
    let mut req = profile_for_class(ProcessClass::RepositoryTool, &workspace());
    req.isolation_level = IsolationLevel::Standard;
    // Force a synthetic High gap even if the platform enforces network denial.
    let missing = vec![lokai_sandbox::MissingControl {
        control: SandboxControl::NetworkDenial,
        reason: "synthetic".into(),
        risk_level: lokai_sandbox::RiskLevel::High,
    }];
    let outcome = lokai_sandbox::backend::evaluate_outcome(&caps, &req, Vec::new(), missing)
        .expect("Standard must broker");
    assert!(matches!(
        outcome,
        IsolationOutcome::BrokeredWithWarning {
            user_approval_required: true,
            ..
        }
    ));
}

// ─────────────────────────────────────────────────────────────────────────
// R20: Linux OS filesystem confinement (M2-3)
// ─────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fs_confinement_either_enforces_or_reports_missing() {
    let caps = platform_backend().capabilities();
    let ws = workspace();
    let req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    let missing = lokai_sandbox::backend::collect_missing_controls(&req, &caps);
    let has_read_gap = missing
        .iter()
        .any(|m| m.control == SandboxControl::FilesystemReadRestriction);
    let has_write_gap = missing
        .iter()
        .any(|m| m.control == SandboxControl::FilesystemWriteRestriction);

    if caps.filesystem_read_restrictions {
        assert!(
            !has_read_gap,
            "enforced fs read restrictions must not report a gap"
        );
    } else {
        assert!(
            has_read_gap,
            "without OS fs read confinement, gap must be reported"
        );
    }

    if caps.filesystem_write_restrictions {
        assert!(
            !has_write_gap,
            "enforced fs write restrictions must not report a gap"
        );
    } else {
        assert!(
            has_write_gap,
            "without OS fs write confinement, gap must be reported"
        );
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_fs_confinement_denies_outside_workspace_read() {
    let caps = platform_backend().capabilities();
    if !caps.filesystem_read_restrictions {
        // Enforce or report: skip live kernel check if Landlock unavailable on this kernel
        return;
    }
    let ws = workspace();
    let outside = std::env::temp_dir().join(format!("lokai-outside-secret-{}", std::process::id()));
    std::fs::write(&outside, "super_secret_forbidden_content").unwrap();

    let mut req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    req.runtime_limit = Duration::from_secs(10);
    req.mode = ProcessMode::OneShot;
    req = apply_shell(
        req,
        "/bin/sh",
        &format!("cat {} 2>&1 || echo BLOCKED", outside.display()),
    );

    let backend = platform_backend();
    let result = match backend.execute(req).await {
        Ok(SandboxedProcess::Completed(r)) => r,
        Ok(_) => panic!("unexpected service"),
        Err(e) => panic!("spawn failed: {e}"),
    };
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        !combined.contains("super_secret_forbidden_content"),
        "Landlock must prevent reading files outside allowed roots; got {combined:?}"
    );

    let _ = std::fs::remove_file(&outside);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_fs_confinement_denies_outside_workspace_write() {
    let caps = platform_backend().capabilities();
    if !caps.filesystem_write_restrictions {
        return;
    }
    let ws = workspace();
    let outside =
        std::env::temp_dir().join(format!("lokai-outside-macos-escape-{}", std::process::id()));
    let mut req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    req.runtime_limit = Duration::from_secs(10);
    req.mode = ProcessMode::OneShot;
    req = apply_shell(
        req,
        "/bin/sh",
        &format!("echo hacked > {} 2>&1 || echo BLOCKED", outside.display()),
    );

    let backend = platform_backend();
    let result = match backend.execute(req).await {
        Ok(SandboxedProcess::Completed(r)) => r,
        Ok(_) => panic!("unexpected service"),
        Err(e) => panic!("spawn failed: {e}"),
    };
    assert!(
        !outside.exists(),
        "Seatbelt must deny file-write outside allowed roots; report={:?}",
        result.report.outcome
    );
}

// ─────────────────────────────────────────────────────────────────────────
// R22: Windows unelevated network denial (M2-3)
// ─────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
#[tokio::test]
async fn windows_strict_deny_all_either_enforces_or_denies() {
    let caps = platform_backend().capabilities();
    let ws = workspace();
    let mut req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    req.isolation_level = IsolationLevel::Strict;
    req.network = lokai_sandbox::NetworkPolicy::DenyAll;

    let backend = platform_backend();
    if caps.network_denial {
        // Elevated / enforced path: must succeed execution with network denial control enforced
        req.runtime_limit = Duration::from_secs(15);
        req.mode = ProcessMode::OneShot;
        req = apply_shell(
            req,
            "powershell",
            "try { (New-Object Net.Sockets.TcpClient).Connect('1.1.1.1',443); 'CONNECTED' } catch { 'BLOCKED' }",
        );
        let result = match backend.execute(req).await {
            Ok(SandboxedProcess::Completed(r)) => r,
            Ok(_) => panic!("unexpected service"),
            Err(e) => panic!("spawn failed: {e}"),
        };
        let combined = format!("{}{}", result.stdout, result.stderr);
        assert!(
            !combined.contains("CONNECTED"),
            "outbound must be blocked on Windows"
        );
    } else {
        // Unelevated path: Strict request with missing network denial MUST fail closed (Denied), never silent Sandboxed
        let missing = lokai_sandbox::backend::collect_missing_controls(&req, &caps);
        assert!(
            missing
                .iter()
                .any(|m| m.control == SandboxControl::NetworkDenial),
            "unelevated Windows must report High NetworkDenial missing control"
        );
        let outcome = lokai_sandbox::backend::evaluate_outcome(&caps, &req, Vec::new(), missing);
        assert!(
            outcome.is_err(),
            "Strict isolation must deny execution when OS network denial is unavailable: {outcome:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// R23: Windows OS filesystem confinement (M2-3)
// ─────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
#[tokio::test]
async fn windows_fs_confinement_either_enforces_or_reports_missing() {
    let caps = platform_backend().capabilities();
    let ws = workspace();
    let req = profile_for_class(ProcessClass::ModelRequestedShell, &ws);
    let missing = lokai_sandbox::backend::collect_missing_controls(&req, &caps);

    let has_read_gap = missing
        .iter()
        .any(|m| m.control == SandboxControl::FilesystemReadRestriction);
    let has_write_gap = missing
        .iter()
        .any(|m| m.control == SandboxControl::FilesystemWriteRestriction);

    if caps.filesystem_read_restrictions {
        assert!(
            !has_read_gap,
            "enforced fs read restrictions must not report a gap"
        );
    } else {
        assert!(
            has_read_gap,
            "without OS fs read confinement, gap must be reported"
        );
    }

    if caps.filesystem_write_restrictions {
        assert!(
            !has_write_gap,
            "enforced fs write restrictions must not report a gap"
        );
    } else {
        assert!(
            has_write_gap,
            "without OS fs write confinement, gap must be reported"
        );
    }

    // Working directory outside allowed read roots must be rejected with Denied
    let mut bad_wd_req = req.clone();
    bad_wd_req.working_directory = std::env::temp_dir().join("some_arbitrary_unallowed_dir");
    let _ = std::fs::create_dir_all(&bad_wd_req.working_directory);
    let backend = platform_backend();
    let res = backend.execute(bad_wd_req).await;
    let is_denied = matches!(res, Err(lokai_sandbox::SandboxError::Denied(_)));
    assert!(
        is_denied,
        "working directory outside allowed roots must be denied"
    );
}
