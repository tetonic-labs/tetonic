//! Remote inference placement types (D1 / fabric).

use lokai_domain::{DataClass, DisclosureTier};

use crate::mode::PolicyMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Local,
    EstateWorker { id: String },
    CirclePeer { circle_id: String, peer_id: String },
}

/// Inputs for a remote inference placement decision.
#[derive(Debug, Clone)]
pub struct FabricJobDraft {
    pub data_class: DataClass,
    pub disclosure_tier: DisclosureTier,
    pub destination: Destination,
    pub is_circle_job: bool,
}

#[derive(Debug, Clone)]
pub struct PolicyContext {
    pub mode: PolicyMode,
    pub session_data_class: DataClass,
}
