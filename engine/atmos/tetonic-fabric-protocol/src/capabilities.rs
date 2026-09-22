//! Worker capability advertisement (M5-1).

use serde::{Deserialize, Serialize};

use crate::{JobKind, ProtocolVersion};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCapabilityAdvertisement {
    pub min_protocol_version: ProtocolVersion,
    pub max_protocol_version: ProtocolVersion,
    pub job_kinds: Vec<JobKind>,
    pub supports_leases: bool,
    pub supports_cancellation: bool,
    pub supports_artifact_semantics: bool,
    /// When true, worker only supports legacy `/v1/chat` — no lease/cancel/artifact guarantees.
    pub legacy_v1_chat_only: bool,
}

impl WorkerCapabilityAdvertisement {
    pub fn fabric_v1_full(job_kinds: Vec<JobKind>) -> Self {
        Self {
            min_protocol_version: ProtocolVersion(1),
            max_protocol_version: ProtocolVersion(1),
            job_kinds,
            supports_leases: true,
            supports_cancellation: true,
            supports_artifact_semantics: true,
            legacy_v1_chat_only: false,
        }
    }

    pub fn legacy_v1_chat_infer_only() -> Self {
        Self {
            min_protocol_version: ProtocolVersion(0),
            max_protocol_version: ProtocolVersion(0),
            job_kinds: vec![JobKind::Infer],
            supports_leases: false,
            supports_cancellation: false,
            supports_artifact_semantics: false,
            legacy_v1_chat_only: true,
        }
    }
}
