//! Envelope validation before artifact ingestion (M5-4).

use chrono::{DateTime, Utc};
use lokai_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId, WorkerId, WorkspaceVersion};
use lokai_domain::workspace::ContentDigest;
use lokai_domain::ResultDisposition;

use crate::result::{ResultEnvelope, RESULT_ENVELOPE_VERSION};
use crate::result_crypto::{verify_result_signature, verifying_key_from_bytes};
use crate::result_keys::ResultSigningKeyRegistry;
use crate::{
    validate_channel_identity, validate_protocol_version, validate_revocation_epoch,
    validate_revoked, FabricError, FabricErrorCode, MessageSizeLimits, OutputLimits,
};

/// Expected binding for an in-flight attempt the coordinator believes is current.
#[derive(Clone, Debug)]
pub struct ExpectedResultBinding {
    pub channel_worker_id: WorkerId,
    pub coordinator_id: String,
    pub run_id: RunId,
    pub job_id: JobId,
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub input_digest: ContentDigest,
    pub workspace_version: Option<WorkspaceVersion>,
    pub worker_revoked: bool,
    pub attempt_canceled: bool,
    pub attempt_superseded: bool,
    pub another_attempt_won: bool,
    pub run_active: bool,
    pub task_active: bool,
    pub known_revocation_epoch: u64,
    pub output_limits: OutputLimits,
}

#[derive(Clone, Debug)]
pub struct EnvelopeValidationOutcome {
    pub disposition: ResultDisposition,
    pub reason: String,
}

impl EnvelopeValidationOutcome {
    pub fn ok() -> Self {
        Self {
            disposition: ResultDisposition::Quarantined,
            reason: "envelope structurally valid; artifacts require quarantine".into(),
        }
    }

    pub fn reject(disposition: ResultDisposition, reason: impl Into<String>) -> Self {
        Self {
            disposition,
            reason: reason.into(),
        }
    }
}

pub fn validate_result_envelope(
    envelope: &ResultEnvelope,
    expected: &ExpectedResultBinding,
    keys: &ResultSigningKeyRegistry,
    limits: &MessageSizeLimits,
    now: DateTime<Utc>,
) -> Result<EnvelopeValidationOutcome, FabricError> {
    validate_protocol_version(&envelope.body.protocol_version)?;
    if envelope.body.result_envelope_version != RESULT_ENVELOPE_VERSION {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedSchema,
            format!(
                "unsupported result_envelope_version {}",
                envelope.body.result_envelope_version
            ),
        ));
    }

    if let Err(e) = validate_channel_identity(&expected.channel_worker_id, &envelope.body.worker_id)
    {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedIdentityMismatch,
            e.message,
        ));
    }

    if envelope.body.coordinator_id.0 != expected.coordinator_id {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedIdentityMismatch,
            "result coordinator_id mismatch",
        ));
    }

    if expected.worker_revoked {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedRevokedWorker,
            "worker revoked",
        ));
    }
    if let Err(e) = validate_revoked(expected.worker_revoked) {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedRevokedWorker,
            e.message,
        ));
    }

    let key_rec = match keys.lookup(&envelope.body.worker_id, &envelope.body.worker_key_id, now) {
        Ok(r) => r,
        Err(e) => {
            let disposition = if e.code == FabricErrorCode::RevokedIdentity {
                ResultDisposition::RejectedRevokedWorker
            } else {
                ResultDisposition::RejectedInvalidSignature
            };
            return Ok(EnvelopeValidationOutcome::reject(disposition, e.message));
        }
    };
    let vk = verifying_key_from_bytes(&key_rec.public_key)?;
    if let Err(e) = verify_result_signature(envelope, &vk) {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedInvalidSignature,
            e.message,
        ));
    }

    if let Err(e) = validate_revocation_epoch(
        envelope.body.revocation_epoch,
        expected.known_revocation_epoch,
    ) {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedRevokedWorker,
            e.message,
        ));
    }

    if envelope.body.run_id != expected.run_id
        || envelope.body.job_id != expected.job_id
        || envelope.body.task_id != expected.task_id
        || envelope.body.attempt_id != expected.attempt_id
    {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedStaleAttempt,
            "result bound to different run/task/attempt",
        ));
    }

    if envelope.body.task_version != expected.task_version {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedStaleAttempt,
            "task version mismatch",
        ));
    }

    if envelope.body.lease_id != expected.lease_id
        || envelope.body.lease_epoch != expected.lease_epoch
    {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedStaleAttempt,
            "lease id/epoch mismatch",
        ));
    }

    if !expected.run_active || !expected.task_active {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedCanceled,
            "run or task not active",
        ));
    }

    if expected.attempt_canceled {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedCanceled,
            "attempt canceled",
        ));
    }

    if expected.attempt_superseded || expected.another_attempt_won {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::Superseded,
            "attempt superseded or another attempt already won",
        ));
    }

    if envelope.body.input_digest != expected.input_digest {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedDigestMismatch,
            "input digest mismatch",
        ));
    }

    if envelope.body.workspace_version != expected.workspace_version {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedStaleAttempt,
            "workspace version mismatch",
        ));
    }

    if envelope.body.result_digest.0.trim().is_empty() {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedDigestMismatch,
            "empty result digest",
        ));
    }

    let payload_len = serde_json::to_vec(&envelope.payload)
        .map(|b| b.len())
        .unwrap_or(0);
    if payload_len as u64 > expected.output_limits.max_bytes
        || payload_len as u32 > limits.max_payload_bytes
    {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedOversized,
            "result payload exceeds size limits",
        ));
    }

    if envelope.body.artifacts.len() as u32 > expected.output_limits.max_artifacts {
        return Ok(EnvelopeValidationOutcome::reject(
            ResultDisposition::RejectedOversized,
            "too many artifacts",
        ));
    }

    for art in &envelope.body.artifacts {
        if art.digest.0.trim().is_empty() || art.artifact_id.trim().is_empty() {
            return Ok(EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedDigestMismatch,
                "artifact missing id or digest",
            ));
        }
        if art.size_bytes > expected.output_limits.max_artifact_size {
            return Ok(EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedOversized,
                format!("artifact {} exceeds size limit", art.artifact_id),
            ));
        }
    }

    Ok(EnvelopeValidationOutcome::ok())
}
