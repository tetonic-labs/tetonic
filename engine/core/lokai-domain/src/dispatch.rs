//! Common dispatch guard (M2-2) — single placement gate for remote inference.

use crate::placement::ProjectPlacementPolicy;
use crate::trust::WorkerTrust;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDecision {
    LocalOnly,
    RemoteAllowed,
    RemoteAllowedWithRedaction,
    Denied,
}

impl DispatchDecision {
    pub fn allows_remote(&self) -> bool {
        matches!(self, Self::RemoteAllowed | Self::RemoteAllowedWithRedaction)
    }
}

/// Stable reason codes for audit / UI (M2-2 telemetry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchDenied {
    pub reason_code: &'static str,
    pub reason: String,
}

impl DispatchDenied {
    pub fn new(reason_code: &'static str, reason: impl Into<String>) -> Self {
        Self {
            reason_code,
            reason: reason.into(),
        }
    }
}

/// Where the dispatch guard is evaluating placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchDestination {
    Local,
    RemoteWorker { worker_id: String },
    CirclePeer { circle_id: String, peer_id: String },
}

/// Complete outbound payload classification for guard evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRequest {
    /// Aggregated classification of the full outbound payload.
    pub payload: Option<crate::Classification>,
    /// Session floor classification (if known).
    pub session: Option<crate::Classification>,
    pub destination: DispatchDestination,
    /// True when evaluating a redacted retry after failover.
    pub post_redaction: bool,
    /// Coordinator-assigned worker trust (M5-3).
    pub worker_trust: Option<WorkerTrust>,
    /// Project placement overrides (M5-3).
    pub project_policy: ProjectPlacementPolicy,
}

impl Default for DispatchRequest {
    fn default() -> Self {
        Self {
            payload: None,
            session: None,
            destination: DispatchDestination::Local,
            post_redaction: false,
            worker_trust: None,
            project_policy: ProjectPlacementPolicy::default(),
        }
    }
}

pub trait DispatchGuard: Send + Sync {
    fn evaluate(&self, request: &DispatchRequest) -> Result<DispatchDecision, DispatchDenied>;

    /// Coordinator project placement policy (M5-3).
    fn project_placement_policy(&self) -> ProjectPlacementPolicy {
        ProjectPlacementPolicy::default()
    }
}
