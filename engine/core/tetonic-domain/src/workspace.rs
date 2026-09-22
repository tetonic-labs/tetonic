//! Workspace versioning and transaction domain types (M2-4).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::classify::DataClass;
use crate::ids::{AttemptId, TaskId, TransactionId};

/// Stable repository identity (not display-path alone).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub String);

impl RepositoryId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceVersionScheme {
    Git,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommitHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub String);

impl ContentDigest {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

/// Workspace-relative path (forward slashes, no leading slash).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkspacePath(pub String);

impl WorkspacePath {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into().replace('\\', "/"))
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Captured workspace state at transaction begin (M2-4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceVersion {
    pub repository_id: RepositoryId,
    pub version_scheme: WorkspaceVersionScheme,
    pub git_head: Option<CommitHash>,
    pub dirty_state_digest: ContentDigest,
    pub tracked_state_digest: ContentDigest,
    pub relevant_path_digests: BTreeMap<WorkspacePath, ContentDigest>,
    pub index_generation: Option<u64>,
}

impl WorkspaceVersion {
    /// Compact fingerprint for capability binding and audit.
    pub fn state_fingerprint(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.repository_id.0,
            self.version_scheme as u8,
            self.dirty_state_digest.0,
            self.tracked_state_digest.0
        )
    }
}

impl fmt::Display for WorkspaceVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state_fingerprint())
    }
}

/// How a workspace is materialized for execution (M1 seam).
///
/// This is a **locator**, not identity. Workspace identity is `WorkspaceVersion`
/// (`RepositoryId` + digests) per INV-WS-001. V3 materializes workspaces on the
/// coordinator's local filesystem only; a remote materialization would be a new
/// variant plus a transport protocol, not a reinterpretation of `root`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBinding {
    LocalFilesystem { root: std::path::PathBuf },
}

impl WorkspaceBinding {
    pub fn local(root: impl Into<std::path::PathBuf>) -> Self {
        Self::LocalFilesystem { root: root.into() }
    }

    /// Host path this workspace is materialized at. Callers must not treat the
    /// returned path as workspace identity.
    pub fn root(&self) -> &std::path::Path {
        match self {
            Self::LocalFilesystem { root } => root.as_path(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Created,
    Staging,
    Staged,
    Verifying,
    ReadyToCommit,
    Committing,
    Committed,
    Aborted,
    Rejected,
    Conflict,
    RecoveryRequired,
}

impl TransactionState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TransactionState::Committed
                | TransactionState::Aborted
                | TransactionState::Rejected
                | TransactionState::Conflict
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    ContentChanged,
    HeadChanged,
    FileAppeared,
    FileRemoved,
    SymlinkRetargeted,
    ModeChanged,
    WorkspaceIdentityChanged,
    NewFileDestinationOccupied,
    DeleteTargetChanged,
    RenameDestinationOccupied,
    PermissionChanged,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedState {
    pub digest: Option<ContentDigest>,
    pub exists: bool,
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualState {
    pub digest: Option<ContentDigest>,
    pub exists: bool,
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConflict {
    pub path: WorkspacePath,
    pub expected: ExpectedState,
    pub actual: ActualState,
    pub conflict_kind: ConflictKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StagedOperationKind {
    Create,
    Replace,
    Edit,
    Delete,
    Rename,
    ModeChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedOperation {
    pub kind: StagedOperationKind,
    pub path: WorkspacePath,
    pub destination: Option<WorkspacePath>,
    pub base_digest: Option<ContentDigest>,
    pub new_digest: Option<ContentDigest>,
    pub new_content_path: Option<String>,
    /// Original permission bits when staging a mode change (Unix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_mode: Option<u32>,
    /// Target permission bits for `ModeChange` operations (Unix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionPreview {
    pub transaction_id: TransactionId,
    pub base_version: WorkspaceVersion,
    pub patch_digest: ContentDigest,
    pub operations: Vec<StagedOperation>,
    pub unified_diff: String,
    pub created_paths: Vec<WorkspacePath>,
    pub deleted_paths: Vec<WorkspacePath>,
    pub modified_paths: Vec<WorkspacePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRecord {
    pub command: String,
    pub sandbox_policy: String,
    pub workspace_version: WorkspaceVersion,
    pub exit_status: Option<i32>,
    pub output_digest: ContentDigest,
    pub success: bool,
    pub unexpected_mutations: Vec<WorkspacePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchApproval {
    pub transaction_id: TransactionId,
    pub patch_digest: ContentDigest,
    pub base_version: WorkspaceVersion,
    pub verification_required: bool,
    pub verification_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitResult {
    pub transaction_id: TransactionId,
    pub base_version: WorkspaceVersion,
    pub result_version: WorkspaceVersion,
    pub patch_digest: ContentDigest,
    pub artifact: TransactionArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionArtifact {
    pub transaction_id: TransactionId,
    pub base_workspace_version: WorkspaceVersion,
    pub result_workspace_version: WorkspaceVersion,
    pub patch_artifact_id: crate::ids::ArtifactId,
    pub verification_artifact_id: Option<crate::ids::ArtifactId>,
    pub task_id: Option<TaskId>,
    pub attempt_id: Option<AttemptId>,
    pub data_class: DataClass,
    pub commit_succeeded: bool,
}
