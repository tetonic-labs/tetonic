//! CLI adapter for `lokai-app` — terminal event rendering and approval coordination.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tetonic_app::approval::ApprovalService;
use tetonic_app::commands::ApprovalResponseCommand;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};

use crate::printer::Printer;

/// Maps canonical application events to terminal output.
pub struct TerminalRenderer {
    printer: Mutex<Printer>,
    pub last_run_status: Mutex<Option<String>>,
    tui_tx: Mutex<Option<crate::event_queue::Sender>>,
}

impl TerminalRenderer {
    pub fn new() -> Self {
        Self {
            printer: Mutex::new(Printer::default()),
            last_run_status: Mutex::new(None),
            tui_tx: Mutex::new(None),
        }
    }

    pub fn with_tui(tx: crate::event_queue::Sender) -> Self {
        Self {
            printer: Mutex::new(Printer::default()),
            last_run_status: Mutex::new(None),
            tui_tx: Mutex::new(Some(tx)),
        }
    }

    pub fn is_tui(&self) -> bool {
        self.tui_tx.lock().unwrap().is_some()
    }

    pub fn end_line(&self) {
        if self.tui_tx.lock().unwrap().is_none() {
            self.printer.lock().unwrap().end_line();
        }
    }

    fn on_event(&self, event: &ApplicationEvent) {
        if let Some(tx) = self.tui_tx.lock().unwrap().as_ref() {
            let _ = tx.send(event.clone());
            return;
        }

        match event {
            ApplicationEvent::StageTransition {
                detail: Some(msg), ..
            } => {
                eprintln!("  [{msg}]");
            }
            ApplicationEvent::NodeStarted { meta } => {
                eprintln!("  ┌─ [{}] {}", meta.node_id, meta.label);
            }
            ApplicationEvent::NodeCompleted {
                state: tetonic_app::events::NodeState::Succeeded { summary },
                ..
            } => {
                eprintln!("  └─ Succeeded: {summary}");
            }
            ApplicationEvent::TurnAnswer { text, .. } => {
                self.printer.lock().unwrap().on_answer(text);
            }
            ApplicationEvent::ModelToken { token, .. } => {
                self.printer.lock().unwrap().on_token(token);
            }
            ApplicationEvent::ThoughtToken { token, .. } => {
                self.printer.lock().unwrap().on_thought(token);
            }
            ApplicationEvent::ToolCall { tool, args, .. } => {
                self.printer.lock().unwrap().on_tool_call(tool, args);
            }
            ApplicationEvent::ToolResult { ok, summary, .. } => {
                self.printer.lock().unwrap().on_tool_result(*ok, summary);
            }
            ApplicationEvent::ContextInformation { info } => {
                eprintln!("\n[ctx] {info}");
            }
            ApplicationEvent::LogDiagnostic { message, .. } => {
                if message.starts_with("router:") || message.starts_with("orchestration:") {
                    eprintln!("  {message}");
                } else {
                    eprintln!("\n[log] {message}");
                }
            }
            ApplicationEvent::RunStatus { status, error, .. } => {
                *self.last_run_status.lock().unwrap() = Some(status.clone());
                if let Some(err) = error {
                    eprintln!("\n[run] {status}: {err}");
                }
            }
            ApplicationEvent::Cancellation { session_id, .. } => {
                eprintln!("\n[canceled] session {session_id}");
            }
            ApplicationEvent::CapacityProgress {
                progress_pct,
                message,
                ..
            } => {
                eprintln!("[capacity {progress_pct}%] {message}");
            }
            ApplicationEvent::DispatchPlacement {
                target,
                decision,
                reason_code,
                ..
            } => {
                let why = reason_code
                    .as_deref()
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default();
                eprintln!("[dispatch] {decision} → {target}{why}");
            }
            ApplicationEvent::ApprovalRequest {
                kind,
                detail,
                missing_controls,
                user_approval_required,
                ..
            } => {
                eprintln!("\n[approval required] {kind} — {detail}");
                let extra = tetonic_app::approval::format_confinement_prompt(
                    missing_controls,
                    *user_approval_required,
                );
                if !extra.is_empty() {
                    eprintln!("{extra}");
                }
            }
            _ => {}
        }
    }
}

/// Coordinates non-TUI approval prompts with `ApprovalService`.
pub struct TerminalApprovalCoordinator {
    approvals: Mutex<Option<Arc<dyn ApprovalService>>>,
}

impl TerminalApprovalCoordinator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            approvals: Mutex::new(None),
        })
    }

    pub fn bind_approvals(&self, approvals: Arc<dyn ApprovalService>) {
        *self.approvals.lock().unwrap() = Some(approvals);
    }

    pub fn get_approvals(&self) -> Option<Arc<dyn ApprovalService>> {
        self.approvals.lock().unwrap().clone()
    }

    async fn prompt_and_respond(
        &self,
        session_id: &str,
        approval_id: &str,
        kind: &str,
        detail: &str,
    ) -> bool {
        eprint!("Allow? [y/N/r=remember]: ");
        let _ = io::stderr().flush();
        let mut line = String::new();
        let read_ok = io::stdin().read_line(&mut line).is_ok();
        let choice = line.trim().to_ascii_lowercase();
        let allowed = read_ok && matches!(choice.as_str(), "y" | "yes");
        let remember = read_ok && matches!(choice.as_str(), "r" | "remember" | "yes remember");
        let approvals = self
            .approvals
            .lock()
            .unwrap()
            .clone()
            .expect("approval coordinator not bound");
        let _ = approvals.respond(ApprovalResponseCommand {
            session_id: session_id.to_string(),
            approval_id: approval_id.to_string(),
            approved: allowed,
            remember: remember && allowed,
            kind: kind.to_string(),
            detail: detail.to_string(),
            channel_delivered: true,
            attempt_id: None,
        });
        allowed
    }
}

pub struct TerminalEventSink {
    renderer: Arc<TerminalRenderer>,
    coordinator: Arc<TerminalApprovalCoordinator>,
}

impl TerminalEventSink {
    pub fn new(
        renderer: Arc<TerminalRenderer>,
        coordinator: Arc<TerminalApprovalCoordinator>,
    ) -> Arc<Self> {
        Arc::new(Self {
            renderer,
            coordinator,
        })
    }
}

impl ApplicationEventSink for TerminalEventSink {
    fn send(&self, event: ApplicationEvent) {
        if let ApplicationEvent::ApprovalRequest {
            session_id,
            approval_id,
            kind,
            detail,
            ..
        } = &event
        {
            if !self.renderer.is_tui() {
                let coordinator = self.coordinator.clone();
                let session_id = session_id.clone();
                let approval_id = approval_id.clone();
                let kind = kind.clone();
                let detail = detail.clone();
                tokio::spawn(async move {
                    coordinator
                        .prompt_and_respond(&session_id, &approval_id, &kind, &detail)
                        .await;
                });
            }
        }
        self.renderer.on_event(&event);
    }
}
