//! Envelope and job validation (M5-1).

use chrono::{DateTime, Utc};

use tetonic_domain::ids::WorkerId;

use crate::{
    bounds::{IMMUTABLE_SECURITY_FEATURES, MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION},
    CancellationRequest, FabricEnvelope, FabricError, FabricErrorCode, Heartbeat, JobEnvelope,
    JobKind, LeaseMessage, LifecycleContext, MessageSizeLimits, ProtocolVersion,
};

pub fn validate_protocol_version(version: &ProtocolVersion) -> Result<(), FabricError> {
    if version.0 < MIN_SUPPORTED_VERSION || version.0 > MAX_SUPPORTED_VERSION {
        return Err(FabricError {
            code: FabricErrorCode::UnsupportedProtocolVersion,
            message: format!("unsupported protocol version {}", version.0),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_mandatory_envelope_fields<T>(
    envelope: &FabricEnvelope<T>,
) -> Result<(), FabricError> {
    validate_protocol_version(&envelope.protocol_version)?;
    if envelope.message_id.0.trim().is_empty() {
        return Err(missing_field("message_id"));
    }
    if envelope.coordinator_id.0.trim().is_empty() {
        return Err(missing_field("coordinator_id"));
    }
    if envelope.worker_id.0.trim().is_empty() {
        return Err(missing_field("worker_id"));
    }
    if envelope.trace_context.trace_id.trim().is_empty() {
        return Err(missing_field("trace_context.trace_id"));
    }
    Ok(())
}

pub fn validate_channel_identity(
    channel_worker_id: &WorkerId,
    envelope_worker_id: &WorkerId,
) -> Result<(), FabricError> {
    if channel_worker_id != envelope_worker_id {
        return Err(FabricError {
            code: FabricErrorCode::IdentityMismatch,
            message: format!(
                "channel worker {} != envelope worker {}",
                channel_worker_id.0, envelope_worker_id.0
            ),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_revocation_epoch(
    sender_epoch: u64,
    known_minimum_epoch: u64,
) -> Result<(), FabricError> {
    if sender_epoch < known_minimum_epoch {
        return Err(FabricError {
            code: FabricErrorCode::StaleRevocationEpoch,
            message: format!(
                "revocation epoch {sender_epoch} stale (minimum {known_minimum_epoch})"
            ),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_revoked(identity_revoked: bool) -> Result<(), FabricError> {
    if identity_revoked {
        return Err(FabricError {
            code: FabricErrorCode::RevokedIdentity,
            message: "identity is revoked".into(),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_clock_skew(
    sent_at: DateTime<Utc>,
    now: DateTime<Utc>,
    max_skew_secs: i64,
) -> Result<(), FabricError> {
    let skew = (sent_at - now).num_seconds().abs();
    if skew > max_skew_secs {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: format!("clock skew {skew}s exceeds max {max_skew_secs}s"),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_job_envelope(job: &JobEnvelope) -> Result<(), FabricError> {
    if job.input_digest.0.trim().is_empty() {
        return Err(missing_field("input_digest"));
    }
    if job.idempotency_key.0.trim().is_empty() {
        return Err(missing_field("idempotency_key"));
    }
    if job.lease_id.0.trim().is_empty() {
        return Err(missing_field("lease_id"));
    }
    for artifact in &job.input_artifacts {
        if artifact.artifact_id.trim().is_empty() {
            return Err(missing_field("input_artifacts[].artifact_id"));
        }
        if artifact.digest.0.trim().is_empty() {
            return Err(missing_field("input_artifacts[].digest"));
        }
    }
    validate_job_kind(&job.job_kind)?;
    Ok(())
}

pub fn validate_job_kind(kind: &JobKind) -> Result<(), FabricError> {
    match kind {
        JobKind::Infer
        | JobKind::Embed
        | JobKind::AnalyzeCode
        | JobKind::IndexShard
        | JobKind::TestShard
        | JobKind::ReviewArtifact => Ok(()),
    }
}

pub fn parse_job_kind_str(raw: &str) -> Result<JobKind, FabricError> {
    match raw {
        "infer" | "Infer" => Ok(JobKind::Infer),
        "embed" | "Embed" => Ok(JobKind::Embed),
        "analyze_code" | "AnalyzeCode" => Ok(JobKind::AnalyzeCode),
        "index_shard" | "IndexShard" => Ok(JobKind::IndexShard),
        "test_shard" | "TestShard" => Ok(JobKind::TestShard),
        "review_artifact" | "ReviewArtifact" => Ok(JobKind::ReviewArtifact),
        other => Err(FabricError {
            code: FabricErrorCode::UnsupportedJobKind,
            message: format!("unknown job kind {other}"),
            details: None,
        }),
    }
}

pub fn validate_lease_epoch(current_epoch: u64, proof_epoch: u64) -> Result<(), FabricError> {
    if proof_epoch != current_epoch {
        return Err(FabricError {
            code: FabricErrorCode::StaleLeaseEpoch,
            message: format!("stale lease epoch {proof_epoch} (current {current_epoch})"),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_lease_current(lease: &LeaseMessage, now: DateTime<Utc>) -> Result<(), FabricError> {
    if now > lease.expires_at {
        return Err(FabricError {
            code: FabricErrorCode::InvalidLease,
            message: "lease expired".into(),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_lifecycle_context(
    ctx: &LifecycleContext,
    channel_worker: &WorkerId,
) -> Result<(), FabricError> {
    validate_protocol_version(&ctx.protocol_version)?;
    validate_channel_identity(channel_worker, &ctx.worker_id)?;
    if ctx.idempotency_key.0.trim().is_empty() {
        return Err(missing_field("idempotency_key"));
    }
    if ctx.lease_id.0.trim().is_empty() {
        return Err(missing_field("lease_id"));
    }
    Ok(())
}

pub fn validate_result_context(
    ctx: &LifecycleContext,
    channel_worker: &WorkerId,
    current_lease_epoch: u64,
    canceled: bool,
) -> Result<(), FabricError> {
    validate_lifecycle_context(ctx, channel_worker)?;
    validate_lease_epoch(current_lease_epoch, ctx.lease_epoch)?;
    if canceled {
        return Err(FabricError {
            code: FabricErrorCode::InvalidLease,
            message: "attempt canceled".into(),
            details: None,
        });
    }
    Ok(())
}

/// Accept a result from a superseded lease epoch for audit only (not authoritative).
pub fn validate_result_context_audit(
    ctx: &LifecycleContext,
    channel_worker: &WorkerId,
    current_lease_epoch: u64,
) -> Result<(), FabricError> {
    validate_lifecycle_context(ctx, channel_worker)?;
    if ctx.lease_epoch > current_lease_epoch {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "future lease epoch".into(),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_heartbeat(
    hb: &Heartbeat,
    channel_worker: &WorkerId,
    limits: &MessageSizeLimits,
) -> Result<(), FabricError> {
    validate_lifecycle_context(&hb.context, channel_worker)?;
    if hb.attempt_state.len() > 256 {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: "heartbeat attempt_state too large".into(),
            details: None,
        });
    }
    if hb.progress_marker.as_ref().is_some_and(|m| m.len() > 4096) {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: "heartbeat progress_marker too large".into(),
            details: None,
        });
    }
    if hb.worker_health_status.len() > 256 {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: "heartbeat worker_health_status too large".into(),
            details: None,
        });
    }
    let summary_len = hb
        .resource_usage_summary
        .as_ref()
        .map(|v| serde_json::to_vec(v).map(|b| b.len()).unwrap_or(0))
        .unwrap_or(0);
    if summary_len as u32 > limits.max_payload_bytes / 4 {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: "heartbeat resource summary too large".into(),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_cancellation(cancel: &CancellationRequest) -> Result<(), FabricError> {
    if cancel.reason.len() > 4096 {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: "cancellation reason too large".into(),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_security_features(features: &[String]) -> Result<(), FabricError> {
    for required in IMMUTABLE_SECURITY_FEATURES {
        if !features.iter().any(|f| f == required) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: format!("missing mandatory security feature {required}"),
                details: None,
            });
        }
    }
    Ok(())
}

pub fn bound_fabric_error_message(msg: &str, limits: &MessageSizeLimits) -> String {
    crate::bounds::bound_error_message(msg, limits)
}

fn missing_field(field: &str) -> FabricError {
    FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: format!("missing mandatory security field {field}"),
        details: None,
    }
}
