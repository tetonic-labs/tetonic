use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EngineStage {
    Routing { mode: String },
    CompilingContext { workspace: String },
    SearchingIndex { query: String },
    PreparingModel { model: String },
    Inferring { agent_id: String },
    RunningTool { tool: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    Specialist { role: String },
    Evaluator { criterion: String },
    RefinementLoop { iteration: u32, max_iterations: u32 },
    ToolExecution { tool: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeState {
    Pending,
    Active { status_message: String },
    BlockedOnApproval { approval_id: String },
    Succeeded { summary: String },
    Failed { error: String, retryable: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionNodeMeta {
    pub node_id: String,
    pub parent_node_id: Option<String>,
    pub run_id: String,
    pub session_id: String,
    pub kind: NodeKind,
    pub label: String,
}

pub fn make_specialist_node_meta(
    agent_id: &str,
    parent_id: &str,
    run_id: &str,
    session_id: &str,
    role_name: &str,
) -> ExecutionNodeMeta {
    let kind = if role_name.eq_ignore_ascii_case("critic") {
        NodeKind::Evaluator {
            criterion: "LSP & Verification".into(),
        }
    } else {
        NodeKind::Specialist {
            role: role_name.to_string(),
        }
    };
    ExecutionNodeMeta {
        node_id: agent_id.to_string(),
        parent_node_id: Some(parent_id.to_string()),
        run_id: run_id.to_string(),
        session_id: session_id.to_string(),
        label: format!("{role_name} ({agent_id})"),
        kind,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApplicationEvent {
    SessionStarted {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
        status: String,
    },
    StageTransition {
        session_id: String,
        agent_id: String,
        stage: EngineStage,
        detail: Option<String>,
    },
    NodeStarted {
        meta: ExecutionNodeMeta,
    },
    NodeProgress {
        node_id: String,
        state: NodeState,
    },
    NodeCompleted {
        node_id: String,
        state: NodeState,
        duration_ms: u64,
    },
    RunStatus {
        run_id: String,
        status: String,
        agent_id: Option<String>,
        error: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    TurnCompleted {
        session_id: String,
        status: String,
        error: Option<String>,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    Cancellation {
        session_id: String,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
    },
    TurnAnswer {
        session_id: String,
        agent_id: String,
        text: String,
        from_finish: bool,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    ModelToken {
        session_id: String,
        agent_id: String,
        token: String,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    ThoughtToken {
        session_id: String,
        agent_id: String,
        token: String,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    ToolCall {
        session_id: String,
        agent_id: String,
        call_id: String,
        tool: String,
        args: serde_json::Value,
        parent_agent_id: Option<String>,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    ToolResult {
        session_id: String,
        agent_id: String,
        call_id: String,
        tool: String,
        ok: bool,
        summary: String,
        error_kind: Option<String>,
        parent_agent_id: Option<String>,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
        #[serde(default)]
        attempt_id: Option<String>,
        #[serde(default)]
        identity_id: Option<String>,
    },
    WorkspaceDiff {
        diff: String,
    },
    ApprovalRequest {
        session_id: String,
        approval_id: String,
        call_id: String,
        kind: String,
        detail: String,
        tool: String,
        args: serde_json::Value,
        missing_controls: Vec<tetonic_core::ConfinementWarning>,
        user_approval_required: bool,
        #[serde(default)]
        attempt_id: Option<String>,
    },
    EgressActivity {
        destination: String,
    },
    ContextInformation {
        info: String,
    },
    ContextSnapshot {
        session_id: String,
        agent_id: String,
        system_tokens: usize,
        tools_tokens: usize,
        conversation_tokens: usize,
        total_tokens: usize,
        budget: usize,
        dropped_messages: usize,
        estimated: bool,
        data_class: Option<tetonic_domain::DataClass>,
    },
    DispatchPlacement {
        session_id: String,
        agent_id: String,
        target: String,
        decision: String,
        reason_code: Option<String>,
        reason: Option<String>,
        data_class: Option<tetonic_domain::DataClass>,
        redacted: bool,
    },
    LogDiagnostic {
        session_id: Option<String>,
        agent_id: Option<String>,
        message: String,
    },
    PolicyUpdated {
        mode: Option<String>,
    },
    CapacityProgress {
        job_id: String,
        progress_pct: u8,
        message: String,
    },
    InspectorUpdate {
        text: String,
    },
    InspectorClear,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventEnvelope {
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub identity_id: Option<String>,
}

impl EventEnvelope {
    pub fn new(
        run_id: impl Into<String>,
        task_id: impl Into<String>,
        attempt_id: impl Into<String>,
        identity_id: Option<String>,
    ) -> Self {
        Self {
            run_id: Some(run_id.into()),
            task_id: Some(task_id.into()),
            attempt_id: Some(attempt_id.into()),
            identity_id,
        }
    }
}

impl ApplicationEvent {
    pub fn attempt_id(&self) -> Option<&str> {
        match self {
            Self::RunStatus { attempt_id, .. }
            | Self::TurnCompleted { attempt_id, .. }
            | Self::Cancellation { attempt_id, .. }
            | Self::TurnAnswer { attempt_id, .. }
            | Self::ModelToken { attempt_id, .. }
            | Self::ThoughtToken { attempt_id, .. }
            | Self::ToolCall { attempt_id, .. }
            | Self::ToolResult { attempt_id, .. }
            | Self::ApprovalRequest { attempt_id, .. } => attempt_id.as_deref(),
            _ => None,
        }
    }

    pub fn run_id(&self) -> Option<&str> {
        match self {
            Self::RunStatus { run_id, .. } => Some(run_id.as_str()),
            Self::TurnCompleted { run_id, .. }
            | Self::Cancellation { run_id, .. }
            | Self::TurnAnswer { run_id, .. }
            | Self::ModelToken { run_id, .. }
            | Self::ThoughtToken { run_id, .. }
            | Self::ToolCall { run_id, .. }
            | Self::ToolResult { run_id, .. } => run_id.as_deref(),
            _ => None,
        }
    }

    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::RunStatus { task_id, .. }
            | Self::TurnCompleted { task_id, .. }
            | Self::TurnAnswer { task_id, .. }
            | Self::ModelToken { task_id, .. }
            | Self::ThoughtToken { task_id, .. }
            | Self::ToolCall { task_id, .. }
            | Self::ToolResult { task_id, .. } => task_id.as_deref(),
            _ => None,
        }
    }

    pub fn identity_id(&self) -> Option<&str> {
        match self {
            Self::RunStatus { identity_id, .. }
            | Self::TurnCompleted { identity_id, .. }
            | Self::TurnAnswer { identity_id, .. }
            | Self::ModelToken { identity_id, .. }
            | Self::ThoughtToken { identity_id, .. }
            | Self::ToolCall { identity_id, .. }
            | Self::ToolResult { identity_id, .. } => identity_id.as_deref(),
            _ => None,
        }
    }

    pub fn run_status(
        run_id: String,
        status: String,
        agent_id: Option<String>,
        error: Option<String>,
        envelope: &EventEnvelope,
    ) -> Self {
        Self::RunStatus {
            run_id,
            status,
            agent_id,
            error,
            task_id: envelope.task_id.clone(),
            attempt_id: envelope.attempt_id.clone(),
            identity_id: envelope.identity_id.clone(),
        }
    }

    pub fn turn_completed(
        session_id: String,
        status: String,
        error: Option<String>,
        envelope: &EventEnvelope,
    ) -> Self {
        Self::TurnCompleted {
            session_id,
            status,
            error,
            run_id: envelope.run_id.clone(),
            task_id: envelope.task_id.clone(),
            attempt_id: envelope.attempt_id.clone(),
            identity_id: envelope.identity_id.clone(),
        }
    }
}

/// Deliver an application event; blocks until the sink accepts it.
pub fn emit(sink: &Arc<dyn ApplicationEventSink>, event: ApplicationEvent) {
    sink.send(event);
}

pub trait ApplicationEventSink: Send + Sync {
    fn send(&self, event: ApplicationEvent);
}

/// Collects application events for parity tests and evaluation recorders.
pub struct RecordingEventSink {
    events: std::sync::Arc<std::sync::Mutex<Vec<ApplicationEvent>>>,
}

impl RecordingEventSink {
    pub fn new() -> (
        Arc<Self>,
        std::sync::Arc<std::sync::Mutex<Vec<ApplicationEvent>>>,
    ) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                events: events.clone(),
            }),
            events,
        )
    }

    pub fn drain(&self) -> Vec<ApplicationEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl ApplicationEventSink for RecordingEventSink {
    fn send(&self, event: ApplicationEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Forwards events to multiple sinks (e.g. record + RPC adapter).
pub struct FanoutEventSink {
    sinks: Vec<Arc<dyn ApplicationEventSink>>,
}

impl FanoutEventSink {
    pub fn new(sinks: Vec<Arc<dyn ApplicationEventSink>>) -> Arc<Self> {
        Arc::new(Self { sinks })
    }
}

impl ApplicationEventSink for FanoutEventSink {
    fn send(&self, event: ApplicationEvent) {
        for sink in &self.sinks {
            sink.send(event.clone());
        }
    }
}

#[derive(Default, Clone)]
pub struct NoopEventSink;

impl ApplicationEventSink for NoopEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

pub fn terminal_status(outcome: &tetonic_domain::CandidateOutcome) -> String {
    match outcome {
        tetonic_domain::CandidateOutcome::Completed { .. } => "ok".into(),
        tetonic_domain::CandidateOutcome::Canceled { .. } => "canceled".into(),
        tetonic_domain::CandidateOutcome::Failed { .. } => "error".into(),
        _ => "ok".into(),
    }
}

pub fn outcome_error(outcome: &tetonic_domain::CandidateOutcome) -> Option<String> {
    match outcome {
        tetonic_domain::CandidateOutcome::Failed { message } => Some(message.clone()),
        tetonic_domain::CandidateOutcome::Canceled { reason } => Some(reason.clone()),
        _ => None,
    }
}
