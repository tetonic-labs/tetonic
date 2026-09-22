use chrono::{DateTime, Utc};
use lokai_domain::ids::{LeaseId, WorkerId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseMessage {
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub holder_identity: WorkerId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub heartbeat_interval_secs: u32,
    pub renewal_limit: Option<u32>,
}

/// Wire heartbeat for lease lifecycle — use [`crate::lifecycle::Heartbeat`] on the coordinator path.
pub type HeartbeatMessage = crate::lifecycle::Heartbeat;
