use lokai_app::events::ApplicationEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        // Transport-only: tokens, logs, terminal run_status after TurnCompleted.
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

pub fn parse_effects(trace_log: &str) -> Vec<SemanticEffect> {
    trace_log
        .lines()
        .filter_map(|line| {
            let val: Value = serde_json::from_str(line).ok()?;
            let op = val.get("operation_name")?.as_str()?;
            match op {
                "session.start" => Some(SemanticEffect::SessionInitialization),
                "inference.chat" => Some(SemanticEffect::ModelRequest),
                "tool.execute" => Some(SemanticEffect::ToolInvocation {
                    tool: "unknown".into(),
                }),
                "turn.complete" => Some(SemanticEffect::TerminalOutcome),
                _ => None,
            }
        })
        .collect()
}

/// Deterministic session + turn lifecycle exercised by both CLI and daemon adapters.
pub async fn run_kernel_lifecycle_scenario(
    app: &lokai_app::Application,
    workspace_root: &str,
    user_input: &str,
) -> Result<String, lokai_app::errors::AppError> {
    use lokai_app::commands::{
        CompleteTurnCommand, EndSessionCommand, RunTurnCommand, StartSessionCommand,
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_app::events::RecordingEventSink;
    use lokai_app::{Application, ApplicationDependencies};
    use std::sync::Arc;

    #[test]
    fn test_compare_equivalent() {
        let seq = vec![
            SemanticEffect::SessionInitialization,
            SemanticEffect::ModelRequest,
            SemanticEffect::TerminalOutcome,
        ];
        assert!(compare_sequences(&seq, &seq).is_ok());
    }

    #[test]
    fn test_compare_divergent() {
        let s1 = vec![SemanticEffect::SessionInitialization];
        let s2 = vec![
            SemanticEffect::SessionInitialization,
            SemanticEffect::TerminalOutcome,
        ];
        assert!(compare_sequences(&s1, &s2).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn kernel_lifecycle_semantic_effects() {
        let (recorder, events) = RecordingEventSink::new();
        let store = lokai_memory::SharedStore::open(":memory:", 1).unwrap();
        let policy = Arc::new(lokai_policy::PolicyEngine::default());
        let artifact_store = Arc::new(
            lokai_artifact::LocalArtifactStore::new(
                std::env::temp_dir().join("artifacts"),
                lokai_app::secret_scanner_factory::artifact_scan_policy(&Some(store.clone())),
            )
            .unwrap(),
        );
        let runtime = Arc::new(lokai_runtime::EngineRuntime::new(
            policy.clone(),
            None,
            artifact_store,
        ));
        let app = Application::new(ApplicationDependencies {
            runtime,
            store: Some(store),
            policy,
            event_sink: recorder,
            index_db: None,
            fabric_hint: None,
        });

        let tmp = std::env::temp_dir();
        let root = tmp.display().to_string();
        run_kernel_lifecycle_scenario(&app, &root, "hello")
            .await
            .expect("lifecycle");

        let effects = normalize_application_events(&events.lock().unwrap());
        let expected = vec![
            SemanticEffect::SessionInitialization,
            SemanticEffect::ModelRequest,
            SemanticEffect::TerminalOutcome,
            SemanticEffect::PersistenceWrite,
        ];
        compare_sequences(&expected, &effects).expect("kernel lifecycle effects");
    }
}
