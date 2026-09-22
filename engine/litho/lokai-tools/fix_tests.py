import os

path = r'crates\lokai-tools\src\integration_tests.rs'
with open(path, 'r', encoding='utf-8') as f:
    lines = f.read()

old_action = '''    let action = ProposedAction {
        action_id: ActionId::new("mut_auth"),
        kind: ActionKind::RunTool {
            name: "write_file".into(),
            args: json!({ "path": "out.txt", "content": "via sink" }),
        },
        context: AuthorizationContext {
            session_id: "s".into(),
            agent_id: "a0".into(),
            workspace_root: dir.display().to_string(),
            data_class: DataClass::RepositorySource,
            turn_id: None,
        },
    };'''

new_action = '''    use lokai_domain::{SessionId, AgentId};
    use lokai_domain::execution::CanonicalActionParameters;
    let action = ProposedAction {
        action_id: ActionId::new("mut_auth"),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        kind: ActionKind::ExecuteProcess,
        parameters: CanonicalActionParameters {
            digest: "digest".into(),
            executable_identity: Some("write_file".into()),
            resolved_path: None,
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: None,
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
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };'''

old_cap = '''    let authorized = lokai_domain::AuthorizedAction {
        capability: IssuedCapability {
            capability_id: lokai_domain::CapabilityId::new("cap_auth"),
            action_id: action.action_id.clone(),
            scope: CapabilityScope::RunTool {
                name: "write_file".into(),
            },
        },
        action,
    };'''

new_cap = '''    let authorized = lokai_domain::AuthorizedAction {
        capability: IssuedCapability {
            capability_id: lokai_domain::CapabilityId::new("cap_auth"),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            agent_id: action.agent_id.clone(),
            action_kind: ActionKind::ExecuteProcess,
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class.clone(),
            issuance_timestamp: 0,
            expiration: 0,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v1".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    };'''

lines = lines.replace(old_action, new_action)
lines = lines.replace(old_cap, new_cap)
lines = lines.replace('ActionId, ActionKind, AuthorizationContext, CapabilityScope, DataClass, IssuedCapability', 'ActionId, ActionKind, DataClass, IssuedCapability')

with open(path, 'w', encoding='utf-8') as f:
    f.write(lines)
