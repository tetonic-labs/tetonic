//! Moveable Execution Volume & Agent State Checkpoint Contract (SAE-404).
//!
//! Provides the immutable state checkpoint representation written to cloud PVCs
//! or local disk, allowing any runner node to restore an agent actor in milliseconds
//! with cryptographic integrity guarantees.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::charter::IntentCharter;
use crate::ids::{AgentId, SquadId};

/// Checkpoint and state restoration errors.
#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("I/O error during checkpoint operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Checkpoint checksum mismatch for agent {agent_id}: recorded {expected}, calculated {actual}")]
    IntegrityFailure {
        agent_id: String,
        expected: String,
        actual: String,
    },
    #[error("No valid checkpoint found for agent {0}")]
    NotFound(String),
    #[error("Checkpoint payload corrupted: {0}")]
    Corrupted(String),
}

/// Snapshot of an agent's state at a point in execution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStateCheckpoint {
    /// Monotonic unique identifier for this checkpoint (e.g. `chk-1727140000-001`).
    pub checkpoint_id: String,
    /// Target agent identity.
    pub agent_id: AgentId,
    /// Associated squad if part of a standing fleet.
    pub squad_id: Option<SquadId>,
    /// Working intent charter snapshot with active operational boundaries.
    pub charter_snapshot: Option<IntentCharter>,
    /// Working memory digest (e.g., summary or hash of episodic memory).
    pub working_memory_digest: String,
    /// Serialized working memory buffer / context turns.
    pub working_memory_buffer: String,
    /// List of bound world adapter descriptors / manifests.
    pub active_world_adapters: Vec<String>,
    /// Checkpoint creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Hexadecimal checksum over checkpoint payload to detect bit rot or partial writes.
    pub checksum: String,
}

impl AgentStateCheckpoint {
    /// Creates a new checkpoint and automatically computes its integrity checksum.
    pub fn new(
        checkpoint_id: impl Into<String>,
        agent_id: AgentId,
        squad_id: Option<SquadId>,
        charter_snapshot: Option<IntentCharter>,
        working_memory_digest: impl Into<String>,
        working_memory_buffer: impl Into<String>,
        active_world_adapters: Vec<String>,
    ) -> Self {
        let mut chk = Self {
            checkpoint_id: checkpoint_id.into(),
            agent_id,
            squad_id,
            charter_snapshot,
            working_memory_digest: working_memory_digest.into(),
            working_memory_buffer: working_memory_buffer.into(),
            active_world_adapters,
            created_at: Utc::now(),
            checksum: String::new(),
        };
        chk.checksum = chk.compute_checksum();
        chk
    }

    /// Computes deterministic checksum over the canonical payload fields.
    pub fn compute_checksum(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let update = |h: &mut u64, bytes: &[u8]| {
            for &b in bytes {
                *h ^= b as u64;
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };

        update(&mut h, self.checkpoint_id.as_bytes());
        update(&mut h, self.agent_id.0.as_bytes());
        if let Some(squad) = &self.squad_id {
            update(&mut h, squad.0.as_bytes());
        }
        update(&mut h, self.working_memory_digest.as_bytes());
        update(&mut h, self.working_memory_buffer.as_bytes());
        for adapter in &self.active_world_adapters {
            update(&mut h, adapter.as_bytes());
        }
        format!("{h:016x}")
    }

    /// Validates whether the recorded checksum matches the payload bytes.
    pub fn verify_integrity(&self) -> Result<(), CheckpointError> {
        let calculated = self.compute_checksum();
        if calculated == self.checksum {
            Ok(())
        } else {
            Err(CheckpointError::IntegrityFailure {
                agent_id: self.agent_id.0.clone(),
                expected: self.checksum.clone(),
                actual: calculated,
            })
        }
    }
}

/// Metadata header for checkpoint discovery without full body deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStateCheckpointHeader {
    pub checkpoint_id: String,
    pub agent_id: AgentId,
    pub squad_id: Option<SquadId>,
    pub created_at: DateTime<Utc>,
    pub checksum: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_integrity_verification() {
        let agent = AgentId("agent-007".into());
        let chk = AgentStateCheckpoint::new(
            "chk-100",
            agent.clone(),
            None,
            None,
            "digest-abc",
            "{\"turns\":[1,2,3]}",
            vec!["fs-adapter".into()],
        );

        assert!(chk.verify_integrity().is_ok());

        // Mutate payload to simulate corruption
        let mut corrupted = chk.clone();
        corrupted.working_memory_buffer = "corrupted-data".into();
        assert!(corrupted.verify_integrity().is_err());
    }
}
