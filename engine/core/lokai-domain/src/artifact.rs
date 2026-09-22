use crate::classify::DataClass;
use crate::ids::{ArtifactId, AttemptId, RunId, TaskId, WorkerId, WorkspaceVersion};
use crate::result_integrity::ArtifactOrigin;
use crate::workspace::ContentDigest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Patch,
    FileSnapshot,
    ContextPack,
    AnalysisReport,
    TestReport,
    VerificationReport,
    BuildLog,
    ModelResponse,
    SymbolMap,
    Diff,
    ReviewReport,
    DiagnosticBundle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub artifact_id: ArtifactId,
    pub kind: ArtifactKind,
    pub schema_version: u32,
    pub producer_run_id: RunId,
    pub producer_task_id: TaskId,
    pub producer_attempt_id: AttemptId,
    pub worker_id: Option<WorkerId>,
    pub content_digest: ContentDigest,
    pub size_bytes: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub data_class: DataClass,
    pub storage_location: ArtifactLocation,
    pub lifecycle_state: ArtifactState,
    pub verification_state: VerificationState,
    #[serde(default = "default_local_origin")]
    pub origin: ArtifactOrigin,
    pub created_at: DateTime<Utc>,
    pub retention_policy: RetentionPolicy,
}

fn default_local_origin() -> ArtifactOrigin {
    ArtifactOrigin::Local
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Declared,
    Writing,
    Sealed,
    Verified,
    Accepted,
    Rejected,
    Abandoned,
    Expired,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Unverified,
    StructurallyValid,
    LocallyVerified,
    IndependentlyVerified,
    FailedVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    Ephemeral,
    UntilRunCompletes,
    ProjectHistory,
    SecurityAudit,
    UserPinned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactLocation {
    LocalFile(PathBuf),
    Remote(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDeclaration {
    pub kind: ArtifactKind,
    pub producer_run_id: RunId,
    pub producer_task_id: TaskId,
    pub producer_attempt_id: AttemptId,
    pub worker_id: Option<WorkerId>,
    pub workspace_version: Option<WorkspaceVersion>,
    pub data_class: DataClass,
    pub retention_policy: RetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceTrustLabel {
    /// Local sealed artifact not yet accepted on a run.
    LocalSealed,
    /// Remote / quarantined content — must not be trusted until Accepted.
    RemoteUnverified,
    /// Accepted on a run (AcceptArtifact / mark_accepted).
    Accepted,
}

/// Thin reconstruction of how a sealed local (or quarantined remote) artifact
/// was produced — enough to audit one accepted patch without a CAS graph (R24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactProvenanceBundle {
    pub artifact_id: ArtifactId,
    pub kind: ArtifactKind,
    pub producer_run_id: RunId,
    pub producer_task_id: TaskId,
    pub producer_attempt_id: AttemptId,
    pub worker_id: Option<WorkerId>,
    pub content_digest: ContentDigest,
    pub data_class: DataClass,
    pub origin: ArtifactOrigin,
    pub lifecycle_state: ArtifactState,
    pub verification_state: VerificationState,
    pub workspace_version: Option<WorkspaceVersion>,
    pub trust_label: ProvenanceTrustLabel,
}

impl ArtifactProvenanceBundle {
    pub fn from_metadata(meta: &ArtifactMetadata) -> Self {
        let trust_label = provenance_trust_label(meta);
        Self {
            artifact_id: meta.artifact_id.clone(),
            kind: meta.kind.clone(),
            producer_run_id: meta.producer_run_id.clone(),
            producer_task_id: meta.producer_task_id.clone(),
            producer_attempt_id: meta.producer_attempt_id.clone(),
            worker_id: meta.worker_id.clone(),
            content_digest: meta.content_digest.clone(),
            data_class: meta.data_class,
            origin: meta.origin,
            lifecycle_state: meta.lifecycle_state.clone(),
            verification_state: meta.verification_state.clone(),
            workspace_version: meta.workspace_version.clone(),
            trust_label,
        }
    }
}

/// Remote quarantined artifacts stay unverified until lifecycle is Accepted.
pub fn provenance_trust_label(meta: &ArtifactMetadata) -> ProvenanceTrustLabel {
    if matches!(meta.lifecycle_state, ArtifactState::Accepted) {
        return ProvenanceTrustLabel::Accepted;
    }
    match meta.origin {
        ArtifactOrigin::Remote => ProvenanceTrustLabel::RemoteUnverified,
        ArtifactOrigin::Local => ProvenanceTrustLabel::LocalSealed,
    }
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("not found: {0}")]
    NotFound(ArtifactId),
    #[error("size limit exceeded: {0}")]
    SizeLimitExceeded(u64),
    #[error("digest mismatch: expected {expected:?}, actual {actual:?}")]
    DigestMismatch {
        expected: ContentDigest,
        actual: ContentDigest,
    },
    #[error("invalid state transition: {0}")]
    InvalidStateTransition(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("artifact storage quota exceeded: usage {usage} max {max}")]
    QuotaExceeded { usage: u64, max: u64 },
    #[error("schema validation failed: {0}")]
    SchemaValidation(String),
    #[error("quarantine rejected: {0}")]
    QuarantineRejected(String),
    #[error("artifact still quarantined / unverified")]
    StillQuarantined,
}

#[async_trait::async_trait]
pub trait ArtifactWriter: Send + Sync {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), ArtifactError>;
    async fn seal(self: Box<Self>) -> Result<ArtifactMetadata, ArtifactError>;
    async fn abandon(self: Box<Self>) -> Result<(), ArtifactError>;
}

#[async_trait::async_trait]
pub trait ArtifactReader: Send + Sync {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, ArtifactError>;
}

#[async_trait::async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn begin_write(
        &self,
        declaration: ArtifactDeclaration,
    ) -> Result<Box<dyn ArtifactWriter>, ArtifactError>;

    async fn open(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Box<dyn ArtifactReader>, ArtifactError>;

    async fn metadata(&self, artifact_id: &ArtifactId) -> Result<ArtifactMetadata, ArtifactError>;

    /// Verify and publish Accepted metadata before the run acceptance receipt.
    /// Failure must prevent recording that receipt. Artifact acceptance alone
    /// does not establish a winning run result; the run owner records that next.
    async fn mark_accepted(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<ArtifactMetadata, ArtifactError>;

    async fn delete(&self, artifact_id: &ArtifactId) -> Result<(), ArtifactError>;
}
