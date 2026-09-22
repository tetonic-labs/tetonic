use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
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

impl From<lokai_run::ManagedRunError> for AppError {
    fn from(error: lokai_run::ManagedRunError) -> Self {
        match error {
            lokai_run::ManagedRunError::InvalidRequest(message) => Self::InvalidRequest(message),
            lokai_run::ManagedRunError::PersistenceFailed(message) => {
                Self::PersistenceFailed(message)
            }
            lokai_run::ManagedRunError::InternalViolation(message) => {
                Self::InternalViolation(message)
            }
        }
    }
}
