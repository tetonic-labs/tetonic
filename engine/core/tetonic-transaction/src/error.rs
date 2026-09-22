//! Transaction errors.

use tetonic_domain::{TransactionState, WorkspaceConflict};

#[derive(Debug, thiserror::Error)]
pub enum TransactionError {
    #[error("invalid workspace root: {0}")]
    InvalidWorkspace(String),
    #[error("path outside workspace: {0}")]
    OutsideWorkspace(String),
    #[error("writer lock held by another process: {0}")]
    LockContention(String),
    #[error("recovery required before new transactions: {0}")]
    RecoveryRequired(String),
    #[error("transaction conflict: {0:?}")]
    Conflict(Vec<WorkspaceConflict>),
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: TransactionState,
        to: TransactionState,
    },
    #[error("approval invalid: {0}")]
    ApprovalInvalid(String),
    #[error("transaction size limit exceeded: {0}")]
    SizeLimit(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("{0}")]
    Other(String),
}
