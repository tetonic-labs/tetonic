//! CodingProcessValidator and coding ProcessExecutor tests (C1).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lokai_domain::execution::{
    AuthorizedAction, CanonicalActionParameters, ExecutionOutcome, IssuedCapability, ProposedAction,
};
use lokai_domain::{ActionId, ActionKind, AgentId, DataClass, ProcessSink, SessionId};
use lokai_tools::process_executor::{
    coding_executor, CodingProcessValidator, EnforcementLevel, ProcessExecutor,
};

fn temp_ws(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pe-coding-{prefix}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn build_authorized_action(
    dir: &Path,
    action_id: &str,
    kind: ActionKind,
    executable: Option<String>,
    args: Vec<String>,
    workspace_version: Option<lokai_domain::WorkspaceVersion>,
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
        parameters: lokai_domain::finalize_parameters(
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
            capability_id: lokai_domain::CapabilityId::new(format!("cap_{action_id}")),
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

/// C1: Coding validator still denies execution when live workspace version changes (stale version).
#[tokio::test]
async fn coding_stale_version_still_denied() {
    let root = temp_ws("stale");
    let file = root.join("hello.txt");
    std::fs::write(&file, "initial content").unwrap();

    // Capture initial live workspace version
    let initial_wv = lokai_transaction::version::capture_workspace_version(&root, &[]).unwrap();

    // Create coding executor bound to root
    let pe = coding_executor(&root, EnforcementLevel::Constrained);

    // Build action with initial version
    let auth = build_authorized_action(
        &root,
        "stale_check",
        ActionKind::ExecuteProcess,
        Some(if cfg!(windows) {
            "cmd".into()
        } else {
            "sh".into()
        }),
        if cfg!(windows) {
            vec!["/C".into(), "echo ok".into()]
        } else {
            vec!["-c".into(), "echo ok".into()]
        },
        Some(initial_wv),
        Some(root.display().to_string()),
    );

    // Mutate file on disk to make captured version stale
    std::fs::write(&file, "modified content making version stale").unwrap();

    // Execute through process sink
    let outcome = pe
        .run_process(&auth, &lokai_domain::work_scope::WorkScope::default())
        .await;
    match outcome {
        ExecutionOutcome::Failed { reason, .. } => {
            assert!(
                reason.contains("workspace version") || reason.contains("mismatch"),
                "stale version must be denied by coding validator; got: {reason}"
            );
        }
        other => {
            panic!("expected stale version denial, got: {other:?}");
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// C1: Permissible staged overlay cwd is preserved and accepted by coding validator.
#[tokio::test]
async fn coding_overlay_cwd_preserved() {
    let root = temp_ws("overlay-root");
    let overlay = root.join("staged_overlay");
    std::fs::create_dir_all(&overlay).unwrap();
    let file = root.join("file.txt");
    std::fs::write(&file, "baseline").unwrap();

    let wv = lokai_transaction::version::capture_workspace_version(&root, &[]).unwrap();

    let validator = Arc::new(CodingProcessValidator::new(&root).with_overlay(&overlay));
    let pe = ProcessExecutor::new(&overlay, EnforcementLevel::Constrained, validator);

    let auth = build_authorized_action(
        &overlay,
        "overlay_check",
        ActionKind::ExecuteProcess,
        Some(if cfg!(windows) {
            "cmd".into()
        } else {
            "sh".into()
        }),
        if cfg!(windows) {
            vec!["/C".into(), "echo overlay_execution_marker".into()]
        } else {
            vec!["-c".into(), "echo overlay_execution_marker".into()]
        },
        Some(wv),
        Some(overlay.display().to_string()),
    );

    let outcome = pe
        .run_process(&auth, &lokai_domain::work_scope::WorkScope::default())
        .await;
    match outcome {
        ExecutionOutcome::Completed { ok, summary, .. } => {
            assert!(ok, "overlay execution must succeed; summary: {summary}");
            assert!(
                summary.contains("overlay_execution_marker"),
                "summary must contain real marker; got: {summary}"
            );
        }
        ExecutionOutcome::Failed { reason, .. } => {
            panic!("permissible overlay execution unexpectedly failed: {reason}");
        }
        other => {
            panic!("unexpected outcome: {other:?}");
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}
