//! Maps canonical application events to JSON-RPC v1 notifications.

use super::types::{CAPACITY_AGENT, CAPACITY_SESSION};
use serde_json::{json, Value};
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::ROOT_AGENT;
use tetonic_rpc::protocol::events;
use tetonic_rpc::Notifier;

pub struct DaemonEventSink {
    notifier: Notifier,
}

impl DaemonEventSink {
    pub fn new(notifier: Notifier) -> Self {
        Self { notifier }
    }
}

impl ApplicationEventSink for DaemonEventSink {
    fn send(&self, event: ApplicationEvent) {
        match event {
            ApplicationEvent::SessionStarted { session_id } => {
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::LOG,
                    json!({
                        "level": "info",
                        "message": "session started",
                    }),
                );
            }
            ApplicationEvent::SessionEnded { session_id, status } => {
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::LOG,
                    json!({
                        "level": "info",
                        "message": format!("session ended ({status})"),
                    }),
                );
            }
            ApplicationEvent::RunStatus {
                run_id,
                status,
                agent_id,
                error,
                ..
            } => {
                let agent = agent_id.as_deref().unwrap_or(ROOT_AGENT);
                let mut payload = json!({ "status": status });
                if let Some(err) = error {
                    payload["error"] = json!(err);
                }
                self.notifier
                    .notify(&run_id, agent, events::RUN_STATUS, payload);
            }
            ApplicationEvent::TurnCompleted {
                session_id,
                status,
                error,
                ..
            } => {
                let mut payload = json!({ "status": status });
                if let Some(err) = error {
                    payload["error"] = json!(err);
                }
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::LOG,
                    json!({
                        "level": "info",
                        "message": format!("turn completed: {payload}"),
                    }),
                );
            }
            ApplicationEvent::Cancellation { session_id, .. } => {
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::LOG,
                    json!({
                        "level": "info",
                        "message": "run canceled",
                    }),
                );
            }
            ApplicationEvent::ApprovalRequest {
                session_id,
                approval_id,
                call_id,
                kind,
                detail,
                tool,
                args: _,
                missing_controls,
                user_approval_required,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::TOOL_CALL,
                    json!({
                        "tool_call_id": call_id,
                        "tool": tool,
                        "phase": "proposed",
                    }),
                );
                let missing: Vec<Value> = missing_controls
                    .iter()
                    .map(|c| {
                        json!({
                            "control": c.control,
                            "risk_level": c.risk_level,
                            "reason": c.reason,
                        })
                    })
                    .collect();
                self.notifier.notify(
                    &session_id,
                    ROOT_AGENT,
                    events::APPROVAL_REQUEST,
                    json!({
                        "approval_id": approval_id,
                        "kind": kind,
                        "detail": detail,
                        "tool_call_id": call_id,
                        "missing_controls": missing,
                        "user_approval_required": user_approval_required,
                    }),
                );
            }
            ApplicationEvent::ModelToken {
                session_id,
                agent_id,
                token,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    events::TOKEN,
                    json!({ "delta": token, "role": "assistant" }),
                );
            }
            ApplicationEvent::ContextSnapshot {
                session_id,
                agent_id,
                system_tokens,
                tools_tokens,
                conversation_tokens,
                total_tokens,
                budget,
                dropped_messages,
                estimated,
                data_class,
            } => {
                let mut payload = json!({
                    "system_tokens": system_tokens,
                    "tools_tokens": tools_tokens,
                    "conversation_tokens": conversation_tokens,
                    "total_tokens": total_tokens,
                    "budget": budget,
                    "dropped_messages": dropped_messages,
                    "estimated": estimated,
                });
                if let Some(class) = data_class {
                    payload["data_class"] = json!(tetonic_app::data_class_name(class));
                }
                self.notifier
                    .notify(&session_id, &agent_id, events::CONTEXT, payload);
            }
            ApplicationEvent::DispatchPlacement {
                session_id,
                agent_id,
                target,
                decision,
                reason_code,
                reason,
                data_class,
                redacted,
            } => {
                let mut payload = json!({
                    "target": target,
                    "decision": decision,
                    "redacted": redacted,
                });
                if let Some(code) = reason_code {
                    payload["reason_code"] = json!(code);
                }
                if let Some(r) = reason {
                    payload["reason"] = json!(r);
                }
                if let Some(class) = data_class {
                    payload["data_class"] = json!(tetonic_app::data_class_name(class));
                }
                self.notifier
                    .notify(&session_id, &agent_id, events::DISPATCH_PLACEMENT, payload);
            }
            ApplicationEvent::StageTransition {
                session_id,
                agent_id,
                stage,
                detail,
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    "event/stage",
                    json!({ "stage": stage, "detail": detail }),
                );
            }
            ApplicationEvent::ThoughtToken {
                session_id,
                agent_id,
                token,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    "event/thought",
                    json!({ "delta": token }),
                );
            }
            ApplicationEvent::NodeStarted { meta } => {
                self.notifier.notify(
                    &meta.session_id,
                    &meta.node_id,
                    "event/node_started",
                    json!({ "meta": meta }),
                );
            }
            ApplicationEvent::NodeProgress { node_id, state } => {
                self.notifier.notify(
                    "",
                    &node_id,
                    "event/node_progress",
                    json!({ "node_id": node_id, "state": state }),
                );
            }
            ApplicationEvent::NodeCompleted {
                node_id,
                state,
                duration_ms,
            } => {
                self.notifier.notify(
                    "",
                    &node_id,
                    "event/node_completed",
                    json!({ "node_id": node_id, "state": state, "duration_ms": duration_ms }),
                );
            }
            ApplicationEvent::TurnAnswer {
                session_id,
                agent_id,
                text,
                from_finish,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    "event/answer",
                    json!({ "text": text, "from_finish": from_finish }),
                );
            }
            ApplicationEvent::ToolCall {
                session_id,
                agent_id,
                call_id,
                tool,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    events::TOOL_CALL,
                    json!({
                        "tool_call_id": call_id,
                        "tool": tool,
                        "phase": "started",
                    }),
                );
            }
            ApplicationEvent::ToolResult {
                session_id,
                agent_id,
                call_id,
                ok,
                summary,
                error_kind,
                ..
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    events::TOOL_RESULT,
                    json!({
                        "tool_call_id": call_id,
                        "ok": ok,
                        "summary": summary,
                        "error_kind": error_kind,
                    }),
                );
            }
            ApplicationEvent::CapacityProgress {
                job_id,
                progress_pct,
                message,
            } => {
                self.notifier.notify(
                    CAPACITY_SESSION,
                    CAPACITY_AGENT,
                    events::CAPACITY_PROGRESS,
                    json!({
                        "job_id": job_id,
                        "phase": "queued",
                        "percent": progress_pct,
                        "message": message,
                    }),
                );
            }
            ApplicationEvent::PolicyUpdated { mode } => {
                tracing::info!(?mode, "policy updated via application service");
            }
            ApplicationEvent::LogDiagnostic {
                session_id: Some(session_id),
                agent_id: Some(agent_id),
                message,
            } => {
                self.notifier.notify(
                    &session_id,
                    &agent_id,
                    events::LOG,
                    json!({
                        "level": "info",
                        "message": message,
                    }),
                );
            }
            ApplicationEvent::LogDiagnostic { message, .. } => {
                tracing::info!(%message, "application event");
            }
            ApplicationEvent::ContextInformation { info } => {
                tracing::debug!(%info, "context information");
            }
            other => {
                tracing::debug!("application event: {:?}", other);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_rpc::outbound::OutboundQueue;
    use tetonic_rpc::Notifier;

    fn frames_for(event: ApplicationEvent) -> String {
        let (queue, _wake) = OutboundQueue::new(8);
        let sink = DaemonEventSink::new(Notifier::new(queue.clone()));
        sink.send(event);
        queue.flush_pending();
        queue.drain_ready().join("\n")
    }

    #[test]
    fn tool_call_notification_does_not_repeat_argument_bodies() {
        let shown = frames_for(ApplicationEvent::ToolCall {
            session_id: "sess".into(),
            agent_id: "a0".into(),
            call_id: "call-1".into(),
            tool: "write_file".into(),
            args: serde_json::json!({
                "path": "notes.txt",
                "content": "PRIVATECANARY file body"
            }),
            parent_agent_id: None,
            run_id: None,
            task_id: None,
            attempt_id: None,
            identity_id: None,
        });
        assert!(shown.contains("write_file"), "{shown}");
        assert!(shown.contains("call-1"), "{shown}");
        assert!(!shown.contains("PRIVATECANARY"), "{shown}");
        assert!(!shown.contains("notes.txt"), "{shown}");
    }
}
