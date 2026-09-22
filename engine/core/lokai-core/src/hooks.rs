use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use crate::conversation::Conversation;

// Approval, spawn, and audit hooks wired by the host (CLI / daemon).

/// What the agent asks the host to approve before running a gated tool. The
/// host (e.g. the daemon) surfaces this to the user and resolves the
/// [`ApprovalHook`] future with the decision.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Correlates with the `tool_call` id in the audit trail / RPC events.
    pub call_id: String,
    /// Machine-facing approval kind (tool name).
    pub kind: String,
    pub tool: String,
    pub args: Value,
    /// Typed OS-confinement gaps predicted for this action (AUDIT H1-2).
    pub missing_controls: Vec<ConfinementWarning>,
    /// When true, remembered rules and `--allow-shell` must not auto-approve.
    pub user_approval_required: bool,
    /// Authoritative AttemptId that proposed the action.
    pub attempt_id: Option<String>,
}

/// A missing OS sandbox control, carried on the approval prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfinementWarning {
    /// Wire name, e.g. `network_denial`.
    pub control: String,
    /// Wire name, e.g. `high`.
    pub risk_level: String,
    pub reason: String,
}

impl Default for ApprovalRequest {
    fn default() -> Self {
        Self {
            call_id: String::new(),
            kind: String::new(),
            tool: String::new(),
            args: Value::Null,
            missing_controls: Vec::new(),
            user_approval_required: false,
            attempt_id: None,
        }
    }
}

/// Async approval gate. When set on the [`Agent`], tools that
/// [`lokai_domain::ToolHost::requires_user_approval`] reports true are routed
/// through it *before* executing when no action broker is present.
///
/// Without a hook the agent defers to the tool layer's own policy (`allow_shell`
/// on the capability host), so the CLI is unaffected.
pub type ApprovalHook =
    Arc<dyn Fn(ApprovalRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Optional staged-mutation abort installed by composition.
/// Missing hook is a no-op. Must not commit.
pub type AbortStaged = Arc<dyn Fn() + Send + Sync>;

/// Optional post-edit snapshot formatter. Missing hook appends nothing.
/// Composition supplies `lokai_tools::format_post_edit_snapshot`. Must not commit.
pub type PostEditSnapshot = Arc<dyn Fn(&lokai_domain::FileChange) -> String + Send + Sync>;

/// Optional workspace jail + size. Missing hook skips the size gate.
/// Composition resolves under root and returns the byte length. The loop must
/// not call `std::fs::metadata`.
pub type ResolveUnderRoot = Arc<dyn Fn(&Path, &str) -> Result<u64, ()> + Send + Sync>;

/// Optional workspace-version capture. The loop interprets `Result` per call site
/// (broker deny / compiled `None` / context `Failed`). Must not commit. Must not
/// take Session. Composition supplies `lokai_transaction::version::capture_workspace_version`.
pub type CaptureWorkspaceVersion = Arc<
    dyn Fn(&Path, &[lokai_domain::WorkspacePath]) -> Result<lokai_domain::WorkspaceVersion, String>
        + Send
        + Sync,
>;

/// In-loop specialist spawn (A13). The host runs the sub-agent turn and returns
/// a tool outcome for the parent model.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub role: String,
    pub task: String,
    pub parent_agent_id: String,
}

pub type SpawnHook = Box<
    dyn for<'a> Fn(
            SpawnRequest,
            &'a mut Conversation,
        ) -> Pin<Box<dyn Future<Output = lokai_domain::ToolOutcome> + Send + 'a>>
        + Send
        + Sync,
>;

/// Best-effort persistence hook for the audit trail. The engine calls this as
/// the loop runs; an implementation (e.g. an adapter over `lokai-memory`) writes
/// it to disk. Every method MUST be best-effort: it must never block or fail the
/// loop — implementations swallow/log their own errors.
///
/// Messages are recorded here **as they happen**, so context compaction (which
/// trims the in-memory working set) never loses the verifiable record.
///
/// Not `Sync` on the sink itself when backed by a mutex-wrapped store; the
/// trait requires `Send + Sync` so the agent loop may run on a worker thread.
pub trait AuditSink: Send + Sync {
    /// A conversation message reached its final form (system/user/assistant/tool).
    fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>);
    /// A tool call settled. `error_kind` distinguishes `denied` from real errors.
    fn tool_call(
        &self,
        id: &str,
        tool: &str,
        args_json: &str,
        ok: bool,
        summary: &str,
        error_kind: Option<&str>,
    );
    /// A file mutation was applied (before/after content, for undo).
    fn file_change(
        &self,
        tool_call_id: &str,
        path: &str,
        kind: &str,
        before: Option<&str>,
        after: Option<&str>,
    );
    /// An out-of-band notice (e.g. an auto-compaction occurred).
    fn note(&self, text: &str);

    /// Tool-result message with name and `tool_call_id` linkage (H3-1).
    fn tool_message(&self, name: &str, tool_call_id: &str, content: &str) {
        let _ = (name, tool_call_id);
        self.message("tool", content, None);
    }

    /// Whether this sink persists to an audit store (vs discarding events).
    fn audit_persists(&self) -> bool {
        true
    }
}
