//! Placement policy types (M5-3).

use serde::{Deserialize, Serialize};

use crate::classify::DataClass;
use crate::ids::{AttemptId, RunId, TaskId, WorkerId, WorkspaceVersion};
use crate::run::{ArtifactRef, TraceContext};
use crate::trust::WorkerTrust;

/// Explainable placement outcome reason codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementReason {
    SecretLocalOnly,
    WorkerTrustInsufficient,
    ProjectPolicyDenied,
    CapabilityUnavailable,
    SandboxInsufficient,
    WorkerRevoked,
    /// Operational slow-worker / integrity quarantine — not trust revocation.
    WorkerQuarantined,
    CapabilityStale,
    ProtocolIncompatible,
    InputTooLarge,
    RedactionRequired,
    RedactionFailed,
    VerificationUnavailable,
}

impl PlacementReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SecretLocalOnly => "secret_local_only",
            Self::WorkerTrustInsufficient => "worker_trust_insufficient",
            Self::ProjectPolicyDenied => "project_policy_denied",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::SandboxInsufficient => "sandbox_insufficient",
            Self::WorkerRevoked => "worker_revoked",
            Self::WorkerQuarantined => "worker_quarantined",
            Self::CapabilityStale => "capability_stale",
            Self::ProtocolIncompatible => "protocol_incompatible",
            Self::InputTooLarge => "input_too_large",
            Self::RedactionRequired => "redaction_required",
            Self::RedactionFailed => "redaction_failed",
            Self::VerificationUnavailable => "verification_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EligibleTarget {
    pub worker_id: WorkerId,
    pub trust: WorkerTrust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionPlanReference {
    pub plan_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationRequirement {
    None,
    Required { policy_id: Option<String> },
}

/// Placement eligibility result — does not dispatch work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementDecision {
    LocalOnly {
        reason: PlacementReason,
    },
    Eligible {
        targets: Vec<EligibleTarget>,
        required_verification: VerificationRequirement,
    },
    EligibleAfterRedaction {
        targets: Vec<EligibleTarget>,
        redaction_plan: RedactionPlanReference,
        resulting_class: DataClass,
    },
    Denied {
        reason: PlacementReason,
    },
}

impl PlacementDecision {
    pub fn allows_remote(&self) -> bool {
        matches!(
            self,
            Self::Eligible { .. } | Self::EligibleAfterRedaction { .. }
        )
    }
}

/// Per-project overrides; global Secret invariant is not overrideable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPlacementPolicy {
    /// Allow SensitiveSource to owner-controlled estate workers.
    #[serde(default = "default_true")]
    pub allow_sensitive_to_owner_estate: bool,
    /// Allow RepositorySource to administratively managed workers.
    #[serde(default)]
    pub allow_repository_to_admin_managed: bool,
    /// Required verification policy for remote execution (e.g. "redundant" or "independent_redundant").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_verification: Option<String>,
}

impl Default for ProjectPlacementPolicy {
    fn default() -> Self {
        Self {
            allow_sensitive_to_owner_estate: true,
            allow_repository_to_admin_managed: false,
            required_verification: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Job kinds considered by the placement engine (M5-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementJobKind {
    Infer,
    Embed,
    Compute,
}

/// Full placement evaluation inputs (M5-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementRequest {
    pub run_id: Option<RunId>,
    pub task_id: Option<TaskId>,
    pub attempt_id: Option<AttemptId>,
    pub job_kind: PlacementJobKind,
    pub data_class: DataClass,
    /// Digest-bound artifacts that form part of the complete outbound payload.
    pub input_artifacts: Vec<ArtifactRef>,
    pub workspace_version: Option<WorkspaceVersion>,
    /// Capability labels required by the job (tools/environment/features).
    pub required_capabilities: Vec<String>,
    pub candidate_worker: Option<WorkerId>,
    pub project_policy: ProjectPlacementPolicy,
    /// Coordinator policy epoch at evaluation time — stale cached decisions must not authorize dispatch.
    pub policy_epoch: u64,
    /// Required sandbox controls when set (future typed compute jobs).
    pub required_sandbox: Option<SandboxRequirements>,
    /// Verification policy reference when attestation is required.
    pub verification_policy: Option<VerificationPolicyReference>,
    pub trace_context: TraceContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SandboxRequirements {
    pub process_tree_enforced: bool,
    /// Job requires OS-enforced network denial (DenyAll / allow-loopback).
    pub network_denial_enforced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationPolicyReference {
    pub policy_id: String,
}

/// UI-safe placement summary without classified payload content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementExplanation {
    pub decision: String,
    pub reason_code: String,
    pub worker_trust: Option<String>,
    pub data_class: Option<String>,
}

impl PlacementDecision {
    pub fn ui_summary(
        &self,
        data_class: Option<DataClass>,
        trust: Option<WorkerTrust>,
    ) -> PlacementExplanation {
        match self {
            Self::LocalOnly { reason } => PlacementExplanation {
                decision: "local_only".into(),
                reason_code: reason.code().into(),
                worker_trust: trust.map(|t| t.as_str().into()),
                data_class: data_class.map(|c| format!("{c:?}")),
            },
            Self::Eligible { .. } => PlacementExplanation {
                decision: "eligible".into(),
                reason_code: "eligible".into(),
                worker_trust: trust.map(|t| t.as_str().into()),
                data_class: data_class.map(|c| format!("{c:?}")),
            },
            Self::EligibleAfterRedaction {
                resulting_class, ..
            } => PlacementExplanation {
                decision: "eligible_after_redaction".into(),
                reason_code: "redaction_required".into(),
                worker_trust: trust.map(|t| t.as_str().into()),
                data_class: Some(format!("{resulting_class:?}")),
            },
            Self::Denied { reason } => PlacementExplanation {
                decision: "denied".into(),
                reason_code: reason.code().into(),
                worker_trust: trust.map(|t| t.as_str().into()),
                data_class: data_class.map(|c| format!("{c:?}")),
            },
        }
    }
}
