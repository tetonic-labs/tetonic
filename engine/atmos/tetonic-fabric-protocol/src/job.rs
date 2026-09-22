use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::classify::DataClass;
use tetonic_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId, WorkspaceVersion};
use tetonic_domain::workspace::ContentDigest;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyKey(pub String);

/// Digest-bound input artifact reference. Placement revalidates these immediately
/// before dispatch so a stale artifact selection cannot authorize a different payload.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub artifact_id: String,
    pub digest: ContentDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct JobRequirements {
    pub tools: Vec<String>,
    pub environment: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    pub max_cpu_millis: Option<u64>,
    pub max_memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputLimits {
    pub max_bytes: u64,
    pub max_artifacts: u32,
    pub max_artifact_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationPolicyReference {
    pub require_verification: bool,
    pub policy_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    Infer,
    Embed,
    AnalyzeCode,
    IndexShard,
    TestShard,
    ReviewArtifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionedJobPayload {
    V1Infer(serde_json::Value),
    V1Embed(serde_json::Value),
    V1AnalyzeCode(serde_json::Value),
    V1IndexShard(serde_json::Value),
    V1TestShard(serde_json::Value),
    V1ReviewArtifact(serde_json::Value),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDeadlines {
    pub queue_deadline: DateTime<Utc>,
    pub start_deadline: DateTime<Utc>,
    pub execution_deadline: DateTime<Utc>,
    pub lease_duration_secs: u64,
}

impl JobDeadlines {
    pub fn from_execution(execution: DateTime<Utc>, lease_duration_secs: u64) -> Self {
        Self {
            queue_deadline: execution,
            start_deadline: execution,
            execution_deadline: execution,
            lease_duration_secs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobEnvelope {
    pub job_id: JobId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt_id: AttemptId,
    pub idempotency_key: IdempotencyKey,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub job_kind: JobKind,
    pub input_digest: ContentDigest,
    pub workspace_version: Option<WorkspaceVersion>,
    pub input_artifacts: Vec<ArtifactReference>,
    pub data_class: DataClass,
    pub required_capabilities: JobRequirements,
    pub resource_limits: ResourceLimits,
    pub deadlines: JobDeadlines,
    pub output_limits: OutputLimits,
    pub verification_policy: VerificationPolicyReference,
    pub payload: VersionedJobPayload,
}
