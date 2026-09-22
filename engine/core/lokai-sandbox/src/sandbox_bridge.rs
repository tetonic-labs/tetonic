//! Bridge authorized actions to sandbox requests.

use std::path::Path;
use std::time::Duration;

use lokai_domain::execution::{AuthorizedAction, ProcessClass};
use lokai_domain::ActionKind;

use crate::{apply_executable, apply_shell, profile_for_class, SandboxRequest};

#[cfg(windows)]
fn default_shell() -> &'static str {
    "cmd"
}

#[cfg(not(windows))]
fn default_shell() -> &'static str {
    "sh"
}

pub fn sandbox_request_from_authorized(
    authorized: &AuthorizedAction,
    workspace: &Path,
    runtime_limit: Duration,
) -> Result<SandboxRequest, String> {
    let class = authorized
        .action
        .parameters
        .process_class
        .clone()
        .unwrap_or_else(infer_class);
    let mut req = profile_for_class(class, workspace);
    req.runtime_limit = runtime_limit;
    req.trace_context = authorized.action.trace_context.clone();
    req.workspace_version = authorized.action.workspace_version.clone();

    match &authorized.action.kind {
        ActionKind::ExecuteShell => {
            let script = authorized
                .action
                .parameters
                .script_bytes
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or("");
            let shell = authorized
                .action
                .parameters
                .shell_identity
                .clone()
                .unwrap_or_else(|| default_shell().to_string());
            req = apply_shell(req, &shell, script);
        }
        ActionKind::ExecuteProcess
        | ActionKind::GitOperation
        | ActionKind::StartInternalService => {
            let exe = authorized
                .action
                .parameters
                .executable_identity
                .clone()
                .unwrap_or_default();
            if exe.is_empty() {
                return Err("empty executable identity".into());
            }
            req = apply_executable(req, &exe, &authorized.action.parameters.arguments);
        }
        other => return Err(format!("unsupported sandbox action kind: {other:?}")),
    }
    Ok(req)
}

fn infer_class() -> ProcessClass {
    ProcessClass::RepositoryTool
}

pub fn process_class_for_verify() -> ProcessClass {
    ProcessClass::BuildVerification
}

pub fn process_class_for_shell() -> ProcessClass {
    ProcessClass::ModelRequestedShell
}

pub fn process_class_for_git() -> ProcessClass {
    ProcessClass::RepositoryTool
}

pub fn process_class_for_lsp() -> ProcessClass {
    ProcessClass::InternalService
}

pub fn format_sandbox_audit(report: &crate::SandboxRunReport) -> String {
    match &report.outcome {
        crate::IsolationOutcome::Sandboxed { .. } => {
            format!("sandbox sandboxed {}", report.capabilities.platform)
        }
        crate::IsolationOutcome::BrokeredWithWarning {
            missing_controls,
            user_approval_required,
            ..
        } => {
            let missing: Vec<_> = missing_controls.iter().map(|m| m.wire_control()).collect();
            format!(
                "sandbox partial {} missing={} approval_required={}",
                report.capabilities.platform,
                missing.join(","),
                user_approval_required
            )
        }
        crate::IsolationOutcome::Denied { reason } => {
            format!("sandbox denied {}", reason.replace('\n', " "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        platform_backend, IsolationOutcome, MissingControl, RiskLevel, SandboxControl,
        SandboxRunReport,
    };

    #[test]
    fn brokered_warning_audit_is_one_short_line() {
        let report = SandboxRunReport {
            outcome: IsolationOutcome::BrokeredWithWarning {
                enforced_controls: vec![SandboxControl::ProcessTreeContainment],
                missing_controls: vec![MissingControl {
                    control: SandboxControl::NetworkDenial,
                    reason: "OS-level network filtering not available; broker-only policy".into(),
                    risk_level: RiskLevel::High,
                }],
                user_approval_required: true,
            },
            capabilities: platform_backend().capabilities(),
            user_message: "Process ran with partial containment".into(),
            audit_summary: r#"{"capabilities":{"mechanisms":["Job Objects"]}}"#.into(),
        };
        let line = format_sandbox_audit(&report);
        assert!(line.len() < 200, "audit line too long: {line}");
        assert!(!line.contains('{'), "must not dump Debug/JSON: {line}");
        assert!(line.contains("partial"));
        assert!(line.contains("network_denial"));
        assert!(line.contains("approval_required=true"));
    }
}
