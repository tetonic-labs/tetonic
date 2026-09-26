//! ProcessExecutor tests in lokai-sandbox (AC2-3 / C1 / M2-3).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tetonic_domain::execution::{
    AuthorizedAction, CanonicalActionParameters, ExecutionOutcome, IssuedCapability, ProposedAction,
};
use tetonic_domain::sinks::{AuthorizedProcessRequest, ProcessBroker};
use tetonic_domain::{
    ActionId, ActionKind, AgentId, CapabilityError, DataClass, ProcessSink, SessionId,
};
use tetonic_sandbox::{EnforcementLevel, NonCodingProcessValidator, ProcessExecutor};

fn temp_ws(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pe-{prefix}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn sandboxed_tier_runs_via_os_backend() {
    let dir = temp_ws("sandbox");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Sandboxed,
        Arc::new(NonCodingProcessValidator),
    );
    #[cfg(windows)]
    let probe = pe.run_shell("echo hello-from-sandbox");
    #[cfg(not(windows))]
    let probe = pe.run_shell("echo lokai_sandbox_ok");
    let r = probe.expect("sandbox shell");
    assert!(
        r.success,
        "sandboxed shell must succeed for trivial echo; got: {}",
        r.output
    );
    assert!(
        r.output.contains("hello-from-sandbox") || r.output.contains("lokai_sandbox_ok"),
        "stdout must be captured through inheritable pipes; got: {}",
        r.output
    );
    #[cfg(windows)]
    {
        let r2 = pe.run_shell("exit 0").expect("exit 0");
        assert!(
            r2.success,
            "sandboxed `exit 0` must report success; got: {}",
            r2.output
        );
        let started = std::time::Instant::now();
        let r3 = pe.run_shell("ping -n 2 127.0.0.1").expect("quiet ping");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "sandboxed quiet command must not hang on pipe drain; elapsed {:?} output={}",
            started.elapsed(),
            r3.output
        );
        let r4 = pe.run_shell("cd").expect("cd");
        let expected = dir
            .canonicalize()
            .unwrap_or_else(|_| dir.clone())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .trim_end_matches(['\\', '/'])
            .to_string();
        let out_lower = r4.output.to_lowercase();
        assert!(
            r4.success && out_lower.contains(&expected.to_lowercase()),
            "sandboxed shell cwd must be the workspace; expected substring {expected:?}; got: {}",
            r4.output
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandboxed_git_runs_via_os_backend() {
    let dir = temp_ws("git");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Sandboxed,
        Arc::new(NonCodingProcessValidator),
    );
    let r = pe.run_git(["--version"]).expect("git --version");
    assert!(r.success, "git via sandbox: {}", r.output);
    assert!(
        r.output.to_lowercase().contains("git"),
        "unexpected output: {}",
        r.output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn minimal_env_hides_inherited_secrets() {
    std::env::set_var("LOKAI_EXEC_SECRET_TEST", "TOP_SECRET_VALUE");
    let dir = temp_ws("env");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    #[cfg(windows)]
    let probe = pe.run_shell("echo %LOKAI_EXEC_SECRET_TEST%");
    #[cfg(not(windows))]
    let probe = pe.run_shell("echo $LOKAI_EXEC_SECRET_TEST");
    std::env::remove_var("LOKAI_EXEC_SECRET_TEST");
    let r = probe.expect("shell probe");
    assert!(
        !r.output.contains("TOP_SECRET_VALUE"),
        "secret leaked through minimal env: {}",
        r.output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn minimal_env_does_not_expose_the_operator_profile() {
    let dir = temp_ws("home");
    let marker = format!("pe-home-{}", std::process::id());
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Sandboxed,
        Arc::new(NonCodingProcessValidator),
    );
    #[cfg(windows)]
    let probe = pe.run_shell("echo %USERPROFILE%");
    #[cfg(not(windows))]
    let probe = pe.run_shell("echo $HOME");
    let r = probe.expect("home probe");
    assert!(
        r.output.contains(&marker),
        "home was not the workspace: {}",
        r.output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn minimal_env_does_not_expose_the_operator_temp() {
    let dir = temp_ws("temp");
    let marker = format!("pe-temp-{}", std::process::id());
    for level in [EnforcementLevel::Sandboxed, EnforcementLevel::Constrained] {
        let pe = ProcessExecutor::new(&dir, level, Arc::new(NonCodingProcessValidator));
        #[cfg(windows)]
        let probe = pe.run_shell("echo %TEMP%");
        #[cfg(not(windows))]
        let probe = pe.run_shell("printf '%s' \"$TMPDIR\"");
        let r = probe.expect("temp probe");
        assert!(
            r.output.contains(&marker),
            "temp was not the workspace for {level:?}: {}",
            r.output
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_is_capped() {
    let huge = "x".repeat(tetonic_sandbox::exec::MAX_OUTPUT_BYTES + 1000);
    let capped = tetonic_sandbox::exec::truncate_output(&huge);
    assert!(capped.contains("truncated"));
    assert!(capped.len() < huge.len());
}

#[test]
fn cancel_stops_long_run() {
    let dir = temp_ws("cancel");
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    #[cfg(windows)]
    let mut cmd = tetonic_sandbox::process_executor::shell_command("ping -n 30 127.0.0.1 > nul");
    #[cfg(not(windows))]
    let mut cmd = tetonic_sandbox::process_executor::shell_command("sleep 30");
    cmd.current_dir(&dir);
    let handle = std::thread::spawn(move || {
        tetonic_sandbox::exec::command_output_with_timeout_and_cancel(
            &mut cmd,
            Duration::from_secs(60),
            Some(&cancel_flag),
            tetonic_sandbox::exec::EnvMode::Minimal,
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let result = handle.join().unwrap();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("canceled"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn build_authorized_action(
    dir: &std::path::Path,
    action_id: &str,
    kind: ActionKind,
    executable: Option<String>,
    args: Vec<String>,
    workspace_version: Option<tetonic_domain::WorkspaceVersion>,
    working_directory: Option<String>,
) -> AuthorizedAction {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let action = ProposedAction {
        action_id: ActionId::new(action_id),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: workspace_version.clone(),
        data_class: DataClass::RepositorySource,
        kind: kind.clone(),
        parameters: tetonic_domain::finalize_parameters(
            &kind,
            CanonicalActionParameters {
                digest: String::new(),
                executable_identity: executable,
                resolved_path: None,
                arguments: args,
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: working_directory.or_else(|| Some(dir.display().to_string())),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: None,
            },
        ),
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    AuthorizedAction {
        capability: IssuedCapability {
            capability_id: tetonic_domain::CapabilityId::new(format!("cap_{action_id}")),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: action.agent_id.clone(),
            action_kind: kind,
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version,
            data_classification: DataClass::RepositorySource,
            issuance_timestamp: now,
            expiration: now + 300,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v2".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    }
}

#[tokio::test]
async fn process_sink_runs_verify() {
    let dir = temp_ws("sink");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let auth = build_authorized_action(
        &dir,
        "verify_sink",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["--version".into()],
        None,
        None,
    );
    let outcome = pe
        .run_process(&auth, &tetonic_domain::work_scope::WorkScope::default())
        .await;
    assert!(matches!(
        outcome,
        ExecutionOutcome::Completed { .. } | ExecutionOutcome::Failed { .. }
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn adversarial_expired_capability_is_rejected() {
    let dir = temp_ws("adv-exp");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "exp",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.capability.issuance_timestamp = 0;
    auth.capability.expiration = 1; // expired
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(result.is_err(), "expired capability must be rejected");
    let err_str = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(err_str.contains("expired"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn adversarial_exhausted_use_count_is_rejected() {
    let dir = temp_ws("adv-uses");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "uses",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.capability.max_use_count = 1;
    auth.capability.current_use_count = 1; // exhausted
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(result.is_err(), "exhausted use count must be rejected");
    let err_str = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(err_str.contains("consumed") || err_str.contains("exhausted"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn adversarial_kind_mismatch_is_rejected() {
    let dir = temp_ws("adv-kind");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "kind",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.capability.action_kind = ActionKind::ExecuteShell; // mismatch
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(result.is_err(), "kind mismatch must be rejected");
    let err_str = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(err_str.contains("scope") || err_str.contains("mismatch"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn adversarial_revoked_capability_is_rejected() {
    let dir = temp_ws("adv-revoked");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "revoked",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.capability.revoked = true;
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(result.is_err(), "revoked capability must be rejected");
    let err_str = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(err_str.contains("revoked"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn adversarial_digest_mismatch_is_rejected() {
    let dir = temp_ws("adv-digest");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "digest",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.capability.canonical_parameter_digest = "other_digest".into();
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(result.is_err(), "digest mismatch must be rejected");
    let err_str = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(err_str.contains("scope") || err_str.contains("mismatch"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn git_via_broker_requires_capability() {
    struct DenyAll;
    impl tetonic_domain::CapabilityConsumer for DenyAll {
        fn authorize(&self, _: &AuthorizedAction) -> Result<(), CapabilityError> {
            Err(CapabilityError::ScopeMismatch)
        }
    }

    let dir = temp_ws("git-cap");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Sandboxed,
        Arc::new(NonCodingProcessValidator),
    )
    .with_capability_consumer(Arc::new(DenyAll));
    let auth = build_authorized_action(
        &dir,
        "git",
        ActionKind::ExecuteProcess,
        Some("git".into()),
        vec!["--version".into()],
        None,
        None,
    );
    let req = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: auth,
    };
    let result = pe.execute(req).await;
    assert!(
        result.is_err(),
        "denied capability must block git broker execute"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// =========================================================================
// C1 Specific Test Cases
// =========================================================================

/// C1: Process execution with a valid non-repository cwd succeeds.
#[tokio::test]
async fn process_cwd_without_repository() {
    let non_repo_dir = temp_ws("non-repo");
    let pe = ProcessExecutor::new(
        &non_repo_dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let auth = build_authorized_action(
        &non_repo_dir,
        "non_repo_action",
        ActionKind::ExecuteProcess,
        Some(if cfg!(windows) {
            "cmd".into()
        } else {
            "sh".into()
        }),
        if cfg!(windows) {
            vec!["/C".into(), "echo marker_output_success".into()]
        } else {
            vec!["-c".into(), "echo marker_output_success".into()]
        },
        None, // No workspace version claimed
        Some(non_repo_dir.display().to_string()),
    );
    let outcome = pe
        .run_process(&auth, &tetonic_domain::work_scope::WorkScope::default())
        .await;
    match outcome {
        ExecutionOutcome::Completed { ok, summary, .. } => {
            assert!(ok, "execution must succeed; summary: {summary}");
            assert!(
                summary.contains("marker_output_success"),
                "must contain real marker output; got: {summary}"
            );
        }
        ExecutionOutcome::Failed { reason, .. } => {
            panic!("process execution unexpectedly failed: {reason}");
        }
        other => {
            panic!("unexpected outcome: {other:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&non_repo_dir);
}

/// C1: Missing or empty process cwd must be denied before execution.
#[tokio::test]
async fn process_missing_cwd_denied() {
    let dir = temp_ws("missing-cwd");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    let mut auth = build_authorized_action(
        &dir,
        "missing_cwd",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        None,
    );
    auth.action.parameters.working_directory = None;
    let outcome = pe
        .run_process(&auth, &tetonic_domain::work_scope::WorkScope::default())
        .await;
    assert!(
        matches!(outcome, ExecutionOutcome::Failed { .. }),
        "missing cwd must be rejected"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// C1: A token issued for one cwd cannot be tampered/replayed with a different cwd.
#[tokio::test]
async fn process_token_cwd_tamper_denied() {
    let dir = temp_ws("tamper-orig");
    let tampered_dir = temp_ws("tamper-evil");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    // Action's working_directory is changed to tampered_dir
    let auth = build_authorized_action(
        &dir,
        "tamper_cwd",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        None,
        Some(tampered_dir.display().to_string()),
    );
    let outcome = pe
        .run_process(&auth, &tetonic_domain::work_scope::WorkScope::default())
        .await;
    assert!(
        matches!(outcome, ExecutionOutcome::Failed { .. }),
        "tampered cwd must be rejected by validator"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tampered_dir);
}

/// C1: Non-coding validator must deny an action claiming a workspace version.
#[tokio::test]
async fn process_claimed_version_without_validator_denied() {
    let dir = temp_ws("claimed-version");
    let pe = ProcessExecutor::new(
        &dir,
        EnforcementLevel::Constrained,
        Arc::new(NonCodingProcessValidator),
    );
    // Fake a workspace version claim
    let fake_wv = tetonic_domain::WorkspaceVersion {
        repository_id: tetonic_domain::RepositoryId::new("repo"),
        version_scheme: tetonic_domain::WorkspaceVersionScheme::Git,
        git_head: None,
        dirty_state_digest: tetonic_domain::ContentDigest::new("d1"),
        tracked_state_digest: tetonic_domain::ContentDigest::new("d2"),
        relevant_path_digests: std::collections::BTreeMap::new(),
        index_generation: None,
    };
    let auth = build_authorized_action(
        &dir,
        "claimed_version",
        ActionKind::ExecuteProcess,
        Some("cargo".into()),
        vec!["check".into()],
        Some(fake_wv),
        Some(dir.display().to_string()),
    );
    let outcome = pe
        .run_process(&auth, &tetonic_domain::work_scope::WorkScope::default())
        .await;
    assert!(
        matches!(outcome, ExecutionOutcome::Failed { .. }),
        "claimed workspace version without coding validator must be denied"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[path = "support/process_cancellation.rs"]
mod cancellation;
