use chrono::{DateTime, Utc};
use lokai_domain::ids::WorkerId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CoordinatorId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FabricTraceContext {
    pub trace_id: String,
    pub span_id: String,
    /// Scheduler decision correlation when stamped by the coordinator (M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_decision_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricMessageType {
    JobOffer,
    JobAccepted,
    JobRejected,
    JobStarted,
    Heartbeat,
    LeaseRenewalRequest,
    LeaseRenewalResponse,
    ProgressUpdate,
    CancellationRequest,
    CancellationAcknowledged,
    JobCompleted,
    JobFailed,
    ResultRejected,
    WorkerDraining,
    WorkerUnavailable,
    Negotiate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricEnvelope<T> {
    pub protocol_version: ProtocolVersion,
    pub message_id: MessageId,
    pub message_type: FabricMessageType,
    pub coordinator_id: CoordinatorId,
    pub worker_id: WorkerId,
    pub sent_at: DateTime<Utc>,
    pub revocation_epoch: u64,
    pub trace_context: FabricTraceContext,
    pub payload: T,
}
