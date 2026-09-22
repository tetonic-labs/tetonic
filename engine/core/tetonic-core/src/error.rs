//! Typed errors for the agent execution pipeline.

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("user denied the action")]
    UserDenied,
    #[error("policy engine denied the action: {0}")]
    PolicyDenied(String),
    #[error("approval required: {0}")]
    ApprovalRequired(String),
    #[error("capability authorization failed: {0}")]
    Capability(String),
    #[error("tool execution task panicked")]
    ToolExecutionPanicked,
}

impl AgentError {
    /// User-facing tool feedback when verify-before-finish fails before running.
    pub fn verify_finish_feedback(&self, cmd: &str) -> (String, String) {
        match self {
            AgentError::UserDenied | AgentError::ApprovalRequired(_) => (
                format!(
                    "Verification command `{cmd}` was not approved — fix issues and try finish again."
                ),
                "verify not approved".into(),
            ),
            AgentError::PolicyDenied(reason) => (
                format!("Verification command `{cmd}` denied by policy: {reason}"),
                "verify denied by policy".into(),
            ),
            AgentError::Capability(reason) => (
                format!("Verification command `{cmd}` denied: {reason}"),
                "verify denied".into(),
            ),
            AgentError::ToolExecutionPanicked => (
                format!("Verification command `{cmd}` failed: internal tool runner error"),
                "verify runner error".into(),
            ),
        }
    }
}
