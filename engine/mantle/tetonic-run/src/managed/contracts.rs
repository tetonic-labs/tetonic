use std::sync::Arc;
use tetonic_domain::{
    AgentIdentity, AgentInvocation, AgentJobSpec, AttemptId, CandidateOutcome, RunId, TaskId,
};

#[derive(Debug, thiserror::Error)]
pub enum ManagedRunError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Persistence failed: {0}")]
    PersistenceFailed(String),
    #[error("Internal invariant violation: {0}")]
    InternalViolation(String),
}

#[derive(Debug, Clone)]
pub struct StartIdentityJobCommand {
    pub identity: AgentIdentity,
    pub job_spec: AgentJobSpec,
    pub invocation: AgentInvocation,
}

#[derive(Debug, Clone)]
pub struct StartIdentityJobResult {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub outcome: CandidateOutcome,
}

/// Product-supplied validation; manager owns invocation and enforcement timing.
pub type ExecutionPolicy = Arc<
    dyn Fn(
            Option<&AgentIdentity>,
            &AgentJobSpec,
            Option<&str>,
            &[String],
            usize,
        ) -> Result<(), String>
        + Send
        + Sync,
>;

/// Effects stay capability-owned; the manager coordinates finalization.
pub trait FinalizationEffectDriver: Send + Sync {
    fn bind_effect_identity(&self, task_id: &TaskId, attempt_id: &AttemptId) -> Result<(), String>;
    fn run_verify(
        &self,
        verify_cmd: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)>;
    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String>;
}

#[derive(Clone, Default)]
pub struct FinalizationPolicy {
    pub effect_driver: Option<Arc<dyn FinalizationEffectDriver>>,
    pub verify_cmd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DispatchId(pub uuid::Uuid);

impl DispatchId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for DispatchId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DispatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct DispatchTicket {
    pub id: DispatchId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedBinding {
    pub session_id: Option<tetonic_domain::SessionId>,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub job_spec: AgentJobSpec,
}

#[derive(Debug, Clone)]
pub struct AdmitJob {
    pub identity: AgentIdentity,
    pub job_spec: AgentJobSpec,
    pub role: Option<String>,
    pub parent_attempt: Option<AttemptId>,
}

#[derive(Clone)]
pub struct FinalizeJob {
    pub attempt: AttemptId,
    pub outcome: CandidateOutcome,
    pub policy: Option<FinalizationPolicy>,
    pub finish_run: bool,
}

pub trait ManagedRunHooks: Send + Sync {
    fn started(&self, binding: &ManagedBinding);
    fn step(&self, binding: &ManagedBinding, step: &tetonic_core::Step);
    fn terminal(&self, result: &StartIdentityJobResult);
    fn fail_approval_waits(&self, attempt: &AttemptId);
}

/// Optional durable correlation and caller-selected task delivery key.
#[derive(Clone, Default)]
pub struct AdmissionContext {
    pub speculation: Option<tetonic_domain::SpeculationConfig>,
    pub session_id: Option<tetonic_domain::SessionId>,
    pub task_id: Option<TaskId>,
}
