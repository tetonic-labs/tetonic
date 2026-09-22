//! Sink and authority trait contracts (AC2). Implementations live in runtime/tools crates.

use crate::execution::{ActionPolicyOutcome, AuthorizedAction, ExecutionOutcome, ProposedAction};
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("approval required but not satisfied")]
    ApprovalRequired,
    #[error("capability scope mismatch")]
    ScopeMismatch,
    #[error("capability already consumed")]
    AlreadyConsumed,
    #[error("capability expired")]
    Expired,
    #[error("capability revoked")]
    Revoked,
    #[error("data classification mismatch")]
    DataClassificationMismatch,
    #[error("workspace version mismatch")]
    WorkspaceVersionMismatch,
    #[error("capability persist failed")]
    PersistFailed,
}

#[derive(Debug, Error)]
pub enum ProcessBrokerError {
    #[error("capability validation failed: {0}")]
    Capability(#[from] CapabilityError),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
}

/// Evaluates a proposed action (policy rules live in `lokai-policy`).
pub trait PolicyEvaluator: Send + Sync {
    fn evaluate(&self, action: &ProposedAction) -> ActionPolicyOutcome;
}

/// Validates capability before sink execution (single-use).
pub trait CapabilityConsumer: Send + Sync {
    fn authorize(&self, authorized: &AuthorizedAction) -> Result<(), CapabilityError>;
}

pub struct AuthorizedProcessRequest {
    pub authorized_action: AuthorizedAction,
    /// Invocation-local ownership; never part of persisted capability authority.
    pub work_scope: crate::work_scope::WorkScope,
}

pub struct AuthorizedServiceRequest {
    pub authorized_action: AuthorizedAction,
}

pub struct ManagedProcessResult {
    pub success: bool,
    pub output: String,
    pub execution_id: crate::ids::ExecutionId,
}

#[async_trait]
pub trait ManagedProcessHandle: Send + Sync {
    async fn wait(&mut self) -> Result<ManagedProcessResult, ProcessBrokerError>;
    async fn cancel(&mut self) -> Result<(), ProcessBrokerError>;
}

#[async_trait]
pub trait ProcessBroker: Send + Sync {
    async fn execute(
        &self,
        request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError>;

    async fn start_service(
        &self,
        request: AuthorizedServiceRequest,
    ) -> Result<Box<dyn ManagedProcessHandle>, ProcessBrokerError>;
}

#[async_trait]
pub trait ProcessSink: Send + Sync {
    /// The worker retains a scope lease even if its async waiter is dropped.
    async fn run_process(
        &self,
        authorized: &AuthorizedAction,
        scope: &crate::work_scope::WorkScope,
    ) -> ExecutionOutcome;
}

#[async_trait]
pub trait MutationSink: Send + Sync {
    async fn apply_mutation(&self, authorized: &AuthorizedAction) -> ExecutionOutcome;
}

#[async_trait]
pub trait ActionBroker: Send + Sync {
    async fn evaluate_and_issue(
        &self,
        action: &ProposedAction,
    ) -> Result<crate::execution::IssuedCapability, CapabilityError>;
}
