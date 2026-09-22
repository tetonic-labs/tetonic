//! Versioned canonical parameter digests for exact capability binding (M2-1).

use std::collections::BTreeMap;

use crate::execution::{ActionKind, CanonicalActionParameters, ProposedAction};

pub const CANONICAL_SCHEMA_VERSION: u32 = 1;

/// Deterministic FNV-1a digest (hex) over canonical payload bytes.
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Compute the canonical digest for action parameters (schema v1).
pub fn compute_canonical_digest(kind: &ActionKind, params: &CanonicalActionParameters) -> String {
    let payload = canonical_payload(kind, params);
    fnv1a_hex(payload.as_bytes())
}

/// Fill `digest` on parameters and return the updated struct.
pub fn finalize_parameters(
    kind: &ActionKind,
    mut params: CanonicalActionParameters,
) -> CanonicalActionParameters {
    params.schema_version = CANONICAL_SCHEMA_VERSION;
    params.digest = compute_canonical_digest(kind, &params);
    params
}

/// Prepare a proposed action for broker evaluation (canonical digest stamped).
pub fn prepare_proposed_action(mut action: ProposedAction) -> ProposedAction {
    action.parameters = finalize_parameters(&action.kind, action.parameters);
    action
}

fn canonical_list(items: &[String]) -> String {
    let mut s = String::new();
    for item in items {
        s.push_str(&format!("{}:{}", item.len(), item));
    }
    s
}

fn canonical_parts(parts: &[String]) -> String {
    let mut s = String::new();
    for part in parts {
        s.push_str(&format!("{}:{}", part.len(), part));
    }
    s
}

fn canonical_payload(kind: &ActionKind, params: &CanonicalActionParameters) -> String {
    let mut parts: Vec<String> = vec![
        format!("schema={}", params.schema_version),
        format!("kind={kind:?}"),
    ];
    if let Some(exe) = &params.executable_identity {
        parts.push(format!("exe={exe}"));
    }
    if let Some(path) = &params.resolved_path {
        parts.push(format!("path={path}"));
    }
    if !params.arguments.is_empty() {
        parts.push(format!("argv={}", canonical_list(&params.arguments)));
    }
    if let Some(shell) = &params.shell_identity {
        parts.push(format!("shell={shell}"));
    }
    if let Some(mode) = &params.shell_mode {
        parts.push(format!("shell_mode={mode}"));
    }
    if let Some(script) = &params.script_bytes {
        parts.push(format!("script={}", fnv1a_hex(script)));
    }
    if let Some(wd) = &params.working_directory {
        parts.push(format!("wd={wd}"));
    }
    if let Some(env) = &params.env_vars {
        parts.push(format!("env={}", env_map_canonical(env)));
    }
    if let Some(class) = &params.process_class {
        parts.push(format!("process_class={class:?}"));
    }
    if let Some(scope) = &params.filesystem_access_scope {
        parts.push(format!("fs_scope={scope}"));
    }
    if let Some(net) = &params.network_policy {
        parts.push(format!("net={net}"));
    }
    if let Some(sandbox) = &params.sandbox_profile {
        parts.push(format!("sandbox={sandbox}"));
    }
    if let Some(tool) = &params.tool_arguments {
        parts.push(format!(
            "tool_args={}",
            serde_json::to_string(tool).unwrap_or_default()
        ));
    }
    canonical_parts(&parts)
}

fn env_map_canonical(env: &BTreeMap<String, String>) -> String {
    let mut s = String::new();
    for (k, v) in env {
        let entry = format!("{k}={v}");
        s.push_str(&format!("{}:{}", entry.len(), entry));
    }
    s
}

/// Validate an authorized action against capability metadata (pre-execution).
pub fn validate_authorized_action(
    authorized: &crate::execution::AuthorizedAction,
    now_secs: u64,
) -> Result<(), crate::sinks::CapabilityError> {
    use crate::sinks::CapabilityError;
    let cap = &authorized.capability;
    let act = &authorized.action;

    if cap.expiration < now_secs {
        return Err(CapabilityError::Expired);
    }
    if cap.revoked {
        return Err(CapabilityError::Revoked);
    }
    if cap.current_use_count >= cap.max_use_count {
        return Err(CapabilityError::AlreadyConsumed);
    }
    if cap.action_kind != act.kind {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.session_id != act.session_id {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.run_id.is_some() && cap.run_id != act.run_id {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.task_id.is_some() && cap.task_id != act.task_id {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.attempt_id.is_some() && cap.attempt_id != act.attempt_id {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.agent_id.is_some() && cap.agent_id != act.agent_id {
        return Err(CapabilityError::ScopeMismatch);
    }
    let expected = compute_canonical_digest(&act.kind, &act.parameters);
    if cap.canonical_parameter_digest != expected {
        return Err(CapabilityError::ScopeMismatch);
    }
    if cap.workspace_version != act.workspace_version {
        return Err(CapabilityError::WorkspaceVersionMismatch);
    }
    if cap.data_classification != act.data_class {
        return Err(CapabilityError::DataClassificationMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::DataClass;
    use crate::execution::ProcessClass;
    use crate::ids::{ActionId, SessionId};

    #[test]
    fn argv_boundary_changes_digest() {
        let base = CanonicalActionParameters {
            digest: String::new(),
            executable_identity: Some("echo".into()),
            resolved_path: None,
            arguments: vec!["a".into(), "b".into()],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: Some("/ws".into()),
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
        };
        let d1 = compute_canonical_digest(&ActionKind::ExecuteProcess, &base);
        let mut fused = base.clone();
        fused.arguments = vec!["a b".into()];
        let d2 = compute_canonical_digest(&ActionKind::ExecuteProcess, &fused);
        assert_ne!(d1, d2);
    }

    #[test]
    fn shell_script_bytes_affect_digest() {
        let mut p = CanonicalActionParameters {
            digest: String::new(),
            executable_identity: None,
            resolved_path: None,
            arguments: vec![],
            shell_identity: Some("sh".into()),
            shell_mode: None,
            script_bytes: Some(b"echo hi".to_vec()),
            working_directory: Some("/ws".into()),
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: Some(ProcessClass::ModelRequestedShell),
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: None,
        };
        let d1 = compute_canonical_digest(&ActionKind::ExecuteShell, &p);
        p.script_bytes = Some(b"echo bye".to_vec());
        let d2 = compute_canonical_digest(&ActionKind::ExecuteShell, &p);
        assert_ne!(d1, d2);
    }

    #[test]
    fn prepare_stamps_digest_on_action() {
        let action = ProposedAction {
            action_id: ActionId::new("a"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::ExecuteShell,
            parameters: CanonicalActionParameters {
                digest: String::new(),
                executable_identity: None,
                resolved_path: None,
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: Some(b"ls".to_vec()),
                working_directory: Some("/ws".into()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: Some(ProcessClass::ModelRequestedShell),
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: None,
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        let prepared = prepare_proposed_action(action);
        assert!(!prepared.parameters.digest.is_empty());
        assert_eq!(
            prepared.parameters.digest,
            compute_canonical_digest(&prepared.kind, &prepared.parameters)
        );
    }

    #[test]
    fn adversarial_argv_delimiters_produce_distinct_digests() {
        let mut base = CanonicalActionParameters {
            digest: String::new(),
            executable_identity: Some("echo".into()),
            resolved_path: None,
            arguments: vec!["first\u{1f}second".into()],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: Some("/ws".into()),
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
        };
        let d1 = compute_canonical_digest(&ActionKind::ExecuteProcess, &base);
        base.arguments = vec!["first".into(), "second".into()];
        let d2 = compute_canonical_digest(&ActionKind::ExecuteProcess, &base);
        assert_ne!(d1, d2);
    }
}
