use serde::{Deserialize, Serialize};
use tetonic_domain::{AgentIdentity, AgentJobSpec};

// ---------------------------------------------------------------------------
// Slice 1 – Initialization
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeCommand {
    pub workspace_root: String,
    pub rpc_token_provided: bool,
    pub rpc_auth_disabled: bool,
}

#[derive(Debug, Clone)]
pub struct InitializeResultPayload {
    pub workspace_root: String,
    pub index_db_path: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct InitializeBootstrapPayload {
    pub workspace_root: String,
    pub policy: std::sync::Arc<tetonic_policy::PolicyEngine>,
    pub runtime: std::sync::Arc<tetonic_runtime::EngineRuntime>,
    pub inference_defaults: tetonic_capacity::InferenceDefaults,
}

// ---------------------------------------------------------------------------
// Slice 2 – Session lifecycle
// ---------------------------------------------------------------------------

/// All parameters the adapter collects before handing off to SessionService.
/// lokai-app owns the live session slot; adapters pass models and transport flags.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartSessionCommand {
    pub workspace_root: String,
    pub resume: Option<bool>,
    pub model_tier: Option<String>,
    pub session_id: Option<String>,
    pub goal: Option<String>,
    pub data_class: Option<String>,
    pub verify_cmd: Option<String>,
    pub briefing: Option<bool>,
    pub orchestration: Option<String>,
    pub critic: Option<bool>,
    pub llm_router: Option<bool>,
    pub model_fast: Option<String>,
    pub model_hard: Option<String>,
    pub session_max_steps: Option<usize>,
    #[serde(default)]
    pub allow_shell: Option<bool>,
    #[serde(default)]
    pub force_explain: Option<bool>,
    #[serde(default)]
    pub auto_grant_approvals: Option<bool>,
}

/// Session start result. Live conversation/cancel/spawn live in SessionService.
pub struct StartSessionResultPayload {
    pub session_id: String,
    pub data_class: String,
    pub resumed: bool,
    pub messages_loaded: u32,
    pub resume_state: String,
    /// The SessionStartPlan produced by SessionHost (briefing, project context, verify_cmd).
    pub plan: tetonic_orchestrator::SessionStartPlan,
    /// Whether the caller explicitly requested the hard model tier.
    pub explicit_hard_tier: bool,
    /// Resolved orchestration mode (Single vs Auto).
    pub orchestration_mode: tetonic_orchestrator::OrchestrationMode,
    pub critic_enabled: bool,
    pub llm_router: bool,
    /// Hydrated conversation for resume; empty for fresh sessions.
    pub messages: Vec<tetonic_inference::Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndSessionCommand {
    pub session_id: String,
    pub workspace_root: String,
    pub status: Option<String>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Slice 3 – Turn execution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTurnCommand {
    pub session_id: String,
    pub user_input: String,
    pub verify_cmd: Option<String>,
    pub llm_router: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentCommand {
    pub session_id: String,
    pub agent_id: String,
    pub parent_agent_id: String,
    pub role: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidateSessionCommand {
    pub session_id: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone)]
pub struct ConsolidateSessionResultPayload {
    pub digest_chars: Option<u32>,
}

pub use tetonic_run::{StartIdentityJobCommand, StartIdentityJobResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteTurnCommand {
    pub session_id: String,
    pub attempt_id: tetonic_domain::AttemptId,
    pub workspace_root: String,
    pub canceled: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Slice 4 – Approvals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRunCommand {
    pub session_id: String,
    pub pooled_cancel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponseCommand {
    pub session_id: String,
    pub approval_id: String,
    pub approved: bool,
    pub remember: bool,
    pub kind: String,
    pub detail: String,
    /// Legacy transport receipt; never authority to approve an unregistered request.
    pub channel_delivered: bool,
    #[serde(default)]
    pub attempt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterApprovalCommand {
    pub session_id: String,
    pub approval_id: String,
    pub call_id: String,
    pub kind: String,
    pub detail: String,
    pub tool: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub missing_controls: Vec<tetonic_core::ConfinementWarning>,
    #[serde(default)]
    pub user_approval_required: bool,
    #[serde(default)]
    pub auto_grant_approvals: bool,
    #[serde(default)]
    pub attempt_id: Option<String>,
}

/// Process-local join for eval / CLI one-shot. Armed before submit. Not durable.
#[derive(Debug, Clone)]
pub struct TurnFinish {
    pub ok: bool,
    pub canceled: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Slice 5 – Policy operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyCommand {
    pub workspace_root: String,
}

#[derive(Debug, Clone)]
pub struct GetPolicyResultPayload {
    pub mode: String,
    pub default_data_class: String,
    pub verify_allowed: bool,
    pub mutations_allowed: bool,
    pub allow_sensitive_to_owner_estate: bool,
    pub allow_repository_to_admin_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPolicyCommand {
    pub mode: Option<String>,
    pub verify_allowed: Option<bool>,
    pub mutations_allowed: Option<bool>,
    pub allow_sensitive_to_owner_estate: Option<bool>,
    pub allow_repository_to_admin_managed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclassifySessionCommand {
    pub session_id: String,
    pub data_class: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ReclassifySessionResultPayload {
    pub data_class: String,
    pub previous_data_class: Option<String>,
}

// ---------------------------------------------------------------------------
// Slice 6 – Estate and capacity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEstateStatusCommand {
    pub fabric_pooled: bool,
}

#[derive(Debug, Clone)]
pub struct EstateStatusResultPayload {
    pub policy_mode: String,
    pub workers_enrolled: u32,
    pub fabric_pooled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeginOptimizeCommand {
    pub sessions_busy: bool,
    pub capacity_busy: bool,
    pub depth: String,
    pub auto_apply: bool,
}

#[derive(Debug, Clone)]
pub struct BeginOptimizeResultPayload {
    pub job_id: String,
    pub depth: tetonic_capacity::OptimizeDepth,
    pub auto_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeCapacityCommand;

#[derive(Clone)]
pub struct CapacityStatusCommand {
    pub client: std::sync::Arc<dyn tetonic_capacity::InferenceClient>,
    pub node_id: String,
    pub ollama_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CapacityStatusResultPayload {
    pub status: tetonic_capacity::CapacityStatus,
}

#[derive(Clone)]
pub struct CapacityDoctorCommand {
    pub client: std::sync::Arc<dyn tetonic_capacity::InferenceClient>,
    pub node_id: String,
    pub ollama_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CapacityDoctorResultPayload {
    pub status: tetonic_capacity::CapacityStatus,
    pub diagnosis: tetonic_capacity::CapacityDiagnosis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOptimizeCommand {
    pub requested_job_id: Option<String>,
    pub active_job_id: Option<String>,
    pub capacity_busy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRunStatusCommand {
    pub session_id: String,
    pub status: String,
    pub agent_id: Option<String>,
    pub error: Option<String>,
}

/// Create a Run without requiring a chat Session (I23).
#[derive(Debug, Clone, Default)]
pub struct CreateRunCommand {
    pub session_id: Option<String>,
    pub root_task_id: Option<String>,
    pub identity: Option<AgentIdentity>,
    pub job_spec: Option<AgentJobSpec>,
}

/// Inspect a Run by id (I10). Does not require a Session.
#[derive(Debug, Clone)]
pub struct InspectRunCommand {
    pub run_id: String,
}

/// Resume public run events after a sequence (I10). Does not require a Session.
#[derive(Debug, Clone)]
pub struct ResumeRunEventsCommand {
    pub run_id: String,
    pub after_sequence: u64,
    pub limit: Option<u32>,
}

/// Cancel a Run by id (I23). Does not require a live Session.
#[derive(Debug, Clone)]
pub struct CancelByRunCommand {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollWorkerCommand {
    pub code: String,
    pub label: Option<String>,
    pub data_dir: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct EnrollWorkerResultPayload {
    pub worker_id: String,
    pub label: String,
    pub host: String,
    pub ip: String,
    pub fabric_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveWorkerCommand {
    pub ref_id: String,
    pub data_dir: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct RemoveWorkerResultPayload {
    pub worker_id: String,
    pub label: String,
    pub revoke_pushed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProfilesCommand {
    pub node_id: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateProfileCommand {
    pub node_id: String,
    pub role: String,
    pub profile_id: String,
}

#[derive(Debug, Clone)]
pub struct ActivateProfileResultPayload {
    pub profile_id: String,
    pub model_fast: String,
    pub num_ctx: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackProfileCommand {
    pub node_id: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct RollbackProfileResultPayload {
    pub profile_id: String,
    pub model_fast: String,
    pub num_ctx: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProfileCommand {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProfileCommand {
    pub json: String,
    pub activate: bool,
    pub refresh_fingerprint: bool,
}

#[derive(Debug, Clone)]
pub struct ImportProfileResultPayload {
    pub profile_id: String,
    pub activated: bool,
    pub model: String,
    pub num_ctx: u32,
}

#[derive(Clone)]
pub struct RunOptimizeCommand {
    pub sessions_busy: bool,
    pub capacity_busy: bool,
    pub depth: String,
    pub auto_apply: bool,
    pub client: std::sync::Arc<dyn tetonic_capacity::InferenceClient>,
    /// When set, skips `begin_optimize` (daemon already queued the job id).
    pub prebegin: Option<BeginOptimizeResultPayload>,
}

#[derive(Debug, Clone)]
pub struct AllowSecretFingerprintResult {
    pub durable: bool,
    pub scope_kind: String,
    pub scope_id: Option<String>,
    pub override_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RevokeSecretFingerprintResult {
    pub revoked_durable: bool,
    pub scope_kind: String,
    pub scope_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCapacityJobCommand {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCapacityJobResultPayload {
    pub job_id: String,
    pub state: String,
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetWorkerTrustResult {
    pub worker_id: String,
    pub trust: String,
    pub policy_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerTrustAuditEntry {
    pub trust: String,
    pub policy_epoch: u64,
    pub recorded_at: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorkerTrustResult {
    pub worker_id: String,
    pub trust: String,
    pub policy_epoch: u64,
    pub audit: Vec<WorkerTrustAuditEntry>,
}
