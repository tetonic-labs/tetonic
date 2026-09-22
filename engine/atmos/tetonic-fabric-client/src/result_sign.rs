//! Helpers to build and verify signed infer-result envelopes on the wire (M5-4).

use chrono::Utc;
use ed25519_dalek::SigningKey;
use tetonic_domain::ids::{KeyId, ResultId, WorkerId};
use tetonic_fabric_protocol::{
    digest_payload, sign_result_envelope, CoordinatorId, ExecutionSummary, JobEnvelope, JobKind,
    ProtocolVersion, ResultEnvelope, ResultStatus, SignedResultBody, WorkerLocalDurations,
    RESULT_ENVELOPE_VERSION,
};
use tetonic_inference::{FabricJobResult, JobStatus, Message};

use crate::FabricClientError;

pub fn key_id_from_public(pk: &[u8]) -> KeyId {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(pk);
    KeyId::new(format!("rsk_{}", hex::encode(&h[..8])))
}

#[allow(clippy::too_many_arguments)]
pub fn build_signed_chat_result(
    job: &JobEnvelope,
    coordinator_id: &str,
    worker_id: &str,
    key_id: &KeyId,
    signing_key: &SigningKey,
    message: Message,
    usage: tetonic_inference::GenUsageSerde,
    status: JobStatus,
    error: Option<String>,
) -> Result<(FabricJobResult, ResultEnvelope), FabricClientError> {
    build_signed_chat_result_timed(
        job,
        coordinator_id,
        worker_id,
        key_id,
        signing_key,
        message,
        usage,
        status,
        error,
        None,
        None,
    )
}

/// Like [`build_signed_chat_result`] with worker-local monotonic timing (M6-3).
#[allow(clippy::too_many_arguments)]
pub fn build_signed_chat_result_timed(
    job: &JobEnvelope,
    coordinator_id: &str,
    worker_id: &str,
    key_id: &KeyId,
    signing_key: &SigningKey,
    message: Message,
    usage: tetonic_inference::GenUsageSerde,
    status: JobStatus,
    error: Option<String>,
    duration_ms: Option<u64>,
    local: Option<WorkerLocalDurations>,
) -> Result<(FabricJobResult, ResultEnvelope), FabricClientError> {
    let result_status = match status {
        JobStatus::Ok => ResultStatus::Ok,
        JobStatus::Preempted => ResultStatus::Preempted,
        JobStatus::Canceled => ResultStatus::Canceled,
        JobStatus::Error | JobStatus::Denied => ResultStatus::Failed,
    };
    let payload = serde_json::json!({
        "message": message,
        "usage": usage,
        "status": format!("{:?}", status),
        "error": error,
    });
    let result_digest = digest_payload(&payload).map_err(FabricClientError::Protocol)?;
    let body = SignedResultBody {
        protocol_version: ProtocolVersion(1),
        result_envelope_version: RESULT_ENVELOPE_VERSION,
        result_id: ResultId::new(format!("res_{}", job.attempt_id.0)),
        worker_id: WorkerId::new(worker_id),
        worker_key_id: key_id.clone(),
        coordinator_id: CoordinatorId(coordinator_id.into()),
        run_id: job.run_id.clone(),
        job_id: job.job_id.clone(),
        task_id: job.task_id.clone(),
        task_version: job.task_version,
        attempt_id: job.attempt_id.clone(),
        lease_id: job.lease_id.clone(),
        lease_epoch: job.lease_epoch,
        idempotency_key: job.idempotency_key.clone(),
        job_kind: JobKind::Infer,
        input_digest: job.input_digest.clone(),
        workspace_version: job.workspace_version.clone(),
        result_status,
        result_digest,
        artifacts: vec![],
        execution_summary: ExecutionSummary {
            duration_ms,
            model: None,
            truncated: false,
            local,
            ..Default::default()
        },
        completed_at: Utc::now(),
        revocation_epoch: 0,
    };
    let envelope =
        sign_result_envelope(body, signing_key, payload).map_err(FabricClientError::Protocol)?;
    let envelope_value = serde_json::to_value(&envelope)
        .map_err(|e| FabricClientError::Http(format!("serialize result envelope: {e}")))?;
    let result = FabricJobResult {
        job_id: job.job_id.0.clone(),
        attempt_id: Some(job.attempt_id.0.clone()),
        message,
        usage,
        status,
        error,
        result_envelope: Some(envelope_value),
    };
    Ok((result, envelope))
}
