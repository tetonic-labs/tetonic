//! Approval coordination (Slice 4) — application-level validation and persistence.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use crate::{commands::*, errors::AppError, events::*};
use tetonic_core::{ApprovalRequest, ConfinementWarning};
use tetonic_domain::execution::ProcessClass;
use tetonic_memory::RecoverMutex;

/// Returns true when a remembered rule auto-allows this request.
pub fn approval_rule_allows(
    store: &tetonic_memory::Store,
    req: &ApprovalRequest,
) -> Result<bool, AppError> {
    let detail = approval_detail(req);
    store
        .approval_rule_matches(&req.kind, &detail)
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))
}

pub fn approval_detail(req: &ApprovalRequest) -> String {
    if is_shell_approval(req) {
        if let Some(cmd) = req.args.get("command").and_then(|v| v.as_str()) {
            return cmd.to_string();
        }
    }
    if req.kind == "run_shell" {
        return req
            .args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }
    req.tool.clone()
}

pub fn is_shell_approval(req: &ApprovalRequest) -> bool {
    req.kind == "run_shell" || req.tool == "run_shell"
}

/// Attach predicted OS-confinement gaps so the prompt can show them (AUDIT H1-2).
pub fn attach_shell_confinement(req: &mut ApprovalRequest, workspace: &Path) {
    if !is_shell_approval(req) {
        return;
    }
    let preview =
        tetonic_sandbox::preview_confinement(ProcessClass::ModelRequestedShell, workspace);
    req.missing_controls = preview
        .missing_controls
        .iter()
        .map(|m| ConfinementWarning {
            control: m.wire_control(),
            risk_level: m.wire_risk(),
            reason: m.reason.clone(),
        })
        .collect();
    req.user_approval_required = preview.user_approval_required;
}

/// Human-readable confinement warning for CLI/TUI prompts.
pub fn format_confinement_prompt(
    missing: &[ConfinementWarning],
    user_approval_required: bool,
) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let mut lines = vec!["no OS confinement for:".to_string()];
    for c in missing {
        lines.push(format!("  [{}] {}: {}", c.risk_level, c.control, c.reason));
    }
    if user_approval_required {
        lines.push(
            "approval is the only barrier — this command can reach the network and files this user can."
                .into(),
        );
    }
    lines.join("\n")
}

/// High-risk missing OS controls override remembered rules, `--allow-shell`,
/// and automatic approval. A person can still accept the gap explicitly.
pub fn must_prompt_interactively(req: &ApprovalRequest) -> bool {
    req.user_approval_required
}

pub fn record_proposed_tool(
    store: &tetonic_memory::SharedStore,
    call_id: &str,
    session_id: &str,
    tool: &str,
    args_json: &str,
) -> Result<(), AppError> {
    store
        .write_sync({
            let call_id = call_id.to_string();
            let session_id = session_id.to_string();
            let tool = tool.to_string();
            let args_json = args_json.to_string();
            move |db| db.propose_tool_call(&call_id, &session_id, &tool, &args_json)
        })
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))
}

pub trait ApprovalService: Send + Sync {
    fn preapprove(&self, req: &ApprovalRequest) -> Result<Option<bool>, AppError>;
    fn register_request(
        &self,
        cmd: RegisterApprovalCommand,
    ) -> Result<oneshot::Receiver<bool>, AppError>;
    fn respond(&self, cmd: ApprovalResponseCommand) -> Result<bool, AppError>;
    fn fail_session_waits(&self, session_id: &str);
    fn fail_attempt_waits(&self, attempt_id: &str);
}

struct ParkedWait {
    tx: oneshot::Sender<bool>,
    session_id: String,
    attempt_id: Option<String>,
    kind: String,
    detail: String,
}

pub struct DefaultApprovalService {
    store: Option<tetonic_memory::SharedStore>,
    events: Arc<dyn ApplicationEventSink>,
    parked: Mutex<HashMap<String, ParkedWait>>,
    completed: Mutex<HashSet<String>>,
}

impl DefaultApprovalService {
    pub fn new(
        store: Option<tetonic_memory::SharedStore>,
        events: Arc<dyn ApplicationEventSink>,
    ) -> Self {
        Self {
            store,
            events,
            parked: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
        }
    }

    pub fn persist_decision(
        &self,
        approval_id: &str,
        session_id: &str,
        kind: &str,
        detail: &str,
        allowed: bool,
        remember: bool,
    ) -> Result<(), AppError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let decision = if allowed { "allow" } else { "deny" };
        store
            .write_sync({
                let approval_id = approval_id.to_string();
                let session_id = session_id.to_string();
                let kind = kind.to_string();
                let detail = detail.to_string();
                let decision = decision.to_string();
                move |db| {
                    db.record_approval(
                        &approval_id,
                        &session_id,
                        &kind,
                        &detail,
                        &decision,
                        remember,
                    )
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    if allowed && remember {
                        db.remember_session_approval_rule(&session_id, &kind, &detail)
                            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    }
                    Ok::<_, AppError>(())
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??;
        Ok(())
    }
}

impl ApprovalService for DefaultApprovalService {
    fn preapprove(&self, req: &ApprovalRequest) -> Result<Option<bool>, AppError> {
        if must_prompt_interactively(req) {
            return Ok(None);
        }
        if req.kind == "verify_finish" {
            return Ok(Some(true));
        }
        if let Some(store) = &self.store {
            let res = store
                .read_sync({
                    let req = req.clone();
                    move |db| approval_rule_allows(db, &req)
                })
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??;
            if res {
                return Ok(Some(true));
            }
        }
        Ok(None)
    }

    fn register_request(
        &self,
        cmd: RegisterApprovalCommand,
    ) -> Result<oneshot::Receiver<bool>, AppError> {
        if cmd.approval_id.is_empty() {
            return Err(AppError::InvalidRequest("approval_id required".into()));
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut parked = self.parked.lock_recover();
            if parked.contains_key(&cmd.approval_id)
                || self.completed.lock_recover().contains(&cmd.approval_id)
            {
                return Err(AppError::InvalidRequest(
                    "duplicate approval request".into(),
                ));
            }
            let args_json = serde_json::to_string(&cmd.args)
                .map_err(|e| AppError::InvalidRequest(format!("args json: {e}")))?;
            if let Some(store) = &self.store {
                record_proposed_tool(store, &cmd.call_id, &cmd.session_id, &cmd.tool, &args_json)?;
            }
            if cmd.auto_grant_approvals {
                // A skipped prompt may allow a confined command. It must not
                // allow a command the OS cannot keep off the network.
                let allowed = !cmd.user_approval_required;
                self.persist_decision(
                    &cmd.approval_id,
                    &cmd.session_id,
                    &cmd.kind,
                    &cmd.detail,
                    allowed,
                    false,
                )?;
                self.completed
                    .lock_recover()
                    .insert(cmd.approval_id.clone());
                let _ = tx.send(allowed);
            } else {
                parked.insert(
                    cmd.approval_id.clone(),
                    ParkedWait {
                        tx,
                        session_id: cmd.session_id.clone(),
                        attempt_id: cmd.attempt_id.clone(),
                        kind: cmd.kind.clone(),
                        detail: cmd.detail.clone(),
                    },
                );
            }
        }
        // Register before publishing so an immediate response cannot outrun the waiter.
        emit(
            &self.events,
            ApplicationEvent::ApprovalRequest {
                session_id: cmd.session_id,
                approval_id: cmd.approval_id,
                call_id: cmd.call_id,
                kind: cmd.kind,
                detail: cmd.detail,
                tool: cmd.tool,
                args: cmd.args,
                missing_controls: cmd.missing_controls,
                user_approval_required: cmd.user_approval_required,
                attempt_id: cmd.attempt_id,
            },
        );
        Ok(rx)
    }

    fn respond(&self, cmd: ApprovalResponseCommand) -> Result<bool, AppError> {
        if cmd.approval_id.is_empty() {
            return Err(AppError::InvalidRequest("approval_id required".into()));
        }
        let mut parked_lock = self.parked.lock_recover();
        let parked = parked_lock.get(&cmd.approval_id).ok_or_else(|| {
            let reason = if self.completed.lock_recover().contains(&cmd.approval_id) {
                "duplicate or stale approval response"
            } else {
                "stale or unknown approval request"
            };
            AppError::InvalidRequest(format!("{reason} '{}'", cmd.approval_id))
        })?;
        if cmd.session_id != parked.session_id {
            return Err(AppError::InvalidRequest("approval session mismatch".into()));
        }
        // Legacy transports may omit the Attempt ID. Resolve it from the registered
        // request, never from response metadata; explicit IDs must match exactly.
        if cmd.attempt_id.is_some() && cmd.attempt_id != parked.attempt_id {
            return Err(AppError::InvalidRequest("approval attempt mismatch".into()));
        }
        // Persist trusted request metadata before consuming the waiter. A failed write
        // leaves the original request pending and allows a real retry.
        self.persist_decision(
            &cmd.approval_id,
            &parked.session_id,
            &parked.kind,
            &parked.detail,
            cmd.approved,
            cmd.remember,
        )?;
        let parked = parked_lock
            .remove(&cmd.approval_id)
            .expect("validated parked request");
        self.completed.lock_recover().insert(cmd.approval_id);
        Ok(parked.tx.send(cmd.approved).is_ok())
    }

    fn fail_session_waits(&self, session_id: &str) {
        let mut parked = self.parked.lock_recover();
        let ids: Vec<String> = parked
            .iter()
            .filter(|(_, w)| w.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(w) = parked.remove(&id) {
                let _ = w.tx.send(false);
                self.completed.lock_recover().insert(id);
            }
        }
    }

    fn fail_attempt_waits(&self, attempt_id: &str) {
        let mut parked = self.parked.lock_recover();
        let ids: Vec<String> = parked
            .iter()
            .filter(|(_, w)| w.attempt_id.as_deref() == Some(attempt_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(w) = parked.remove(&id) {
                let _ = w.tx.send(false);
                self.completed.lock_recover().insert(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_core::ApprovalRequest;

    fn shell_req(user_approval_required: bool) -> ApprovalRequest {
        ApprovalRequest {
            call_id: "tc_1".into(),
            kind: "run_shell".into(),
            tool: "run_shell".into(),
            args: serde_json::json!({ "command": "npm test" }),
            missing_controls: vec![ConfinementWarning {
                control: "network_denial".into(),
                risk_level: "high".into(),
                reason: "OS-level network filtering not available; broker-only policy".into(),
            }],
            user_approval_required,
            ..Default::default()
        }
    }

    #[test]
    fn attach_shell_confinement_sets_high_risk_network_gap() {
        let mut req = ApprovalRequest {
            kind: "run_shell".into(),
            tool: "run_shell".into(),
            args: serde_json::json!({ "command": "echo hi" }),
            ..Default::default()
        };
        attach_shell_confinement(&mut req, Path::new("."));
        let caps = tetonic_sandbox::platform_backend().capabilities();
        if !caps.network_denial {
            assert!(
                req.missing_controls
                    .iter()
                    .any(|c| c.control == "network_denial" && c.risk_level == "high"),
                "preview must list network_denial as high: {:?}",
                req.missing_controls
            );
            assert!(req.user_approval_required);
        } else {
            assert!(
                req.missing_controls
                    .iter()
                    .all(|c| c.control != "network_denial"),
                "preview must not list network_denial when available: {:?}",
                req.missing_controls
            );
        }
    }

    #[test]
    fn high_risk_gap_skips_remembered_rule() {
        let req = shell_req(true);
        assert!(must_prompt_interactively(&req));
        let (events, _) = crate::events::RecordingEventSink::new();
        let svc = DefaultApprovalService::new(None, events);
        assert_eq!(svc.preapprove(&req).unwrap(), None);
    }

    #[test]
    fn auto_grant_does_not_approve_an_unconfined_shell() {
        let (events, _) = crate::events::RecordingEventSink::new();
        let svc = DefaultApprovalService::new(None, events);
        let rx = svc
            .register_request(crate::commands::RegisterApprovalCommand {
                session_id: "session".into(),
                approval_id: "approval".into(),
                call_id: "call".into(),
                kind: "run_shell".into(),
                detail: "echo hi".into(),
                tool: "run_shell".into(),
                args: serde_json::json!({ "command": "echo hi" }),
                missing_controls: vec![],
                user_approval_required: true,
                auto_grant_approvals: true,
                attempt_id: None,
            })
            .unwrap();
        assert_eq!(rx.blocking_recv().unwrap(), false);
    }

    #[test]
    fn auto_grant_still_approves_a_confined_shell() {
        let (events, _) = crate::events::RecordingEventSink::new();
        let svc = DefaultApprovalService::new(None, events);
        let rx = svc
            .register_request(crate::commands::RegisterApprovalCommand {
                session_id: "session".into(),
                approval_id: "approval-ok".into(),
                call_id: "call".into(),
                kind: "run_shell".into(),
                detail: "echo hi".into(),
                tool: "run_shell".into(),
                args: serde_json::json!({ "command": "echo hi" }),
                missing_controls: vec![],
                user_approval_required: false,
                auto_grant_approvals: true,
                attempt_id: None,
            })
            .unwrap();
        assert_eq!(rx.blocking_recv().unwrap(), true);
    }

    #[test]
    fn without_required_flag_verify_finish_still_auto_allows() {
        let req = ApprovalRequest {
            kind: "verify_finish".into(),
            tool: "verify".into(),
            ..Default::default()
        };
        let (events, _) = crate::events::RecordingEventSink::new();
        let svc = DefaultApprovalService::new(None, events);
        assert_eq!(svc.preapprove(&req).unwrap(), Some(true));
    }
}
