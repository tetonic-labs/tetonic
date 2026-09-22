//! JSON-RPC 2.0 envelope + the v1 method params/results and notification
//! payloads. Types derive `schemars::JsonSchema` so the editor's TypeScript
//! client can be code-generated from a single source of truth.
//!
//! Notification envelope convention: every notification's `params` object also
//! carries `session_id`, `agent_id`, and a per-session monotonic `seq`, injected
//! by [`crate::server::Notifier`]. The payload structs below describe only the
//! event-specific fields; `agent_id` exists from v1 (always `"a0"` today) so a
//! future editor can render a tree of agents without a protocol bump.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---- JSON-RPC 2.0 envelope -------------------------------------------------

/// An inbound message (editor -> daemon). Requests carry an `id`; notifications
/// (none in v1 editor->daemon, but tolerated) omit it.
#[derive(Debug, Clone, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A response to a request. Exactly one of `result`/`error` is set.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.code(),
            message: message.into(),
            data: None,
        }
    }
}

/// Stable error codes. Standard JSON-RPC reserved codes plus a small
/// application range. Codes are part of the contract — do not renumber.
#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    InternalError,
    /// Method is recognized but not implemented in this build (e.g. fabric).
    NotImplemented,
    /// `session_id` does not refer to a live session.
    UnknownSession,
    /// Daemon is not yet initialized (no `initialize` call).
    NotReady,
}

impl ErrorCode {
    pub fn code(self) -> i64 {
        match self {
            ErrorCode::ParseError => -32700,
            ErrorCode::InvalidRequest => -32600,
            ErrorCode::MethodNotFound => -32601,
            ErrorCode::InvalidParams => -32602,
            ErrorCode::InternalError => -32603,
            ErrorCode::NotImplemented => -32000,
            ErrorCode::UnknownSession => -32001,
            ErrorCode::NotReady => -32002,
        }
    }
}

// ---- Method names (editor -> daemon) ---------------------------------------

pub mod methods {
    pub const INITIALIZE: &str = "initialize";
    pub const SESSION_START: &str = "session/start";
    pub const SESSION_END: &str = "session/end";
    pub const CHAT_SEND: &str = "chat/send";
    pub const SESSION_CANCEL: &str = "session/cancel";
    pub const APPROVAL_RESPOND: &str = "approval/respond";
    pub const MODEL_LIST: &str = "model/list";
    pub const EGRESS_POLICY_GET: &str = "egress/policy.get";
    pub const EGRESS_POLICY_SET: &str = "egress/policy.set";
    pub const POLICY_GET: &str = "policy/get";
    pub const POLICY_SET: &str = "policy/set";
    pub const ESTATE_STATUS: &str = "estate/status";
    pub const ESTATE_CAPACITY_STATUS: &str = "estate/capacity/status";
    pub const ESTATE_CAPACITY_DOCTOR: &str = "estate/capacity/doctor";
    pub const ESTATE_CAPACITY_OPTIMIZE: &str = "estate/capacity/optimize";
    pub const ESTATE_CAPACITY_CANCEL: &str = "estate/capacity/cancel";
    pub const ESTATE_CAPACITY_PROFILES_LIST: &str = "estate/capacity/profiles/list";
    pub const ESTATE_CAPACITY_PROFILES_ACTIVATE: &str = "estate/capacity/profiles/activate";
    pub const ESTATE_CAPACITY_PROFILES_ROLLBACK: &str = "estate/capacity/profiles/rollback";
    pub const ESTATE_CAPACITY_PROFILES_EXPORT: &str = "estate/capacity/profiles/export";
    pub const ESTATE_CAPACITY_JOBS_GET: &str = "estate/capacity/jobs/get";
    pub const ESTATE_CAPACITY_JOBS_CANCEL: &str = "estate/capacity/jobs/cancel";
    pub const FABRIC_STATUS: &str = "fabric/status";
    pub const FABRIC_WORKER_TRUST_SET: &str = "fabric/worker.trust.set";
    pub const FABRIC_WORKER_TRUST_GET: &str = "fabric/worker.trust.get";
    pub const AGENT_SPAWN: &str = "agent/spawn";
    pub const PROJECT_CONSOLIDATE: &str = "project/consolidate";
    pub const SESSION_RECLASSIFY: &str = "session/reclassify";
    pub const SESSION_INFERENCE: &str = "session/inference";
    pub const SESSION_MODELS: &str = "session/models";
    pub const SESSION_SELECT_MODEL: &str = "session/selectModel";
    pub const SESSION_SET_INFERENCE: &str = "session/setInference";
    pub const RUN_SNAPSHOT: &str = "run/snapshot";
    pub const RUN_RESUME: &str = "run/resume";
    pub const RUN_CANCEL: &str = "run/cancel";
    pub const SECRET_RULE_ADD: &str = "secret/rule/add";
    pub const SECRET_FINGERPRINT_ALLOW: &str = "secret/fingerprint/allow";
    pub const SECRET_FINGERPRINT_REVOKE: &str = "secret/fingerprint/revoke";
    pub const SHUTDOWN: &str = "shutdown";
}

// ---- Notification names (daemon -> editor) ---------------------------------

pub mod events {
    pub const RUN_STATUS: &str = "event/run_status";
    pub const TOKEN: &str = "event/token";
    pub const TOOL_CALL: &str = "event/tool_call";
    pub const TOOL_RESULT: &str = "event/tool_result";
    pub const DIFF: &str = "event/diff";
    pub const APPROVAL_REQUEST: &str = "event/approval_request";
    pub const EGRESS: &str = "event/egress";
    /// Additive (Phase D Context Inspector). Editors that don't know it ignore it.
    pub const CONTEXT: &str = "event/context";
    pub const LOG: &str = "event/log";
    /// Capacity optimize job progress (`session_id` is synthetic, e.g. `__capacity__`).
    pub const CAPACITY_PROGRESS: &str = "event/capacity/progress";
    /// M2-2: explains why inference stayed local or went remote.
    pub const DISPATCH_PLACEMENT: &str = "event/dispatch_placement";
}

// ---- initialize ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub workspace_root: String,
    /// Session token printed by `lokaid` on stderr at startup (SEC-003).
    pub rpc_token: String,
    #[serde(default)]
    pub client_info: Option<ClientInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InitializeResult {
    pub protocol_version: u32,
    pub daemon_info: DaemonInfo,
    pub capabilities: Capabilities,
    /// Active capacity profile summary (ES5). Omitted when store unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<CapacitySummary>,
    /// Echo of the accepted RPC session token (editor may reuse on reconnect).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DaemonInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Capabilities {
    pub streaming: bool,
    pub approvals: bool,
    /// Workspace tools the agent can call this session.
    pub tools: Vec<String>,
    /// Reserved orchestration tool names handled at the orchestrator level
    /// (Phase C harness/sub-agent model). Advertised from v1 so a client can
    /// route/render them later; NOT executed yet.
    pub orchestration_tools: Vec<String>,
}

// ---- session/start ---------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionStartParams {
    #[serde(default)]
    pub goal: Option<String>,
    /// Reserved for the router (Phase C): which capability tier to target.
    #[serde(default)]
    pub model_tier: Option<String>,
    /// Optional verify-before-finish command run in the workspace when the agent
    /// finishes (e.g. "pytest -q"). On failure the agent must fix and finish
    /// again. A trusted, host-supplied allowlist entry — not model-driven shell.
    #[serde(default)]
    pub verify_cmd: Option<String>,
    /// Override session data class (M2-2: `public` | `repository_source` | `sensitive_source` | `secret`;
    /// legacy aliases `private` | `personal` | `circle_ok` accepted).
    #[serde(default)]
    pub data_class: Option<String>,
    /// Inject repo briefing on first turn (default true when omitted).
    #[serde(default)]
    pub briefing: Option<bool>,
    /// Orchestration mode: `single` (default) or `auto` (router + specialists + critic).
    #[serde(default)]
    pub orchestration: Option<String>,
    /// Post-edit critic pass after mutating specialist work (default true when orchestration is auto).
    #[serde(default)]
    pub critic: Option<bool>,
    /// Use one-line LLM router before keyword fallback (default false).
    #[serde(default)]
    pub llm_router: Option<bool>,
    /// When true, rebuild `Conversation` from audit `messages` instead of starting fresh.
    #[serde(default)]
    pub resume: Option<bool>,
    /// Session to resume (required when multiple sessions exist; defaults to latest for workspace).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Allow shell execution in the session (defaults to false when omitted).
    #[serde(default)]
    pub allow_shell: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentSpawnParams {
    pub session_id: String,
    /// Specialist role: planner | coder | debugger | reviewer | critic
    pub role: String,
    pub task: String,
    #[serde(default)]
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentSpawnResult {
    pub agent_id: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionStartResult {
    pub session_id: String,
    /// Effective session data class after floor coercion (SEC2-E2-026).
    #[serde(default = "default_personal_data_class")]
    pub data_class: String,
    /// True when an existing audit session was reopened (AR1-1).
    #[serde(default)]
    pub resumed: bool,
    /// Messages loaded into the working conversation (0 when not resuming).
    #[serde(default)]
    pub messages_loaded: u32,
    /// `fresh` | `continued` | `incomplete` — session continuity hint (AC2-5).
    #[serde(default = "default_resume_state_fresh")]
    pub resume_state: String,
}

fn default_resume_state_fresh() -> String {
    "fresh".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionEndParams {
    pub session_id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionEndResult {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReclassifyParams {
    pub session_id: String,
    pub data_class: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReclassifyResult {
    pub ok: bool,
    pub data_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_data_class: Option<String>,
}

fn default_personal_data_class() -> String {
    "repository_source".into()
}

// ---- Secret Overrides ----------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddSecretRuleRequest {
    pub pattern: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AllowSecretFingerprintRequest {
    pub fingerprint: String,
    /// `global` (default) | `session` | `project`
    #[serde(default = "default_override_scope_kind")]
    pub scope_kind: String,
    #[serde(default)]
    pub scope_id: Option<String>,
    /// Persist in lokai.db (default true).
    #[serde(default = "default_true")]
    pub durable: bool,
}

fn default_override_scope_kind() -> String {
    "global".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RevokeSecretFingerprintRequest {
    pub fingerprint: String,
    #[serde(default = "default_override_scope_kind")]
    pub scope_kind: String,
    #[serde(default)]
    pub scope_id: Option<String>,
}

// ---- Methods (v1) -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatSendParams {
    pub session_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Attachment {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Accepted {
    pub accepted: bool,
}

// ---- session/cancel --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCancelParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Canceled {
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunSnapshotParams {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunSnapshotResult {
    pub run_id: String,
    pub sequence: u64,
    pub state: String,
    pub snapshot: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunResumeParams {
    pub run_id: String,
    pub after_sequence: u64,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunResumeGap {
    pub requested_after: u64,
    pub earliest_available: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_sequence: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunResumeResult {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<RunResumeGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunCancelParams {
    pub run_id: String,
}

// ---- project/consolidate ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectConsolidateParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectConsolidateResult {
    pub ok: bool,
    #[serde(default)]
    pub digest_chars: Option<usize>,
}

// ---- approval/respond ------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApprovalRespondParams {
    pub session_id: String,
    pub approval_id: String,
    pub decision: ApprovalDecision,
    /// Reserved: persist/scope an allow rule (e.g. "always allow cargo test").
    #[serde(default)]
    pub remember: bool,
}

/// Generic `{ ok: true }` acknowledgement.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ack {
    pub ok: bool,
}

// ---- model/list ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelListResult {
    pub models: Vec<String>,
    /// Whether the currently selected model advertises tool-calling.
    pub tool_capable: bool,
    pub default_model: String,
    /// The model used for `model_tier: "hard"` sessions (largest tool-capable
    /// installed model, or the `LOKAI_MODEL_HARD` override).
    pub hard_model: String,
}

// ---- policy/get | policy/set (D1/D3) ---------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyGetResult {
    pub mode: String,
    pub default_data_class: String,
    pub verify_allowed: bool,
    pub mutations_allowed: bool,
    pub policy_epoch: u64,
    pub allow_sensitive_to_owner_estate: bool,
    pub allow_repository_to_admin_managed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicySetParams {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub verify_allowed: Option<bool>,
    #[serde(default)]
    pub mutations_allowed: Option<bool>,
    #[serde(default)]
    pub allow_sensitive_to_owner_estate: Option<bool>,
    #[serde(default)]
    pub allow_repository_to_admin_managed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicySetResult {
    pub ok: bool,
    pub mode: String,
    pub verify_allowed: bool,
    pub mutations_allowed: bool,
    pub policy_epoch: u64,
    pub allow_sensitive_to_owner_estate: bool,
    pub allow_repository_to_admin_managed: bool,
}

// ---- estate/status (D3) ----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EstateStatusResult {
    pub policy_mode: String,
    pub workers_enrolled: u32,
    pub fabric_pooled: bool,
}

// ---- fabric/status (N0) ----------------------------------------------------

/// Capacity health summary for a fabric node (mirrors inference-fabric-v1).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricNodeCapacityHealth {
    /// `healthy` | `degraded` | `unknown` | `no_profile`
    pub doctor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    pub gates_ok: bool,
    pub stale: bool,
}

/// One node in the fabric topology snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricNodeInfo {
    pub id: String,
    pub label: String,
    pub vram_total_mb: u32,
    pub vram_free_mb: u32,
    pub resident_models: Vec<String>,
    pub queue_depth: u32,
    pub healthy: bool,
    #[serde(default)]
    pub models_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<FabricNodeCapacityHealth>,
}

/// Result of `fabric/status` (inference-fabric-v1 topology snapshot).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricStatusResult {
    pub nodes: Vec<FabricNodeInfo>,
    pub effective_concurrency: u32,
    /// RFC 3339 timestamp.
    pub generated_at: String,
    /// Scheduler mean |actual − predicted| finish error (ms), when samples > 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_prediction_mae_ms: Option<u64>,
    /// Samples contributing to `scheduler_prediction_mae_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_prediction_samples: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricWorkerTrustSetParams {
    pub worker_id: String,
    /// `local_machine` | `owner_controlled_estate` | `administratively_managed` | `external_untrusted`
    pub trust: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricWorkerTrustSetResult {
    pub ok: bool,
    pub worker_id: String,
    pub trust: String,
    pub policy_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricWorkerTrustGetParams {
    pub worker_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricWorkerTrustAuditEntry {
    pub trust: String,
    pub policy_epoch: u64,
    pub recorded_at: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FabricWorkerTrustGetResult {
    pub worker_id: String,
    pub trust: String,
    pub policy_epoch: u64,
    pub audit: Vec<FabricWorkerTrustAuditEntry>,
}

/// Capacity profile summary (`estate/capacity/status`, `initialize.capacity`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacitySummary {
    pub completed: bool,
    pub stale: bool,
    /// `healthy` | `degraded` | `unknown` | `no_profile`
    pub doctor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_summary: Option<String>,
    pub gates_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_setup_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityDoctorResult {
    pub status: CapacitySummary,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityOptimizeParams {
    /// `quick` (default) or `full`.
    #[serde(default)]
    pub depth: Option<String>,
    /// Activate the best profile when gates pass (default false).
    #[serde(default)]
    pub auto_apply: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityOptimizeResult {
    pub job_id: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityCancelParams {
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityCancelResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProgressParams {
    pub job_id: String,
    pub phase: String,
    pub percent: u32,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesListParams {
    #[serde(default)]
    pub node_id: Option<String>,
    /// `coder` (default) | `fast` | `hard` | `embed`
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfileListItem {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub gates_passed: bool,
    pub estate_model: String,
    pub num_ctx: u32,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesListResult {
    pub profiles: Vec<CapacityProfileListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesActivateParams {
    pub profile_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesActivateResult {
    pub ok: bool,
    pub profile_id: String,
    pub status: CapacitySummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesRollbackParams {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesRollbackResult {
    pub ok: bool,
    pub profile_id: String,
    pub status: CapacitySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesExportParams {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityProfilesExportResult {
    pub profile: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityJobsGetParams {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityJobsGetResult {
    pub job_id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
}

// ---- egress/policy.get -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EgressPolicy {
    /// Always `"deny"` in v1 (default-deny is an invariant, not a setting).
    pub default: String,
    pub allow: Vec<EgressAllowRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EgressAllowRule {
    pub label: String,
    pub ip: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EgressPolicySetParams {
    #[serde(default)]
    pub add: Vec<EgressAllowRule>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EgressPolicySetResult {
    pub ok: bool,
    pub allow: Vec<EgressAllowRule>,
}

// ---- notification payloads (event-specific fields only) --------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunStatusParams {
    /// `started` | `ok` | `error` | `canceled`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenParams {
    pub delta: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolCallParams {
    pub tool_call_id: String,
    pub tool: String,
    pub args: Value,
    /// `proposed` | `started`.
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolResultParams {
    pub tool_call_id: String,
    pub ok: bool,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    pub tool_call_id: String,
    pub path: String,
    /// `edit` | `create` | `delete`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MissingControlParams {
    /// Wire name of the sandbox control, e.g. `network_denial`.
    pub control: String,
    /// `low` | `medium` | `high` | `critical`.
    pub risk_level: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApprovalRequestParams {
    pub approval_id: String,
    /// e.g. `run_shell`.
    pub kind: String,
    /// Human-readable detail (e.g. the exact shell command).
    pub detail: String,
    pub tool_call_id: String,
    /// Typed OS-confinement gaps. Empty when the action is fully sandboxed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_controls: Vec<MissingControlParams>,
    /// When true, the host must prompt even if a remembered rule would allow.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub user_approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EgressEventParams {
    pub initiator: String,
    pub host: String,
    pub resolved_ip: Option<String>,
    pub port: u16,
    /// `allow` | `deny`.
    pub decision: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContextParams {
    pub system_tokens: usize,
    pub tools_tokens: usize,
    pub conversation_tokens: usize,
    pub total_tokens: usize,
    pub budget: usize,
    pub dropped_messages: usize,
    pub estimated: bool,
    /// M2-2: effective data class for this context snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DispatchPlacementParams {
    pub target: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<String>,
    #[serde(default)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogParams {
    pub level: String,
    pub message: String,
}

/// Every request/result/notification payload type registered for codegen.
/// Keep in sync with `schema_bundle()` — the schema test fails if this drifts.
pub const SCHEMA_DEFINITIONS: &[&str] = &[
    "SessionInferenceParams",
    "SessionSelectModelParams",
    "SessionModelsResult",
    "ModelChoice",
    "SessionSetInferenceParams",
    "SessionInferenceResult",
    "Accepted",
    "Ack",
    "AgentSpawnParams",
    "AgentSpawnResult",
    "ApprovalDecision",
    "ApprovalRequestParams",
    "ApprovalRespondParams",
    "Attachment",
    "Canceled",
    "CapacityCancelParams",
    "CapacityCancelResult",
    "CapacityDoctorResult",
    "CapacityJobsGetParams",
    "CapacityJobsGetResult",
    "CapacityOptimizeParams",
    "CapacityOptimizeResult",
    "CapacityProfileListItem",
    "CapacityProfilesActivateParams",
    "CapacityProfilesActivateResult",
    "CapacityProfilesExportParams",
    "CapacityProfilesExportResult",
    "CapacityProfilesListParams",
    "CapacityProfilesListResult",
    "CapacityProfilesRollbackParams",
    "CapacityProfilesRollbackResult",
    "CapacityProgressParams",
    "CapacitySummary",
    "Capabilities",
    "ChatSendParams",
    "ClientInfo",
    "ContextParams",
    "DaemonInfo",
    "DiffParams",
    "DispatchPlacementParams",
    "EgressAllowRule",
    "EgressEventParams",
    "EgressPolicy",
    "EgressPolicySetParams",
    "EgressPolicySetResult",
    "EstateStatusResult",
    "FabricNodeCapacityHealth",
    "FabricNodeInfo",
    "FabricStatusResult",
    "InitializeParams",
    "InitializeResult",
    "LogParams",
    "MissingControlParams",
    "ModelListResult",
    "PolicyGetResult",
    "PolicySetParams",
    "PolicySetResult",
    "ProjectConsolidateParams",
    "ProjectConsolidateResult",
    "RunCancelParams",
    "RunResumeGap",
    "RunResumeParams",
    "RunResumeResult",
    "RunSnapshotParams",
    "RunSnapshotResult",
    "RunStatusParams",
    "SessionCancelParams",
    "SessionEndParams",
    "SessionEndResult",
    "SessionReclassifyParams",
    "SessionReclassifyResult",
    "SessionStartParams",
    "SessionStartResult",
    "TokenParams",
    "ToolCallParams",
    "ToolResultParams",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_params_round_trip() {
        let raw = r#"{"protocol_version":1,"workspace_root":"/tmp/ws","rpc_token":"abc","client_info":{"name":"test"}}"#;
        let p: InitializeParams = serde_json::from_str(raw).unwrap();
        assert_eq!(p.protocol_version, 1);
        assert_eq!(p.workspace_root, "/tmp/ws");
        assert_eq!(p.rpc_token, "abc");
        assert_eq!(p.client_info.unwrap().name, "test");
    }

    #[test]
    fn approval_decision_is_snake_case() {
        let d: ApprovalDecision = serde_json::from_str("\"allow\"").unwrap();
        assert_eq!(d, ApprovalDecision::Allow);
        assert_eq!(
            serde_json::to_string(&ApprovalDecision::Deny).unwrap(),
            "\"deny\""
        );
    }

    #[test]
    fn approval_request_params_typed_missing_controls() {
        let p = ApprovalRequestParams {
            approval_id: "ap_1".into(),
            kind: "run_shell".into(),
            detail: "npm test".into(),
            tool_call_id: "tc_1".into(),
            missing_controls: vec![MissingControlParams {
                control: "network_denial".into(),
                risk_level: "high".into(),
                reason: "OS-level network filtering not available; broker-only policy".into(),
            }],
            user_approval_required: true,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["missing_controls"][0]["control"], "network_denial");
        assert_eq!(v["missing_controls"][0]["risk_level"], "high");
        assert_eq!(v["user_approval_required"], true);
        let back: ApprovalRequestParams = serde_json::from_value(v).unwrap();
        assert_eq!(back.missing_controls[0].control, "network_denial");
        assert!(back.user_approval_required);

        let legacy = serde_json::json!({
            "approval_id": "ap_1",
            "kind": "run_shell",
            "detail": "echo hi",
            "tool_call_id": "tc_1"
        });
        let old: ApprovalRequestParams = serde_json::from_value(legacy).unwrap();
        assert!(old.missing_controls.is_empty());
        assert!(!old.user_approval_required);
    }

    #[test]
    fn response_serializes_without_null_error() {
        let r = Response::ok(serde_json::json!(1), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"result\""));
        assert!(!s.contains("\"error\""));
    }

    #[test]
    fn incoming_tolerates_missing_id_and_params() {
        let raw = r#"{"jsonrpc":"2.0","method":"shutdown"}"#;
        let m: Incoming = serde_json::from_str(raw).unwrap();
        assert_eq!(m.method, "shutdown");
        assert!(m.id.is_none());
        assert!(m.params.is_null());
    }

    #[test]
    fn schema_generation_works_for_codegen() {
        // The TS client is generated from these schemas; make sure they build.
        let _ = schemars::schema_for!(InitializeResult);
        let _ = schemars::schema_for!(ToolCallParams);
        let _ = schemars::schema_for!(DiffParams);
    }
}
