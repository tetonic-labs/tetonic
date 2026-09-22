//! Result integrity and acceptance policy types (M5-4).

use serde::{Deserialize, Serialize};

/// How thoroughly a remote result must be verified before acceptance.
/// Distinct from placement [`crate::placement::VerificationRequirement`], which
/// only gates whether a job may be dispatched remotely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ResultVerificationRequirement {
    #[default]
    StructuralValidation,
    LocalVerification,
    IndependentRedundantVerification {
        required_agreement: u32,
    },
    LocalAndRedundantVerification,
}

/// Durable disposition of a remote result envelope (idempotent replay key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultDisposition {
    Accepted,
    Quarantined,
    RejectedInvalidSignature,
    RejectedIdentityMismatch,
    RejectedRevokedWorker,
    RejectedStaleAttempt,
    RejectedCanceled,
    RejectedDigestMismatch,
    RejectedSchema,
    RejectedPolicy,
    RejectedVerification,
    RejectedOversized,
    RejectedPathUnsafe,
    RejectedExecutablePayload,
    Superseded,
}

impl ResultDisposition {
    pub fn is_terminal_reject(self) -> bool {
        !matches!(self, Self::Accepted | Self::Quarantined)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Quarantined => "quarantined",
            Self::RejectedInvalidSignature => "rejected_invalid_signature",
            Self::RejectedIdentityMismatch => "rejected_identity_mismatch",
            Self::RejectedRevokedWorker => "rejected_revoked_worker",
            Self::RejectedStaleAttempt => "rejected_stale_attempt",
            Self::RejectedCanceled => "rejected_canceled",
            Self::RejectedDigestMismatch => "rejected_digest_mismatch",
            Self::RejectedSchema => "rejected_schema",
            Self::RejectedPolicy => "rejected_policy",
            Self::RejectedVerification => "rejected_verification",
            Self::RejectedOversized => "rejected_oversized",
            Self::RejectedPathUnsafe => "rejected_path_unsafe",
            Self::RejectedExecutablePayload => "rejected_executable_payload",
            Self::Superseded => "superseded",
        }
    }
}

/// Coordinator-observed operational health for scheduling eligibility (M5-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOperationalState {
    #[default]
    Healthy,
    Degraded,
    Quarantined,
    Revoked,
}

impl WorkerOperationalState {
    pub fn allows_scheduling(self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Quarantined => "quarantined",
            Self::Revoked => "revoked",
        }
    }
}

/// Origin tag for artifact quarantine (remote results enter as Remote).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOrigin {
    Local,
    Remote,
}

/// Counts of result-integrity violations used for eligibility signals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkerBehaviorSignals {
    pub invalid_signatures: u64,
    pub digest_mismatches: u64,
    pub stale_results: u64,
    pub lease_violations: u64,
    pub oversized_responses: u64,
    pub schema_violations: u64,
    pub verification_failures: u64,
    pub independence_failures: u64,
    pub malformed_output: u64,
}

impl WorkerBehaviorSignals {
    pub fn record(&mut self, disposition: ResultDisposition) {
        match disposition {
            ResultDisposition::RejectedInvalidSignature => self.invalid_signatures += 1,
            ResultDisposition::RejectedDigestMismatch => self.digest_mismatches += 1,
            ResultDisposition::RejectedStaleAttempt | ResultDisposition::Superseded => {
                self.stale_results += 1
            }
            ResultDisposition::RejectedOversized => self.oversized_responses += 1,
            ResultDisposition::RejectedSchema
            | ResultDisposition::RejectedPathUnsafe
            | ResultDisposition::RejectedExecutablePayload => self.schema_violations += 1,
            ResultDisposition::RejectedVerification => self.verification_failures += 1,
            ResultDisposition::RejectedIdentityMismatch
            | ResultDisposition::RejectedRevokedWorker
            | ResultDisposition::RejectedCanceled
            | ResultDisposition::RejectedPolicy => self.malformed_output += 1,
            ResultDisposition::Accepted | ResultDisposition::Quarantined => {}
        }
    }

    pub fn derive_operational_state(&self) -> WorkerOperationalState {
        if self.invalid_signatures >= 3 || self.verification_failures >= 5 {
            return WorkerOperationalState::Quarantined;
        }
        if self.stale_results
            + self.lease_violations
            + self.digest_mismatches
            + self.schema_violations
            + self.oversized_responses
            + self.malformed_output
            >= 3
        {
            return WorkerOperationalState::Degraded;
        }
        WorkerOperationalState::Healthy
    }
}
