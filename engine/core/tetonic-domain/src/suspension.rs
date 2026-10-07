//! Durable, effect-free suspension at a host-controlled execution boundary.
//! Checkpoint bodies belong to scoped artifact storage, never the run journal.
use crate::{ArtifactRef, AttemptId, CommandEnvelope, ExecutionTargetId, LeaseProof, RunId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspensionReason {
    HumanInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptSuspension {
    pub checkpoint: ArtifactRef,
    pub reason: SuspensionReason,
    pub suspended_at: u64,
    /// Remaining execution time, not a new allowance on each resumption.
    pub remaining_seconds: u64,
}

/// Only the trusted executor may acknowledge that all owned effects have settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuspendAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub lease_proof: LeaseProof,
    pub checkpoint: ArtifactRef,
    pub reason: SuspensionReason,
}

/// An exclusive wake claim. A replay is never a second permission to execute.
/// Current authorization and checkpoint integrity must be checked by the manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub checkpoint: ArtifactRef,
    pub lease_proof: LeaseProof,
    pub holder: ExecutionTargetId,
}
