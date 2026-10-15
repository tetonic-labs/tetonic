use super::*;
use serde_json::json;
use tetonic_domain::{ActionKind, AuthorizedAction, ToolHost};

fn authorize(tools: &Tools, args: &Value) -> AuthorizedAction {
    authorize_named(tools, "run_shell", args)
}

fn authorize_named(tools: &Tools, name: &str, args: &Value) -> AuthorizedAction {
    let parameters = tools.prepare_parameters(name, serde_json::from_value(json!({
        "digest":"", "arguments":[], "schema_version":1, "tool_name":name, "tool_arguments":args,
        "working_directory":tools.ws.root(), "script_bytes":args["command"].as_str().unwrap().as_bytes(),
        "process_class":"ModelRequestedShell"
    })).unwrap()).unwrap();
    let action = tetonic_domain::prepare_proposed_action(tetonic_domain::ProposedAction {
        action_id: tetonic_domain::ActionId::new("check"),
        session_id: tetonic_domain::SessionId::new("session"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: None,
        workspace_version: Some(
            tetonic_transaction::version::capture_workspace_version(tools.ws.root(), &[]).unwrap(),
        ),
        data_class: tetonic_domain::DataClass::RepositorySource,
        kind: ActionKind::ExecuteShell,
        parameters,
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    });
    AuthorizedAction {
        capability: tetonic_domain::IssuedCapability {
            capability_id: tetonic_domain::CapabilityId::new("cap"),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            action_kind: action.kind.clone(),
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class,
            issuance_timestamp: 0,
            expiration: u64::MAX,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "test".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    }
}

fn stage(tools: &Tools, value: &str) {
    let result = tools.execute("write_file", &json!({"path":"result.txt","content":value}));
    assert!(result.ok, "{result:?}");
}

#[test]
fn shell_checks_pending_bytes_and_commit_waits_for_repair_and_recheck() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "original\n").unwrap();
    let mut tools = Tools::new(Workspace::new(dir.path()).unwrap(), true);
    tools.executor = tools.executor.with_level(EnforcementLevel::Constrained);
    let command = if cfg!(windows) {
        "findstr ready result.txt"
    } else {
        "grep -x ready result.txt"
    };
    let args = json!({"command":command,"view":"staged"});
    stage(&tools, "broken\n");
    let result = tools.execute_authorized("run_shell", &args, Some(&authorize(&tools, &args)));
    assert!(!result.ok, "{result:?}");
    assert_eq!(result.error_kind.as_deref(), Some("verification_failed"));
    assert!(tools.commit_staged_if_any().is_err());
    stage(&tools, "ready\n");
    let approved = authorize(&tools, &args);
    assert!(staged_shell_directory(tools.ws.root(), &approved.action.parameters).is_some());
    let result = tools.execute_authorized("run_shell", &args, Some(&approved));
    assert!(result.ok, "{result:?}");
    assert!(result.content.contains("ready"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "original\n"
    );
    assert!(tools
        .commit_staged_if_any()
        .unwrap()
        .unwrap()
        .artifact
        .verification_artifact_id
        .is_some());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "ready\n"
    );
}

#[test]
fn staged_shell_rejects_unapproved_changed_revoked_canceled_and_stale_actions() {
    let dir = tempfile::tempdir().unwrap();
    let mut tools = Tools::new(Workspace::new(dir.path()).unwrap(), true);
    tools.executor = tools.executor.with_level(EnforcementLevel::Constrained);
    stage(&tools, "draft");
    let args = json!({"command":"echo should-not-run>sentinel.txt","view":"staged"});
    let approved = authorize(&tools, &args);
    assert!(!tools.execute("run_shell", &args).ok);
    let mut revoked = approved.clone();
    revoked.capability.revoked = true;
    assert!(
        !tools
            .execute_authorized("run_shell", &args, Some(&revoked))
            .ok
    );
    let mut changed = approved.clone();
    changed.action.parameters.working_directory = Some(dir.path().display().to_string());
    assert!(
        !tools
            .execute_authorized("run_shell", &args, Some(&changed))
            .ok
    );
    assert!(
        !tools
            .execute_authorized(
                "run_shell",
                &json!({"command":"echo different","view":"staged"}),
                Some(&approved)
            )
            .ok
    );
    let scope = tetonic_domain::work_scope::WorkScope::default();
    scope.cancel();
    assert!(
        !tools
            .execute_authorized_cancellable(
                "run_shell",
                &args,
                Some(&approved),
                Some(&scope.cancellation_signal())
            )
            .ok
    );
    stage(&tools, "new draft");
    let stale = tools.execute_authorized("run_shell", &args, Some(&approved));
    assert!(
        !stale.ok && stale.content.contains("changed after approval"),
        "{stale:?}"
    );
    assert!(!dir.path().join("sentinel.txt").exists());
    assert!(
        !staged_shell_directory(tools.ws.root(), &approved.action.parameters)
            .unwrap()
            .exists(),
        "no process or copy before valid execution"
    );
}

#[test]
fn run_checks_preserves_shell_approval_identity_and_checks_the_exact_pending_patch() {
    let dir = tempfile::tempdir().unwrap();
    let mut tools = Tools::new(Workspace::new(dir.path()).unwrap(), true).with_allowed_tools(
        ["write_file", "run_checks", "inspect_changes"]
            .into_iter()
            .map(String::from)
            .collect(),
    );
    tools.executor = tools.executor.with_level(EnforcementLevel::Constrained);
    let command = if cfg!(windows) {
        "findstr ready result.txt"
    } else {
        "grep -x ready result.txt"
    };
    let args = json!({"command":command});
    stage(&tools, "broken\n");
    assert!(
        !tools.execute("run_checks", &args).ok,
        "requires an issued approval"
    );
    assert!(tools.requires_user_approval("run_checks"));
    assert_eq!(
        tools.propose("run_checks", &args).unwrap().kind,
        ActionKind::ExecuteShell
    );
    let approved = authorize_named(&tools, "run_checks", &args);
    assert!(staged_shell_directory(tools.ws.root(), &approved.action.parameters).is_some());
    let result = tools.execute_authorized("run_checks", &args, Some(&approved));
    assert!(!result.ok, "{result:?}");
    assert!(tools.commit_staged_if_any().is_err());
    assert!(tools
        .execute("inspect_changes", &json!({}))
        .content
        .contains("check failed"));
    stage(&tools, "ready\n");
    let approved = authorize_named(&tools, "run_checks", &args);
    let mut revoked = approved.clone();
    revoked.capability.revoked = true;
    assert!(
        !tools
            .execute_authorized("run_checks", &args, Some(&revoked))
            .ok
    );
    let other = authorize(&tools, &json!({"command":command,"view":"staged"}));
    assert!(
        !tools
            .execute_authorized("run_checks", &args, Some(&other))
            .ok,
        "shell receipt cannot authorize another tool"
    );
    assert!(
        !tools
            .execute_authorized(
                "run_checks",
                &json!({"command":"echo changed"}),
                Some(&approved)
            )
            .ok
    );
    let scope = tetonic_domain::work_scope::WorkScope::default();
    scope.cancel();
    assert!(
        !tools
            .execute_authorized_cancellable(
                "run_checks",
                &args,
                Some(&approved),
                Some(&scope.cancellation_signal())
            )
            .ok
    );
    let result = tools.execute_authorized("run_checks", &args, Some(&approved));
    assert!(result.ok, "{result:?}");
    assert!(tools
        .execute("inspect_changes", &json!({}))
        .content
        .contains("check passed"));
    assert!(!dir.path().join("result.txt").exists());
    stage(&tools, "ready again\n");
    assert!(tools
        .execute("inspect_changes", &json!({}))
        .content
        .contains("needs a new passing check"));
    assert!(tools.commit_staged_if_any().is_err());
    assert!(
        !tools
            .execute_authorized("run_checks", &args, Some(&approved))
            .ok,
        "stale patch receipt"
    );
    let result = tools.execute_authorized(
        "run_checks",
        &args,
        Some(&authorize_named(&tools, "run_checks", &args)),
    );
    assert!(result.ok, "{result:?}");
    assert!(tools
        .commit_staged_if_any()
        .unwrap()
        .unwrap()
        .artifact
        .verification_artifact_id
        .is_some());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "ready again\n"
    );
}
