//! Semantic effect normalization and comparison for lifecycle parity.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{
    CompleteTurnCommand, EndSessionCommand, RunTurnCommand, StartSessionCommand,
};
use crate::errors::AppError;
use crate::events::ApplicationEvent;
use crate::Application;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SemanticEffect {
    SessionInitialization,
    ModelRequest,
    PolicyDecision,
    CapabilityIssuance,
    ApprovalRequest,
    ToolInvocation { tool: String },
    ProcessExecutionRequest { cmd: String },
    FileRead { path: String },
    WorkspaceMutation { action: String },
    DiffProduction,
    PersistenceWrite,
    RemoteInferenceRequest,
    Cancellation,
    TerminalOutcome,
}

pub fn compare_sequences(
    cli_seq: &[SemanticEffect],
    daemon_seq: &[SemanticEffect],
) -> Result<(), String> {
    if cli_seq == daemon_seq {
        Ok(())
    } else {
        Err(format!(
            "Sequences diverge.\nCLI: {:#?}\nDaemon: {:#?}",
            cli_seq, daemon_seq
        ))
    }
}

/// Normalize canonical application events into M0-4 semantic effects.
pub fn normalize_application_events(events: &[ApplicationEvent]) -> Vec<SemanticEffect> {
    let mut out = Vec::new();
    for event in events {
        if let Some(effect) = normalize_application_event(event) {
            out.push(effect);
        }
    }
    out
}

fn normalize_application_event(event: &ApplicationEvent) -> Option<SemanticEffect> {
    match event {
        ApplicationEvent::SessionStarted { .. } => Some(SemanticEffect::SessionInitialization),
        ApplicationEvent::RunStatus { status, .. } if status == "started" => {
            Some(SemanticEffect::ModelRequest)
        }
        ApplicationEvent::TurnCompleted { .. } => Some(SemanticEffect::TerminalOutcome),
        ApplicationEvent::ApprovalRequest { .. } => Some(SemanticEffect::ApprovalRequest),
        ApplicationEvent::ToolCall { tool, .. } if tool == "finish" => None,
        ApplicationEvent::ToolCall { tool, .. } => {
            Some(SemanticEffect::ToolInvocation { tool: tool.clone() })
        }
        ApplicationEvent::ToolResult { .. } => None,
        ApplicationEvent::Cancellation { .. } => Some(SemanticEffect::Cancellation),
        ApplicationEvent::PolicyUpdated { .. } => Some(SemanticEffect::PolicyDecision),
        ApplicationEvent::SessionEnded { .. } => Some(SemanticEffect::PersistenceWrite),
        ApplicationEvent::WorkspaceDiff { diff } => Some(SemanticEffect::WorkspaceMutation {
            action: diff.clone(),
        }),
        ApplicationEvent::EgressActivity { .. } => Some(SemanticEffect::RemoteInferenceRequest),
        ApplicationEvent::RunStatus { .. }
        | ApplicationEvent::TurnAnswer { .. }
        | ApplicationEvent::StageTransition { .. }
        | ApplicationEvent::NodeStarted { .. }
        | ApplicationEvent::NodeProgress { .. }
        | ApplicationEvent::NodeCompleted { .. }
        | ApplicationEvent::ModelToken { .. }
        | ApplicationEvent::ThoughtToken { .. }
        | ApplicationEvent::ContextSnapshot { .. }
        | ApplicationEvent::DispatchPlacement { .. }
        | ApplicationEvent::LogDiagnostic { .. }
        | ApplicationEvent::ContextInformation { .. }
        | ApplicationEvent::CapacityProgress { .. }
        | ApplicationEvent::InspectorUpdate { .. }
        | ApplicationEvent::InspectorClear => None,
    }
}

/// Normalize JSON-RPC daemon notifications (transport) into semantic effects.
pub fn normalize_rpc_notifications(values: &[Value]) -> Vec<SemanticEffect> {
    let mut out = Vec::new();
    for v in values {
        if let Some(effect) = normalize_rpc_notification(v) {
            out.push(effect);
        }
    }
    out
}

fn normalize_rpc_notification(v: &Value) -> Option<SemanticEffect> {
    let method = v.get("method")?.as_str()?;
    let params = v.get("params")?;
    match method {
        "event/run_status" => {
            let status = params.get("status")?.as_str()?;
            match status {
                "started" => Some(SemanticEffect::ModelRequest),
                "ok" | "error" | "canceled" => Some(SemanticEffect::TerminalOutcome),
                _ => None,
            }
        }
        "event/approval_request" => Some(SemanticEffect::ApprovalRequest),
        "event/tool_call" => {
            if params.get("phase").and_then(|p| p.as_str()) == Some("proposed") {
                Some(SemanticEffect::ToolInvocation {
                    tool: params
                        .get("tool")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                })
            } else {
                None
            }
        }
        "event/tool_result" => None,
        "event/token" => None,
        "event/log" => {
            let msg = params.get("message")?.as_str()?;
            if msg.contains("session started") {
                Some(SemanticEffect::SessionInitialization)
            } else if msg.contains("session ended") {
                Some(SemanticEffect::PersistenceWrite)
            } else if msg.contains("run canceled") {
                Some(SemanticEffect::Cancellation)
            } else if msg.contains("turn completed") {
                Some(SemanticEffect::TerminalOutcome)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Deterministic session + turn lifecycle exercised by both CLI and daemon adapters.
pub async fn run_kernel_lifecycle_scenario(
    app: &Application,
    workspace_root: &str,
    user_input: &str,
) -> Result<String, AppError> {
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: workspace_root.to_string(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: None,
            briefing: Some(false),
            orchestration: None,
            critic: None,
            llm_router: None,
            model_fast: None,
            model_hard: None,
            session_max_steps: None,
            ..Default::default()
        })
        .await?;

    let plan = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: user_input.to_string(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await?;

    app.runs
        .complete_turn(
            &CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: workspace_root.to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await?;

    app.sessions.end_session(EndSessionCommand {
        session_id: started.session_id.clone(),
        workspace_root: workspace_root.to_string(),
        status: Some("ok".into()),
        error: None,
    })?;

    Ok(started.session_id)
}
