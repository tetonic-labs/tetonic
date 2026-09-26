use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("registered agent already has admitted work; retry after it has quiesced")]
    ExecutionCapacityExceeded,
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Session conflict")]
    SessionConflict,
    #[error("Policy denied: {0}")]
    PolicyDenied(String),
    #[error("Approval required for: {0}")]
    ApprovalRequired(String),
    #[error("Canceled")]
    Canceled,
    #[error("Workspace unavailable")]
    WorkspaceUnavailable,
    #[error("Inference unavailable")]
    InferenceUnavailable,
    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),
    #[error("Persistence failed: {0}")]
    PersistenceFailed(String),
    #[error("Internal invariant violation: {0}")]
    InternalViolation(String),
}

impl From<tetonic_run::ManagedRunError> for AppError {
    fn from(error: tetonic_run::ManagedRunError) -> Self {
        match error {
            tetonic_run::ManagedRunError::ExecutionCapacityExceeded => {
                Self::ExecutionCapacityExceeded
            }
            tetonic_run::ManagedRunError::InvalidRequest(message) => Self::InvalidRequest(message),
            tetonic_run::ManagedRunError::PersistenceFailed(message) => {
                Self::PersistenceFailed(message)
            }
            tetonic_run::ManagedRunError::InternalViolation(message) => {
                Self::InternalViolation(message)
            }
        }
    }
}
