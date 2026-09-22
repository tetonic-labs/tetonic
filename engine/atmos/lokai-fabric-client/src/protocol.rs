//! Protocol encode/decode and validation for coordinator client (M5-1).

use chrono::Utc;
use lokai_fabric_protocol::{
    decode_envelope_json, default_message_limits, validate_channel_identity, validate_job_envelope,
    validate_mandatory_envelope_fields, validate_result_context, validate_revocation_epoch,
    CancellationState, FabricEnvelope, FabricError, IngressState, JobEnvelope, LifecycleContext,
    MessageSizeLimits, ProtocolVersion, WorkerCapabilityAdvertisement,
};

pub use lokai_fabric_protocol::{
    self, validate_revoked, CancellationAcknowledged, CancellationRequest,
    CancellationState as ProtocolCancellationState, FabricMessageType, IdempotencyKey,
    IngressDecision, IngressState as ProtocolIngressState, JobKind, JobOffer, LeaseMessage,
    VersionNegotiationRequest, WorkerCapabilityAdvertisement as ProtocolCapabilities,
};

use crate::FabricClientError;

pub fn legacy_worker_capabilities() -> WorkerCapabilityAdvertisement {
    WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
}

pub fn validate_outbound_job(
    channel_worker_id: &lokai_domain::ids::WorkerId,
    envelope: &FabricEnvelope<JobOffer>,
    limits: &MessageSizeLimits,
) -> Result<(), FabricClientError> {
    validate_mandatory_envelope_fields(envelope).map_err(map_protocol_err)?;
    validate_channel_identity(channel_worker_id, &envelope.worker_id).map_err(map_protocol_err)?;
    validate_job_envelope(&envelope.payload.job).map_err(map_protocol_err)?;
    let payload_len = serde_json::to_vec(&envelope.payload.job)
        .map(|b| b.len())
        .unwrap_or(0);
    lokai_fabric_protocol::check_payload_size(payload_len, limits).map_err(map_protocol_err)?;
    Ok(())
}

pub fn validate_inbound_envelope_bytes(
    bytes: &[u8],
    channel_worker_id: &lokai_domain::ids::WorkerId,
    known_revocation_epoch: u64,
    identity_revoked: bool,
    limits: &MessageSizeLimits,
) -> Result<FabricEnvelope<serde_json::Value>, FabricClientError> {
    validate_revoked(identity_revoked).map_err(map_protocol_err)?;
    let envelope =
        decode_envelope_json::<serde_json::Value>(bytes, limits).map_err(map_protocol_err)?;
    validate_mandatory_envelope_fields(&envelope).map_err(map_protocol_err)?;
    validate_channel_identity(channel_worker_id, &envelope.worker_id).map_err(map_protocol_err)?;
    validate_revocation_epoch(envelope.revocation_epoch, known_revocation_epoch)
        .map_err(map_protocol_err)?;
    Ok(envelope)
}

pub fn validate_result_lifecycle(
    ctx: &LifecycleContext,
    channel_worker_id: &lokai_domain::ids::WorkerId,
    current_lease_epoch: u64,
    cancel_state: &CancellationState,
) -> Result<(), FabricClientError> {
    let canceled = cancel_state.is_canceled(&ctx.attempt_id, ctx.lease_epoch);
    validate_result_context(ctx, channel_worker_id, current_lease_epoch, canceled)
        .map_err(map_protocol_err)
}

pub fn accept_job_delivery(
    ingress: &mut IngressState,
    job: &JobEnvelope,
) -> Result<IngressDecision, FabricClientError> {
    ingress.accept_delivery(job).map_err(map_protocol_err)
}

pub fn encode_job_offer_envelope(
    coordinator_id: lokai_fabric_protocol::CoordinatorId,
    worker_id: lokai_domain::ids::WorkerId,
    job: JobEnvelope,
    revocation_epoch: u64,
) -> FabricEnvelope<JobOffer> {
    FabricEnvelope {
        protocol_version: ProtocolVersion(lokai_fabric_protocol::PROTOCOL_VERSION),
        message_id: lokai_fabric_protocol::MessageId(format!("msg_{}", uuid_simple())),
        message_type: FabricMessageType::JobOffer,
        coordinator_id,
        worker_id,
        sent_at: Utc::now(),
        revocation_epoch,
        trace_context: lokai_fabric_protocol::FabricTraceContext {
            trace_id: uuid_simple(),
            span_id: uuid_simple(),
            scheduler_decision_id: None,
        },
        payload: JobOffer { job },
    }
}

pub fn encode_cancellation_envelope(
    coordinator_id: lokai_fabric_protocol::CoordinatorId,
    worker_id: lokai_domain::ids::WorkerId,
    cancel: CancellationRequest,
    revocation_epoch: u64,
) -> FabricEnvelope<CancellationRequest> {
    FabricEnvelope {
        protocol_version: ProtocolVersion(lokai_fabric_protocol::PROTOCOL_VERSION),
        message_id: lokai_fabric_protocol::MessageId(format!("msg_{}", uuid_simple())),
        message_type: FabricMessageType::CancellationRequest,
        coordinator_id,
        worker_id,
        sent_at: Utc::now(),
        revocation_epoch,
        trace_context: lokai_fabric_protocol::FabricTraceContext {
            trace_id: uuid_simple(),
            span_id: uuid_simple(),
            scheduler_decision_id: None,
        },
        payload: cancel,
    }
}

pub fn encode_lease_renewal_envelope(
    coordinator_id: lokai_fabric_protocol::CoordinatorId,
    worker_id: lokai_domain::ids::WorkerId,
    lease: lokai_fabric_protocol::LeaseRenewalRequest,
    revocation_epoch: u64,
) -> FabricEnvelope<lokai_fabric_protocol::LeaseRenewalRequest> {
    FabricEnvelope {
        protocol_version: ProtocolVersion(lokai_fabric_protocol::PROTOCOL_VERSION),
        message_id: lokai_fabric_protocol::MessageId(format!("msg_{}", uuid_simple())),
        message_type: FabricMessageType::LeaseRenewalRequest,
        coordinator_id,
        worker_id,
        sent_at: Utc::now(),
        revocation_epoch,
        trace_context: lokai_fabric_protocol::FabricTraceContext {
            trace_id: uuid_simple(),
            span_id: uuid_simple(),
            scheduler_decision_id: None,
        },
        payload: lease,
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:x}", d.as_nanos()))
        .unwrap_or_else(|_| "0".into())
}

pub fn map_protocol_err(e: FabricError) -> FabricClientError {
    FabricClientError::Protocol(e)
}

pub fn default_limits() -> MessageSizeLimits {
    default_message_limits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_capabilities_are_restricted() {
        let caps = legacy_worker_capabilities();
        assert!(caps.legacy_v1_chat_only);
        assert!(!caps.supports_leases);
        assert!(!caps.supports_cancellation);
    }
}
