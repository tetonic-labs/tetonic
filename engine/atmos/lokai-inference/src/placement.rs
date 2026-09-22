//! Dispatch placement reporting (M2-2 telemetry / UI).

use lokai_domain::{ClassificationSummary, DataClass, DispatchDecision, WorkerTrust};

/// Audit-friendly report when the dispatch guard evaluates placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchPlacementReport {
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub target: String,
    pub decision: DispatchDecision,
    pub reason_code: Option<&'static str>,
    pub reason: Option<String>,
    pub effective_class: Option<DataClass>,
    pub classification: Option<ClassificationSummary>,
    pub redacted: bool,
    /// Coordinator-assigned worker trust tier when target is remote (M5-3).
    pub worker_trust: Option<WorkerTrust>,
}

impl DispatchPlacementReport {
    pub fn decision_str(&self) -> &'static str {
        match self.decision {
            DispatchDecision::LocalOnly => "local_only",
            DispatchDecision::RemoteAllowed => "remote_allowed",
            DispatchDecision::RemoteAllowedWithRedaction => "remote_allowed_with_redaction",
            DispatchDecision::Denied => "denied",
        }
    }
}

pub trait DispatchPlacementSink: Send + Sync {
    fn report(&self, report: DispatchPlacementReport);
}
