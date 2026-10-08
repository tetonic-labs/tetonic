//! Redacted agent observations. These events cannot establish execution outcomes.
use crate::events::{emit, ApplicationEvent, ApplicationEventSink};
use crate::redaction_audit::{MissingStoreRedactionSink, StoreRedactionSink};
use std::sync::Arc;
use tetonic_core::Step;
use tetonic_domain::secrets::{OutboundRedaction, OutboundRedactionSink};
use tetonic_secrets::ScannerEngine;

fn redact_step_text(
    scanner: Option<&ScannerEngine>,
    sink: Option<&dyn OutboundRedactionSink>,
    session_id: &str,
    role: &str,
    text: String,
) -> Option<String> {
    let scanner = scanner?;
    match tetonic_secrets::redact_text_sync(scanner, &text) {
        Ok((out, hit)) => {
            if !hit {
                return Some(text);
            }
            if let Some(sink) = sink {
                let omitted = out.is_empty();
                let _ = sink.record(&OutboundRedaction {
                    session_id: Some(session_id.to_string()),
                    model: String::new(),
                    role: role.to_string(),
                    message_index: 0,
                    omitted,
                    records: Vec::new(),
                });
            }
            Some(if out.is_empty() {
                "[omitted — secret material withheld]".into()
            } else {
                out
            })
        }
        Err(_) => Some(tetonic_secrets::SCAN_FAILED_PLACEHOLDER.to_string()),
    }
}

fn redact_step_json(
    scanner: Option<&ScannerEngine>,
    value: serde_json::Value,
) -> Option<serde_json::Value> {
    let scanner = scanner?;
    match tetonic_secrets::redact_json_value(scanner, &value) {
        Ok((out, _)) => Some(out),
        Err(_) => Some(serde_json::Value::String(
            tetonic_secrets::SCAN_FAILED_PLACEHOLDER.to_string(),
        )),
    }
}

pub fn step_to_events(
    events: &Arc<dyn ApplicationEventSink>,
    session_id: &str,
    agent_id: &str,
    step: Step,
    scanner: Option<&ScannerEngine>,
    sink: Option<&dyn OutboundRedactionSink>,
    envelope: Option<&crate::events::EventEnvelope>,
) {
    let run_id = envelope.and_then(|e| e.run_id.clone());
    let task_id = envelope.and_then(|e| e.task_id.clone());
    let attempt_id = envelope.and_then(|e| e.attempt_id.clone());
    let identity_id = envelope.and_then(|e| e.identity_id.clone());
    match step {
        Step::Token(token) => {
            let Some(token) = redact_step_text(scanner, sink, session_id, "token", token) else {
                return;
            };
            emit(
                events,
                ApplicationEvent::ModelToken {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    token,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Thought(token) => {
            let Some(token) = redact_step_text(scanner, sink, session_id, "thought", token) else {
                return;
            };
            emit(
                events,
                ApplicationEvent::ThoughtToken {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    token,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Context(r) => emit(
            events,
            ApplicationEvent::ContextSnapshot {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
                system_tokens: r.system_tokens,
                tools_tokens: r.tools_tokens,
                conversation_tokens: r.conversation_tokens,
                total_tokens: r.total_tokens,
                budget: r.budget,
                dropped_messages: r.dropped_messages,
                estimated: r.estimated,
                data_class: r.data_class,
            },
        ),
        Step::Note(text) => {
            let Some(message) =
                redact_step_text(scanner, sink, session_id, "note", text.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::LogDiagnostic {
                    session_id: Some(session_id.to_string()),
                    agent_id: Some(agent_id.to_string()),
                    message,
                },
            )
        }
        Step::Generation(u) => emit(
            events,
            ApplicationEvent::LogDiagnostic {
                session_id: Some(session_id.to_string()),
                agent_id: Some(agent_id.to_string()),
                message: format!(
                    "gen prefill {:?} decode {:?}",
                    u.prompt_tokens, u.eval_tokens
                ),
            },
        ),
        Step::ToolCall {
            call_id,
            name,
            args,
        } => {
            let Some(args) = redact_step_json(scanner, args) else {
                return;
            };
            let parent_agent_id = if agent_id.contains('.') {
                agent_id.rsplit_once('.').map(|(p, _)| p.to_string())
            } else {
                None
            };
            emit(
                events,
                ApplicationEvent::ToolCall {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    call_id,
                    tool: name,
                    args,
                    parent_agent_id,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::ToolResult {
            call_id,
            name,
            ok,
            summary,
        } => {
            let Some(summary) = redact_step_text(scanner, sink, session_id, "tool", summary) else {
                return;
            };
            let parent_agent_id = if agent_id.contains('.') {
                agent_id.rsplit_once('.').map(|(p, _)| p.to_string())
            } else {
                None
            };
            emit(
                events,
                ApplicationEvent::ToolResult {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    call_id,
                    tool: name.clone(),
                    ok,
                    summary: summary.clone(),
                    error_kind: if !ok && summary.to_ascii_lowercase().contains("denied") {
                        Some("denied".into())
                    } else {
                        None
                    },
                    parent_agent_id,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            );
            // R27: mutating tools produce a semantic workspace-mutation effect
            // on the shared execute_turn path (CLI + daemon).
            if ok && matches!(name.as_str(), "write_file" | "edit_file") {
                emit(events, ApplicationEvent::WorkspaceDiff { diff: name });
            }
        }
        Step::Answer(text) => {
            let Some(text) =
                redact_step_text(scanner, sink, session_id, "answer", text.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::TurnAnswer {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    text,
                    from_finish: false,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Stopped(reason) => {
            let Some(message) =
                redact_step_text(scanner, sink, session_id, "stopped", reason.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::LogDiagnostic {
                    session_id: Some(session_id.to_string()),
                    agent_id: Some(agent_id.to_string()),
                    message: format!("stopped: {message}"),
                },
            )
        }
    }
}

/// Build ScannerEngine + redaction audit sink for outbound event edges (R4-3).
pub fn outbound_event_scanner(
    store: &Option<tetonic_memory::SharedStore>,
) -> (Arc<ScannerEngine>, Arc<dyn OutboundRedactionSink>) {
    let scanner = crate::secret_scanner_factory::scanner_from_shared_store(store);
    let sink: Arc<dyn OutboundRedactionSink> = match store {
        Some(s) => Arc::new(StoreRedactionSink::new(s.clone())),
        None => Arc::new(MissingStoreRedactionSink),
    };
    (scanner, sink)
}
