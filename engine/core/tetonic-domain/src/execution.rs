//! Execution contract: proposed action through capability to sink.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::classify::DataClass;
use crate::ids::{
    ActionId, AgentId, AttemptId, CapabilityId, ExecutionId, RunId, SessionId, TaskId, WorkerId,
    WorkspaceVersion,
};
use crate::policy::PolicyDecision;

/// V3 execution target discriminant — one of the three domain seams (M1).
///
/// `LocalTarget` — execution on the coordinator's local machine (today's only impl for
/// Process, Read, and Mutation authorities).
/// `WorkerTarget` — advertised remote capacity identified by a `WorkerId`.
/// V3 implements WorkerTarget for **Infer only**. (WorkerTarget, Process) must refuse.
///
/// # Serde compatibility
/// Before M1 this was an opaque newtype that serialised as a plain string, and
/// `RunCommand` payloads carrying `holder` are persisted as `payload_json` in the run
/// event store and replayed. `Deserialize` is implemented by hand so a stored
/// `"local"` / `"worker-7"` string still loads: `"local"` becomes `Local`, any other
/// string becomes `Worker`. New writes use the tagged enum form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTargetId {
    /// The coordinator process / local machine — no worker lookup required.
    Local,
    /// A remote or collocated worker identified by `worker_id`.
    Worker { worker_id: WorkerId },
}

/// Label used for `Local` in both the legacy string form and the tagged form.
const LOCAL_TARGET_LABEL: &str = "local";

impl ExecutionTargetId {
    pub fn local() -> Self {
        Self::Local
    }
    pub fn worker(id: impl Into<String>) -> Self {
        Self::Worker {
            worker_id: WorkerId::new(id),
        }
    }
    pub fn as_label(&self) -> String {
        match self {
            Self::Local => LOCAL_TARGET_LABEL.into(),
            Self::Worker { worker_id } => worker_id.0.clone(),
        }
    }
    /// Parse the string form used by persisted run events and scheduler labels.
    pub fn from_label(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        if raw == LOCAL_TARGET_LABEL {
            Self::Local
        } else {
            Self::Worker {
                worker_id: WorkerId::new(raw),
            }
        }
    }
    pub fn worker_id(&self) -> Option<&WorkerId> {
        match self {
            Self::Local => None,
            Self::Worker { worker_id } => Some(worker_id),
        }
    }
}

impl<'de> Deserialize<'de> for ExecutionTargetId {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            /// Legacy opaque string, and the tagged form of the `Local` unit variant.
            Label(String),
            Tagged(TaggedWorker),
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum TaggedWorker {
            Worker { worker_id: WorkerId },
        }
        Ok(match Wire::deserialize(de)? {
            Wire::Label(raw) => Self::from_label(raw),
            Wire::Tagged(TaggedWorker::Worker { worker_id }) => Self::Worker { worker_id },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionKind {
    ReadFile,
    WriteFile,
    DeleteFile,
    ExecuteProcess,
    ExecuteShell,
    ReadEnvironment,
    NetworkRequest,
    DispatchRemote,
    GitOperation,
    StartInternalService,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessClass {
    InternalService,
    RepositoryTool,
    BuildVerification,
    ModelRequestedShell,
    HardwareProbe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalActionParameters {
    pub digest: String,
    pub executable_identity: Option<String>,
    pub resolved_path: Option<String>,
    pub arguments: Vec<String>,
    pub shell_identity: Option<String>,
    pub shell_mode: Option<String>,
    pub script_bytes: Option<Vec<u8>>,
    pub working_directory: Option<String>,
    pub env_vars: Option<std::collections::BTreeMap<String, String>>,
    pub stdin_source_classification: Option<DataClass>,
    pub filesystem_access_scope: Option<String>,
    pub network_policy: Option<String>,
    pub resource_limits: Option<String>,
    pub process_class: Option<ProcessClass>,
    pub sandbox_profile: Option<String>,
    pub expected_output_limits: Option<usize>,
    pub schema_version: u32,
    pub tool_arguments: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CapabilitySet(pub HashSet<String>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TraceContext {
    pub span_id: Option<String>,
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedAction {
    pub action_id: ActionId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<AttemptId>,
    pub agent_id: Option<AgentId>,
    pub workspace_version: Option<WorkspaceVersion>,
    pub data_class: DataClass,
    pub kind: ActionKind,
    pub parameters: CanonicalActionParameters,
    pub requested_capabilities: CapabilitySet,
    pub trace_context: TraceContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequirement {
    None,
    Interactive,
    RememberRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPolicyOutcome {
    pub decision: PolicyDecision,
    pub approval: ApprovalRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "scope")]
pub enum CapabilityScope {
    Bound {
        digest: String,
        /// Host locator for the bound workspace. Not workspace identity (INV-WS-001);
        /// `workspace_version` is the identity field. M3 makes the version required.
        workspace_root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_version: Option<WorkspaceVersion>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedCapability {
    pub capability_id: CapabilityId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<AttemptId>,
    pub agent_id: Option<AgentId>,
    pub action_kind: ActionKind,
    pub canonical_parameter_digest: String,
    pub workspace_version: Option<WorkspaceVersion>,
    pub data_classification: DataClass,
    pub issuance_timestamp: u64,
    pub expiration: u64,
    pub max_use_count: u32,
    pub current_use_count: u32,
    pub issuing_policy_version: String,
    pub approval_record_id: Option<String>,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedAction {
    pub capability: IssuedCapability,
    pub action: ProposedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ExecutionOutcome {
    Started {
        execution_id: ExecutionId,
    },
    Completed {
        execution_id: ExecutionId,
        ok: bool,
        summary: String,
    },
    Failed {
        execution_id: ExecutionId,
        reason: String,
    },
    Cancelled {
        execution_id: ExecutionId,
    },
}

impl ProposedAction {
    pub fn digest(&self) -> String {
        self.parameters.digest.clone()
    }
}

#[cfg(test)]
mod execution_target_tests {
    use super::*;

    #[test]
    fn tagged_form_round_trips() {
        for target in [
            ExecutionTargetId::Local,
            ExecutionTargetId::worker("worker-7"),
        ] {
            let json = serde_json::to_string(&target).unwrap();
            let back: ExecutionTargetId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, target);
        }
    }

    #[test]
    fn legacy_string_payloads_still_deserialize() {
        let local: ExecutionTargetId = serde_json::from_str("\"local\"").unwrap();
        assert_eq!(local, ExecutionTargetId::Local);

        let worker: ExecutionTargetId = serde_json::from_str("\"worker-7\"").unwrap();
        assert_eq!(worker, ExecutionTargetId::worker("worker-7"));
    }

    #[test]
    fn capability_scope_accepts_documents_without_workspace_version() {
        let legacy = r#"{"scope":"bound","digest":"d1","workspace_root":"/repo"}"#;
        let scope: CapabilityScope = serde_json::from_str(legacy).unwrap();
        let CapabilityScope::Bound {
            workspace_version, ..
        } = &scope;
        assert!(workspace_version.is_none());
        assert_eq!(serde_json::to_string(&scope).unwrap(), legacy);
    }
}

// Ensure old AuthorizationContext isn't fully removed if it's used directly in places we aren't refactoring, but it seems we should refactor them.
