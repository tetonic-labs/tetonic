//! Legacy `/v1/chat` adapter — maps protocol job envelopes to transport-v1 JSON (M5-1).

use chrono::{Duration, Utc};
use tetonic_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId};
use tetonic_domain::workspace::ContentDigest;
use tetonic_fabric_protocol::{
    to_canonical_json, validate_job_envelope, IdempotencyKey, JobDeadlines, JobEnvelope, JobKind,
    JobRequirements, OutputLimits, ResourceLimits, VerificationPolicyReference,
    VersionedJobPayload,
};
use tetonic_inference::{
    DataClass, DisclosureTier, FabricJob, JobPriority, Message, SampleOptions, ToolSchema,
};

use crate::FabricClientError;

/// Parameters for building a protocol infer job before legacy transport adaptation.
pub struct LegacyInferParams {
    pub job_id: String,
    pub attempt_id: String,
    pub data_class: DataClass,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
}

/// Build a protocol `JobEnvelope` for legacy `/v1/chat` inference dispatch.
pub fn build_infer_job_envelope(
    params: &LegacyInferParams,
    messages: &[Message],
    model: &str,
) -> Result<JobEnvelope, FabricClientError> {
    let input_digest = legacy_input_digest(messages)?;
    let run_id = RunId::new(params.run_id.as_deref().unwrap_or(&params.job_id));
    let task_id = TaskId::new(params.task_id.as_deref().unwrap_or(&params.attempt_id));
    let envelope = JobEnvelope {
        job_id: JobId::new(&params.job_id),
        run_id,
        task_id,
        task_version: 1,
        attempt_id: AttemptId::new(&params.attempt_id),
        idempotency_key: IdempotencyKey(format!("{}:{}", params.attempt_id, params.job_id)),
        lease_id: LeaseId::new(&params.attempt_id),
        lease_epoch: 1,
        job_kind: JobKind::Infer,
        input_digest,
        workspace_version: None,
        input_artifacts: vec![],
        data_class: params.data_class,
        required_capabilities: JobRequirements::default(),
        resource_limits: ResourceLimits::default(),
        deadlines: JobDeadlines::from_execution(Utc::now() + Duration::hours(1), 3600),
        output_limits: OutputLimits {
            max_bytes: 8 * 1024 * 1024,
            max_artifacts: 0,
            max_artifact_size: 0,
        },
        verification_policy: VerificationPolicyReference::default(),
        payload: VersionedJobPayload::V1Infer(serde_json::json!({
            "model": model,
            "messages": messages,
        })),
    };
    validate_job_envelope(&envelope).map_err(FabricClientError::Protocol)?;
    Ok(envelope)
}

/// Convert a legacy `/v1/chat` transport job into a protocol envelope (worker ingress).
pub fn fabric_job_to_envelope(job: &FabricJob) -> Result<JobEnvelope, FabricClientError> {
    let attempt_id = job.attempt_id.as_deref().unwrap_or(&job.job_id).to_string();
    build_infer_job_envelope(
        &LegacyInferParams {
            job_id: job.job_id.clone(),
            attempt_id,
            data_class: job.data_class,
            run_id: job.session_id.clone(),
            task_id: Some(format!("{}:{}", job.agent_id, job.step_index)),
        },
        &job.messages,
        &job.model,
    )
}

fn legacy_input_digest(messages: &[Message]) -> Result<ContentDigest, FabricClientError> {
    let value = serde_json::to_value(messages).map_err(|e| {
        FabricClientError::Protocol(tetonic_fabric_protocol::FabricError {
            code: tetonic_fabric_protocol::FabricErrorCode::InvalidEnvelope,
            message: format!("canonical input digest failed: {e}"),
            details: None,
        })
    })?;
    let bytes = to_canonical_json(&value).map_err(|e| {
        FabricClientError::Protocol(tetonic_fabric_protocol::FabricError {
            code: tetonic_fabric_protocol::FabricErrorCode::InvalidEnvelope,
            message: format!("canonical input digest failed: {e}"),
            details: None,
        })
    })?;
    Ok(ContentDigest::new(format!(
        "canonical:fnv1a64:{}",
        fnv1a_hex(&bytes)
    )))
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Translate an `Infer` job envelope into legacy `/v1/chat` body JSON.
#[allow(clippy::too_many_arguments)]
pub fn infer_job_to_legacy_chat(
    job: &JobEnvelope,
    estate_id: &str,
    messages: Vec<Message>,
    tools: Vec<ToolSchema>,
    model: &str,
    policy_epoch: u64,
    session_id: Option<&str>,
    agent_id: &str,
    step_index: u32,
    options: SampleOptions,
    disclosure_tier: DisclosureTier,
    turn_affinity: Option<String>,
) -> Result<FabricJob, FabricClientError> {
    if job.job_kind != JobKind::Infer {
        return Err(FabricClientError::Protocol(
            tetonic_fabric_protocol::FabricError {
                code: tetonic_fabric_protocol::FabricErrorCode::UnsupportedJobKind,
                message: "legacy adapter supports Infer only".into(),
                details: None,
            },
        ));
    }
    Ok(FabricJob {
        job_id: job.job_id.0.clone(),
        attempt_id: Some(job.attempt_id.0.clone()),
        estate_id: estate_id.to_string(),
        session_id: session_id.map(String::from),
        agent_id: agent_id.to_string(),
        step_index,
        model: model.to_string(),
        tier: None,
        messages,
        tools,
        options,
        priority: JobPriority::OwnerInteractive,
        data_class: job.data_class,
        disclosure_tier,
        audit_envelope: None,
        circle_id: None,
        consumer_peer_id: None,
        policy_epoch,
        turn_affinity,
    })
}

/// Legacy workers cannot enforce cancellation — expose limitation to scheduling.
pub fn legacy_cancellation_supported() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_inference::Message;

    #[test]
    fn legacy_v1_chat_inference() {
        let messages = vec![Message::user("hello")];
        let params = LegacyInferParams {
            job_id: "job_1".into(),
            attempt_id: "att_1".into(),
            data_class: DataClass::RepositorySource,
            run_id: Some("sess_1".into()),
            task_id: None,
        };
        let envelope = build_infer_job_envelope(&params, &messages, "qwen").unwrap();
        let job = infer_job_to_legacy_chat(
            &envelope,
            "estate_1",
            messages,
            vec![],
            "qwen",
            3,
            Some("sess_1"),
            "agent_1",
            2,
            SampleOptions {
                stream: Some(true),
                ..Default::default()
            },
            DisclosureTier::Auditable,
            None,
        )
        .unwrap();
        assert_eq!(job.job_id, "job_1");
        assert_eq!(job.attempt_id.as_deref(), Some("att_1"));
        assert_eq!(job.model, "qwen");
        assert_eq!(job.session_id.as_deref(), Some("sess_1"));
        let json = serde_json::to_value(&job).unwrap();
        assert!(json.get("messages").is_some());
        assert_eq!(
            json.get("estate_id").and_then(|v| v.as_str()),
            Some("estate_1")
        );
    }

    #[test]
    fn legacy_worker_cancellation_limitation() {
        use tetonic_fabric_protocol::WorkerCapabilityAdvertisement;
        assert!(!legacy_cancellation_supported());
        let caps = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
        assert!(!caps.supports_cancellation);
    }

    #[test]
    fn non_infer_job_rejected_by_legacy_adapter() {
        let mut envelope = build_infer_job_envelope(
            &LegacyInferParams {
                job_id: "j".into(),
                attempt_id: "a".into(),
                data_class: DataClass::RepositorySource,
                run_id: None,
                task_id: None,
            },
            &[],
            "m",
        )
        .unwrap();
        envelope.job_kind = JobKind::Embed;
        let err = infer_job_to_legacy_chat(
            &envelope,
            "e",
            vec![],
            vec![],
            "m",
            0,
            None,
            "agent",
            0,
            SampleOptions::default(),
            DisclosureTier::Auditable,
            None,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            FabricClientError::Protocol(tetonic_fabric_protocol::FabricError {
                code: tetonic_fabric_protocol::FabricErrorCode::UnsupportedJobKind,
                ..
            })
        ));
    }
}
