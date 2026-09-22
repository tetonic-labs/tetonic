use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricErrorCode {
    UnsupportedProtocolVersion,
    UnsupportedJobKind,
    InvalidEnvelope,
    InvalidSignature,
    IdentityMismatch,
    RevokedIdentity,
    StaleRevocationEpoch,
    DuplicateConflict,
    InvalidLease,
    StaleLeaseEpoch,
    DeadlineExceeded,
    InputTooLarge,
    OutputTooLarge,
    CapabilityUnavailable,
    PolicyDenied,
    WorkerBusy,
    WorkerDraining,
    InternalFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricError {
    pub code: FabricErrorCode,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl std::fmt::Display for FabricError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for FabricError {}
