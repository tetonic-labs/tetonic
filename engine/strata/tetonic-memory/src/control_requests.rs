//! Named inputs for organization-scoped store mutations and admission checks.
//!
//! These are untrusted inputs, not authorization proofs. Store methods retain
//! their existing permission, version, idempotency and transaction checks.
use crate::{HumanQuestionContent, PlanAgentPin, PlanContent};
use tetonic_domain::{AgentJobSpec, ExecutionScope};

/// Explicit disclosure of one stored message to another discussion.
pub struct PublishContextMessage<'a> {
    pub actor: &'a str,
    pub source_context: &'a str,
    pub source_session: &'a str,
    pub source_seq: i64,
    pub destination_context: &'a str,
    pub destination_session: &'a str,
    pub request_id: &'a str,
}

/// Exact parent, child scope, job, and request expected at execution admission.
pub struct DelegatedExecutionBinding<'a> {
    pub id: &'a str,
    pub scope: &'a ExecutionScope,
    pub job: &'a AgentJobSpec,
    pub parent_run: &'a str,
    pub parent_attempt: &'a str,
    pub request: &'a str,
    pub now: i64,
}

/// Scoped, retry-safe proposal for a consequential effect.
pub struct ProposeEffectApproval<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub approval_id: &'a str,
    pub proposal_digest: &'a str,
    pub request_id: &'a str,
    pub expires_at: i64,
    pub work_id: Option<&'a str>,
}

/// Human decision bound to the exact approval and proposal digest.
pub struct ResolveEffectApproval<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub approval_id: &'a str,
    pub proposal_digest: &'a str,
    pub allow: bool,
    pub now_unix: i64,
}

/// Observed effort attributed to a scoped work item or goal.
pub struct RecordTeamEffort<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub entry_id: &'a str,
    pub request_id: &'a str,
    pub measured_tokens: Option<i64>,
    pub goal_id: Option<&'a str>,
    pub work_id: Option<&'a str>,
}

/// Identity and intent shared by all team work creation entry points.
pub struct CreateTeamWorkItem<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work_id: &'a str,
    pub title: &'a str,
    pub request_id: &'a str,
    pub goal_id: Option<&'a str>,
}

/// External event coordinates used to activate work exactly once.
pub struct ActivateWorkCursor<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub source: &'a str,
    pub cursor_key: &'a str,
    pub event_id: &'a str,
    pub work_title: &'a str,
}

/// Child work, funding, and stop scope requested under an existing parent.
pub struct CreateWorkDelegation<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub delegation_id: &'a str,
    pub parent_work_id: &'a str,
    pub child_work_id: &'a str,
    pub child_title: &'a str,
    pub request_id: &'a str,
    pub parent_budget_tokens: i64,
    pub child_budget_tokens: i64,
    pub stop_scope: &'a str,
    pub peer_org: Option<&'a str>,
    pub peer_team: Option<&'a str>,
}

/// Versioned intent update with its original retry identity.
pub struct SaveWorkBrief<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub request: &'a str,
    pub expected: i64,
    pub body: &'a str,
}

/// Inference call charged to an exact work, run, task, and attempt.
pub struct BeginWorkInference<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub run: &'a str,
    pub task: &'a str,
    pub attempt: &'a str,
    pub call: &'a str,
    pub model: &'a str,
    pub now: u64,
}

/// Versioned plan proposal tied to its brief and generation evidence.
pub struct SaveHuddlePlan<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub request: &'a str,
    pub expected: i64,
    pub brief_revision: i64,
    pub generation_id: &'a str,
    pub generation_input: &'a str,
    pub content: Option<&'a PlanContent>,
}

/// Question raised by an executing work attempt.
pub struct AskWorkHuman<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub attempt: &'a str,
    pub id: &'a str,
    pub content: HumanQuestionContent,
    pub now: u64,
}

/// Retry-safe answer to an outstanding work question.
pub struct AnswerWorkHuman<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub id: &'a str,
    pub request: &'a str,
    pub answer: &'a str,
    pub now: u64,
}

/// Versioned change to upcoming assignment instructions.
pub struct AmendPlanAssignment<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub source: &'a str,
    pub expected: i64,
    pub request: &'a str,
    pub key: &'a str,
    pub instructions: &'a str,
    pub now: u64,
}

/// Approved plan revision and pinned agents to admit for execution.
pub struct BeginHuddleExecution<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub source: &'a str,
    pub revision: i64,
    pub request: &'a str,
    pub root: &'a str,
    pub coordinator_digest: &'a str,
    pub pins: &'a [PlanAgentPin],
    pub max_elapsed_seconds: u64,
}

/// Owner enrollment of a device; the secret must not be logged.
pub struct EnrollWorkstation<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub workstation_id: &'a str,
    pub label: &'a str,
    pub platform: &'a str,
    pub device_secret: &'a str,
    pub shared_assignment: bool,
}

/// Device claim for the exact assignment and enrollment generation.
pub struct ClaimWorkerAssignment<'a> {
    pub org: &'a str,
    pub workstation_id: &'a str,
    pub device_secret: &'a str,
    pub assignment_id: &'a str,
    pub request_id: &'a str,
    pub work_id: Option<&'a str>,
    pub claimed_generation: i64,
}
