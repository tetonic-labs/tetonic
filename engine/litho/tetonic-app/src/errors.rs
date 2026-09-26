use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
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
    #[error("unknown session_id")]
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

impl AppError {
    /// Local history commands must not repeat a store or lock error.
    pub fn hide_store_failure<E: std::fmt::Display>(error: E) -> Self {
        let _ = error;
        Self::PersistenceFailed("request failed".into())
    }

    /// Text an employee may see. Persistence, tool, and internal failures stay
    /// `request failed` so a store or tool body is not repeated.
    pub fn employee_message(&self) -> String {
        match self {
            Self::PersistenceFailed(_)
            | Self::ToolExecutionFailed(_)
            | Self::InternalViolation(_) => "request failed".to_string(),
            Self::SessionNotFound(_) | Self::SessionConflict => "unknown session_id".to_string(),
            Self::InvalidRequest(message)
            | Self::PolicyDenied(message)
            | Self::ApprovalRequired(message) => message.clone(),
            Self::OrganizationCapacityExceeded
            | Self::TeamCapacityExceeded
            | Self::PrincipalCapacityExceeded
            | Self::ExecutionCapacityExceeded
            | Self::Canceled
            | Self::WorkspaceUnavailable
            | Self::InferenceUnavailable => self.to_string(),
        }
    }
}

impl From<tetonic_run::ManagedRunError> for AppError {
    fn from(error: tetonic_run::ManagedRunError) -> Self {
        match error {
            tetonic_run::ManagedRunError::OrganizationCapacityExceeded => {
                Self::OrganizationCapacityExceeded
            }
            tetonic_run::ManagedRunError::TeamCapacityExceeded => Self::TeamCapacityExceeded,
            tetonic_run::ManagedRunError::PrincipalCapacityExceeded => {
                Self::PrincipalCapacityExceeded
            }
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

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn employee_message_hides_store_and_tool_bodies() {
        let stored = AppError::PersistenceFailed("sqlite: PRIVATECANARY".into());
        let tool = AppError::ToolExecutionFailed("stdout PRIVATECANARY".into());
        let missing = AppError::SessionNotFound("disc-private".into());
        assert_eq!(stored.employee_message(), "request failed");
        assert_eq!(tool.employee_message(), "request failed");
        assert_eq!(missing.employee_message(), "unknown session_id");
        assert!(!stored.employee_message().contains("PRIVATECANARY"));
        let hidden = AppError::hide_store_failure("sqlite: PRIVATECANARY");
        assert_eq!(hidden.employee_message(), "request failed");
        assert!(!hidden.to_string().contains("PRIVATECANARY"));
        assert!(!missing.employee_message().contains("disc-private"));
        assert!(!missing.to_string().contains("disc-private"));
        assert_eq!(
            AppError::InvalidRequest("unknown session_id".into()).employee_message(),
            "unknown session_id"
        );
    }
}
