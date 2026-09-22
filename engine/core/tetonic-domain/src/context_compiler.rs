//! Context compiler trait for the generic loop. Not ReadAuthority.

use async_trait::async_trait;

use serde::Serialize;

use crate::classify::DataClass;
use crate::ids::{RunId, SessionId, TaskId};
use crate::workspace::WorkspaceVersion;

#[derive(Debug, Clone)]
pub struct ContextCompileRequest {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub objective: String,
    pub workspace_version: WorkspaceVersion,
    pub data_class_ceiling: DataClass,
}

/// Caller identity comes from the runtime, never model-supplied tool arguments.
#[derive(Debug, Clone)]
pub struct ContextExpansionRequest {
    pub handle_id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub current_workspace_fp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompiledEvidence {
    pub evidence_id: String,
    pub repository_path: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct CompiledContext {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub workspace_version: WorkspaceVersion,
    pub data_class: DataClass,
    pub evidence: Vec<CompiledEvidence>,
}

/// Pack/runtime compiler. Implementations must not read the workspace themselves.
#[async_trait]
pub trait ContextCompiler: Send + Sync {
    /// Revoke retained expansion handles for a session. Stateless compilers
    /// need no cleanup. Stateful implementations must also reject in-flight
    /// expansion results whose authority was revoked during retrieval.
    fn invalidate_session(&self, _session: &SessionId) -> Result<(), String> {
        Ok(())
    }
    async fn compile(&self, req: ContextCompileRequest) -> Result<CompiledContext, String>;
    async fn expand(
        &self,
        request: &ContextExpansionRequest,
    ) -> Result<Vec<CompiledEvidence>, String> {
        let _ = request;
        Err("context expansion is not supported by this compiler".into())
    }
}
