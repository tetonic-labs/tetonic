use crate::manifest::EvaluationManifest;
use crate::result::{ResourceUsage, TimingBreakdown};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Provides the immutable snapshot of a repository for a given scenario.
#[async_trait]
pub trait CorpusProvider: Send + Sync {
    /// Mounts the required snapshot and returns the path to the temporary workspace.
    async fn mount_snapshot(&self, snapshot_id: &str) -> Result<PathBuf>;

    /// Cleans up the mounted snapshot.
    async fn unmount_snapshot(&self, workspace: &Path) -> Result<()>;
}

/// Provides the execution boundaries for the agent and the evaluation graders.
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    /// Applies the evaluation limits (time, tokens, memory) to the workspace environment.
    async fn setup_sandbox(&self, workspace: &Path, manifest: &EvaluationManifest) -> Result<()>;
}

/// The result returned by the agent orchestrator after attempting a task.
pub struct AgentExecutionResult {
    pub patch_digest: Option<String>,
    pub output_digest: Option<String>,
    pub resource_usage: ResourceUsage,
    pub timing: TimingBreakdown,
    pub touched_files: Vec<String>,
    /// Concatenated Infer message bodies as the *model* saw them (post-scan).
    pub outbound_texts: Vec<String>,
    /// True when a secret detector still matches those outbound bodies.
    pub found_secrets: bool,
    /// Terminal harness status (Incomplete/Failed never count as pass).
    pub run_status: crate::result::RunStatus,
    pub failure_classification: Option<String>,
}

/// Executes the core agent loop for the evaluation task.
#[async_trait(?Send)]
pub trait AgentOrchestrator: Send + Sync {
    /// Runs the agent against the provided workspace and returns the execution result.
    async fn run_task(
        &self,
        workspace: &Path,
        manifest: &EvaluationManifest,
    ) -> Result<AgentExecutionResult>;
}
