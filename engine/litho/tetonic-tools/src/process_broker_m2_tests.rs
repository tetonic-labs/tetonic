//! M2 IMPLEMENT: ProcessBroker::execute is sandboxed or refused (CH-1 / D-1 / D-4).

use super::*;
use tetonic_domain::sinks::{
    AuthorizedProcessRequest, AuthorizedServiceRequest, ProcessBroker, ProcessBrokerError,
};
use tetonic_domain::{
    prepare_proposed_action, ActionId, ActionKind, AgentId, DataClass, IssuedCapability,
    ProposedAction, SessionId,
};

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn valid_capability(action: &ProposedAction) -> IssuedCapability {
    let now = now_secs();
    IssuedCapability {
        capability_id: tetonic_domain::CapabilityId::new("cap_m2"),
        session_id: action.session_id.clone(),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: action.agent_id.clone(),
        action_kind: action.kind.clone(),
        canonical_parameter_digest: action.parameters.digest.clone(),
        workspace_version: action.workspace_version.clone(),
        data_classification: action.data_class,
        issuance_timestamp: now,
        expiration: now + 300,
        max_use_count: 1,
        current_use_count: 0,
        issuing_policy_version: "v2".into(),
        approval_record_id: None,
        revoked: false,
    }
}

fn shell_action(
    workspace: &std::path::Path,
    script: &str,
    cwd: Option<&std::path::Path>,
) -> ProposedAction {
    prepare_proposed_action(ProposedAction {
        action_id: ActionId::new("m2_exec"),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: Some(
            tetonic_transaction::version::capture_workspace_version(workspace, &[]).unwrap(),
        ),
        data_class: DataClass::RepositorySource,
        kind: ActionKind::ExecuteShell,
        parameters: tetonic_domain::execution::CanonicalActionParameters {
            digest: String::new(),
            executable_identity: None,
            resolved_path: None,
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: Some(script.as_bytes().to_vec()),
            working_directory: Some(cwd.unwrap_or(workspace).display().to_string()),
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: Some(tetonic_domain::execution::ProcessClass::BuildVerification),
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: None,
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    })
}

fn request_for(action: ProposedAction) -> AuthorizedProcessRequest {
    AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: AuthorizedAction {
            capability: valid_capability(&action),
            action,
        },
    }
}

#[tokio::test]
async fn constrained_execute_is_refused() {
    let dir = std::env::temp_dir().join(format!("pe-m2-constrained-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let pe = ProcessExecutor::new(&dir, EnforcementLevel::Constrained);
    #[cfg(windows)]
    let script = "echo m2-constrained";
    #[cfg(not(windows))]
    let script = "echo m2-constrained";
    let result = pe
        .execute(request_for(shell_action(&dir, script, None)))
        .await;
    let err = match result {
        Err(e) => e.to_string(),
        Ok(ok) => panic!(
            "Constrained execute must refuse, got success={}",
            ok.success
        ),
    };
    assert!(
        err.contains("Sandboxed") || err.contains("refused"),
        "constrained refuse must mention Sandboxed; got {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn sandboxed_execute_includes_audit_line() {
    let dir = std::env::temp_dir().join(format!("pe-m2-audit-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let pe = ProcessExecutor::new(&dir, EnforcementLevel::Sandboxed);
    #[cfg(windows)]
    let script = "echo m2-sandboxed-ok";
    #[cfg(not(windows))]
    let script = "echo m2-sandboxed-ok";
    let result = pe
        .execute(request_for(shell_action(&dir, script, None)))
        .await
        .expect("sandboxed execute");
    assert!(
        result.output.contains("sandbox sandboxed") || result.output.contains("sandbox partial"),
        "execute output must carry sandbox audit; got: {}",
        result.output
    );
    assert!(
        result.output.contains("m2-sandboxed-ok"),
        "stdout must be captured; got: {}",
        result.output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn execute_uses_overlay_working_directory() {
    let dir = std::env::temp_dir().join(format!("pe-m2-overlay-{}", std::process::id()));
    let overlay = dir.join("overlay_cwd");
    let _ = std::fs::create_dir_all(&overlay);
    let pe = ProcessExecutor::new(&dir, EnforcementLevel::Sandboxed);
    #[cfg(windows)]
    let script = "cd";
    #[cfg(not(windows))]
    let script = "pwd";
    let result = pe
        .execute(request_for(shell_action(&dir, script, Some(&overlay))))
        .await
        .expect("overlay execute");
    let expected = overlay
        .canonicalize()
        .unwrap_or_else(|_| overlay.clone())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .trim_end_matches(['\\', '/'])
        .to_string();
    let out_lower = result.output.to_lowercase();
    assert!(
        result.success && out_lower.contains(&expected.to_lowercase()),
        "execute cwd must be overlay {expected:?}; got: {}",
        result.output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn start_service_is_retired() {
    let dir = std::env::temp_dir().join(format!("pe-m2-svc-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let pe = ProcessExecutor::new(&dir, EnforcementLevel::Sandboxed);
    let action = shell_action(&dir, "echo unused", None);
    let req = AuthorizedServiceRequest {
        authorized_action: AuthorizedAction {
            capability: valid_capability(&action),
            action,
        },
    };
    let err = pe
        .start_service(req)
        .await
        .err()
        .expect("start_service must refuse");
    match err {
        ProcessBrokerError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("SandboxLspLauncher") || msg.contains("retired"),
                "got {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got {other}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
