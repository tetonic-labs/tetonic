//! Owned inputs for the existing application resource operations.
//!
//! These internal Rust inputs are not wire DTOs or authorization proofs. They
//! contain no authenticated principal, credentials, or live authority handles.
//! Each service still authenticates the separate credential and applies its
//! original scope, version, idempotency, and delegation checks before storage.

/// Work identity and intent shared by the work creation entry points.
pub struct CreateTeamWorkItem {
    pub org: String,
    pub team: String,
    pub work_id: String,
    pub title: String,
    pub request_id: String,
    pub goal_id: Option<String>,
}

/// Cursor and event identity for deduplicated work activation.
pub struct ActivateWorkCursor {
    pub org: String,
    pub team: String,
    pub source: String,
    pub cursor_key: String,
    pub event_id: String,
    pub work_title: String,
}

/// Child work, funding and stop scope under an existing parent.
pub struct CreateWorkDelegation {
    pub org: String,
    pub team: String,
    pub delegation_id: String,
    pub parent_work_id: String,
    pub child_work_id: String,
    pub child_title: String,
    pub request_id: String,
    pub parent_budget_tokens: i64,
    pub child_budget_tokens: i64,
    pub stop_scope: String,
    pub peer_org: Option<String>,
    pub peer_team: Option<String>,
}

/// A versioned brief update with its original retry identity.
pub struct SaveWorkBrief {
    pub org: String,
    pub team: String,
    pub work: String,
    pub request: String,
    pub expected: i64,
    pub body: String,
}

/// A hierarchical stop applied by the existing managed runtime.
pub struct ApplyControlStop {
    pub org: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub mode: String,
    pub reason: String,
}

/// Explicit publication of one stored message; replacement text is not accepted.
pub struct PublishContextMessage {
    pub source_context: String,
    pub source_session: String,
    pub source_seq: i64,
    pub destination_context: String,
    pub destination_session: String,
    pub request_id: String,
}

/// A scoped, retry-safe proposal for a consequential effect.
pub struct ProposeEffectApproval {
    pub org: String,
    pub team: String,
    pub approval_id: String,
    pub proposal_digest: String,
    pub request_id: String,
    pub expires_at: i64,
    pub work_id: Option<String>,
}

/// A human decision bound to the exact approval and proposal digest.
pub struct ResolveEffectApproval {
    pub org: String,
    pub team: String,
    pub approval_id: String,
    pub proposal_digest: String,
    pub allow: bool,
    pub now_unix: i64,
}

/// Measured effort attributed to scoped work or a goal.
pub struct RecordTeamEffort {
    pub org: String,
    pub team: String,
    pub entry_id: String,
    pub request_id: String,
    pub measured_tokens: Option<i64>,
    pub goal_id: Option<String>,
    pub work_id: Option<String>,
}

/// Device enrollment attributes; employee and device credentials are separate arguments.
pub struct EnrollWorkstation {
    pub org: String,
    pub workstation_id: String,
    pub label: String,
    pub platform: String,
    pub shared_assignment: bool,
}

/// Assignment identity and generation; the device credential is a separate argument.
pub struct ClaimWorkerAssignment {
    pub org: String,
    pub workstation_id: String,
    pub assignment_id: String,
    pub request_id: String,
    pub work_id: Option<String>,
    pub claimed_generation: i64,
}

/// Stored child grant selectors; live parent authority is a separate argument.
pub struct BindDelegatedExecutionGrant {
    pub org: String,
    pub context: String,
    pub agent_key: String,
    pub definition_digest: String,
    pub grant_id: String,
}
