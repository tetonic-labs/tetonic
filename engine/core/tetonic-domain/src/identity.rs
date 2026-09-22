//! Durable identity and job-spec contracts (WORK-01).
//!
//! Neutral manager types. No coding, Session, RoleId, or repository fields.
//! `AgentJobSpec` is durable. `AgentInvocation` is runtime-only and is not
//! stored as job truth.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ids::{AttemptId, IdentityId};
use crate::invocation::{AgentInvocation, CandidateOutcome};

/// Durable standing actor. Not Session. Not `struct Agent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: IdentityId,
    pub owning_application: String,
    pub bound_definition_digest: String,
    pub privilege_class: String,
    pub toolset_subscriptions: Vec<String>,
    pub context_bindings: Vec<String>,
    pub recovery_id: String,
}

/// Immutable durable job description. Names an identity. Not `AgentInvocation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentJobSpec {
    pub identity_id: IdentityId,
    pub definition_digest: String,
    pub input_digest: String,
    pub capability_bindings: Vec<String>,
    pub artifact_bindings: Vec<String>,
    pub recovery_id: String,
}

/// Runtime leftover context for one local executor call. Conversation stays
/// on the implementation (CODE-02 leftover), not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptExecutionContext {
    pub attempt_id: AttemptId,
}

/// Manager executor boundary. One implementation; first placement local.
#[async_trait]
pub trait AgentAttemptExecutor: Send {
    async fn execute(
        &mut self,
        invocation: AgentInvocation,
        ctx: AttemptExecutionContext,
    ) -> CandidateOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_job_spec_round_trip_json() {
        let id = AgentIdentity {
            id: IdentityId::new("id_coding_production"),
            owning_application: "coding".into(),
            bound_definition_digest: "digest".into(),
            privilege_class: "default".into(),
            toolset_subscriptions: vec!["planner".into()],
            context_bindings: vec!["memory".into()],
            recovery_id: "id_coding_production".into(),
        };
        let spec = AgentJobSpec {
            identity_id: id.id.clone(),
            definition_digest: "digest".into(),
            input_digest: "input:1".into(),
            capability_bindings: Vec::new(),
            artifact_bindings: Vec::new(),
            recovery_id: id.recovery_id.clone(),
        };
        let id_json = serde_json::to_string(&id).unwrap();
        let spec_json = serde_json::to_string(&spec).unwrap();
        assert!(!id_json.contains("session"));
        assert!(!spec_json.contains("instructions"));
        assert!(!spec_json.contains("empty_tool_nudge"));
        assert!(!spec_json.contains("completion_tool"));
        let id_back: AgentIdentity = serde_json::from_str(&id_json).unwrap();
        let spec_back: AgentJobSpec = serde_json::from_str(&spec_json).unwrap();
        assert_eq!(id_back, id);
        assert_eq!(spec_back, spec);
    }
}
