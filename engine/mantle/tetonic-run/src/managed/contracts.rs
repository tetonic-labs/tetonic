use std::sync::Arc;
use tetonic_domain::{
    AgentIdentity, AgentInvocation, AgentJobSpec, AttemptId, CandidateOutcome, RunId, TaskId,
};

#[derive(Debug, thiserror::Error)]
pub enum ManagedRunError {
    #[error("organization execution capacity is occupied; retry after admitted work quiesces")]
    OrganizationCapacityExceeded,
    #[error("team execution capacity is occupied; retry after admitted work quiesces")]
    TeamCapacityExceeded,
    #[error(
        "initiating principal execution capacity is occupied; retry after admitted work quiesces"
    )]
    PrincipalCapacityExceeded,
    #[error("registered agent already has admitted work; retry after it has quiesced")]
    ExecutionCapacityExceeded,
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Persistence failed: {0}")]
    PersistenceFailed(String),
    #[error("Internal invariant violation: {0}")]
    InternalViolation(String),
}

impl ManagedRunError {
    /// Persistence and internal text stay off employee-visible outcomes.
    pub fn redacts_detail(&self) -> bool {
        matches!(
            self,
            Self::PersistenceFailed(_) | Self::InternalViolation(_)
        )
    }

    pub fn employee_message(&self) -> String {
        if self.redacts_detail() {
            "request failed".to_string()
        } else {
            self.to_string()
        }
    }
}

pub(crate) fn published_managed(error: &ManagedRunError) -> String {
    if error.redacts_detail() {
        tracing::warn!("managed run hid an internal failure");
    }
    error.employee_message()
}

pub(crate) fn published_supervisor(error: &tetonic_domain::RunSupervisorError) -> String {
    if error.redacts_detail() {
        tracing::warn!("managed run hid an internal failure");
    }
    error.employee_message()
}

#[cfg(test)]
mod tests {
    use super::ManagedRunError;

    #[test]
    fn employee_message_hides_persistence_bodies() {
        for error in [
            ManagedRunError::PersistenceFailed("sqlite: PRIVATECANARY".into()),
            ManagedRunError::InternalViolation("digest PRIVATECANARY".into()),
        ] {
            let message = super::published_managed(&error);
            assert_eq!(message, "request failed");
            assert!(!message.contains("PRIVATECANARY"));
        }
        let shown = ManagedRunError::ExecutionCapacityExceeded;
        assert!(shown.employee_message().contains("quiesced"));
        assert!(!shown.redacts_detail());
    }
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

/// A durable locator, not permission to execute or a claim that work is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationReceipt {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub audit_session_id: String,
}

#[derive(Debug)]
pub enum ManagedAdmission {
    Admitted(ManagedBinding),
    Existing(ActivationReceipt),
}

pub enum ManagedSubmission {
    Started {
        binding: ManagedBinding,
        completion: tokio::sync::oneshot::Receiver<StartIdentityJobResult>,
    },
    Existing(ActivationReceipt),
}

/// Product-supplied validation of the complete invocation before execution is claimed.
/// The manager supplies the actual invocation; policy implementations decide which
/// fields must match their pinned harness definition and effective grants.
pub type ExecutionPolicy = Arc<
    dyn Fn(
            Option<&AgentIdentity>,
            &AgentJobSpec,
            Option<&str>,
            &[String],
            &AgentInvocation,
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
    pub execution_scope: Option<tetonic_domain::ExecutionScope>,
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
    /// Execution has been claimed. This is the first point a sessionless job
    /// may be reported as started.
    fn execution_claimed(&self, _binding: &ManagedBinding) {}
    fn step(&self, binding: &ManagedBinding, step: &tetonic_core::Step);
    fn terminal(&self, result: &StartIdentityJobResult);
    fn fail_approval_waits(&self, attempt: &AttemptId);
}

/// Optional durable correlation and caller-selected task delivery key.
/// Correlation alone is not employee authorization. The optional host authority
/// binds sessionless tasks; session IDs still require legacy-local scope and cannot
/// be combined with governed activation until session composition is implemented.
#[derive(Clone, Default)]
pub struct AdmissionContext {
    pub activation: Option<tetonic_domain::ActivationBinding>,
    /// Absolute Unix deadline selected by the host, persisted on the task.
    /// Child work can shorten, but cannot extend, its parent's deadline.
    pub deadline: Option<u64>,
    pub authorization: Option<AuthorizedExecution>,
    pub speculation: Option<tetonic_domain::SpeculationConfig>,
    pub session_id: Option<tetonic_domain::SessionId>,
    pub task_id: Option<TaskId>,
}

/// Trusted host authorization, separate from harness definition conformance.
/// Implementations must check current credentials, context and execution grants.
/// Denial details are deliberately not exposed by the manager.
#[async_trait::async_trait]
pub trait ExecutionAuthority: Send + Sync {
    async fn authorize(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> Result<(), ()>;

    /// Checked while an attempt is already running. The default keeps the
    /// admission decision. A grant that can be revoked must override this.
    async fn revoked_during_execution(
        &self,
        _scope: &tetonic_domain::ExecutionScope,
        _identity: &AgentIdentity,
        _job: &AgentJobSpec,
    ) -> bool {
        false
    }
}

/// Host-composed binding, never accepted as a deserialized employee request.
#[derive(Clone)]
pub struct AuthorizedExecution {
    /// Optional durable grant locator; custom host authorities may not use stored grants.
    pub grant_id: Option<String>,
    pub scope: tetonic_domain::ExecutionScope,
    pub authority: Arc<dyn ExecutionAuthority>,
}
