// Code generated from `tetonicd --print-schema`. DO NOT EDIT.
// Source of truth: engine/crates/lokai-rpc (Rust). Regenerate with
// `cargo run -q -p tetonicd -- --print-schema | python scripts/gen_ts_protocol.py`.

export const PROTOCOL_VERSION = 1 as const;

export interface Accepted {
  accepted: boolean;
}

/** Generic `{ ok: true }` acknowledgement. */
export interface Ack {
  ok: boolean;
}

export interface AgentSpawnParams {
  parent_agent_id?: string | null;
  /** Specialist role: planner | coder | debugger | reviewer | critic */
  role: string;
  session_id: string;
  task: string;
}

export interface AgentSpawnResult {
  accepted: boolean;
  agent_id: string;
}

export type ApprovalDecision = "allow" | "deny";

export interface ApprovalRequestParams {
  approval_id: string;
  /** Human-readable detail (e.g. the exact shell command). */
  detail: string;
  /** e.g. `run_shell`. */
  kind: string;
  /** Typed OS-confinement gaps. Empty when the action is fully sandboxed. */
  missing_controls?: MissingControlParams[];
  tool_call_id: string;
  /** When true, the host must prompt even if a remembered rule would allow. */
  user_approval_required?: boolean;
}

export interface ApprovalRespondParams {
  approval_id: string;
  decision: ApprovalDecision;
  /** Reserved: persist/scope an allow rule (e.g. "always allow cargo test"). */
  remember?: boolean;
  session_id: string;
}

export interface Attachment {
  path: string;
}

export interface Canceled {
  canceled: boolean;
}

export interface Capabilities {
  approvals: boolean;
  /** Reserved orchestration tool names handled at the orchestrator level (Phase C harness/sub-agent model). Advertised from v1 so a client can route/render them later; NOT executed yet. */
  orchestration_tools: string[];
  streaming: boolean;
  /** Workspace tools the agent can call this session. */
  tools: string[];
}

export interface CapacityCancelParams {
  job_id?: string | null;
}

export interface CapacityCancelResult {
  cancelled: boolean;
}

export interface CapacityDoctorResult {
  codes?: string[];
  recommendations?: string[];
  status: CapacitySummary;
  summary: string;
}

export interface CapacityJobsGetParams {
  job_id: string;
}

export interface CapacityJobsGetResult {
  job_id: string;
  json?: unknown;
  state: string;
}

export interface CapacityOptimizeParams {
  /** Activate the best profile when gates pass (default false). */
  auto_apply?: boolean | null;
  /** `quick` (default) or `full`. */
  depth?: string | null;
}

export interface CapacityOptimizeResult {
  accepted: boolean;
  job_id: string;
}

export interface CapacityProfileListItem {
  active: boolean;
  created_at: string;
  estate_model: string;
  gates_passed: boolean;
  id: string;
  label: string;
  num_ctx: number;
}

export interface CapacityProfilesActivateParams {
  node_id?: string | null;
  profile_id: string;
  role?: string | null;
}

export interface CapacityProfilesActivateResult {
  ok: boolean;
  profile_id: string;
  status: CapacitySummary;
}

export interface CapacityProfilesExportParams {
  profile_id: string;
}

export interface CapacityProfilesExportResult {
  profile: unknown;
}

export interface CapacityProfilesListParams {
  node_id?: string | null;
  /** `coder` (default) | `fast` | `hard` | `embed` */
  role?: string | null;
}

export interface CapacityProfilesListResult {
  profiles: CapacityProfileListItem[];
}

export interface CapacityProfilesRollbackParams {
  node_id?: string | null;
  role?: string | null;
}

export interface CapacityProfilesRollbackResult {
  ok: boolean;
  profile_id: string;
  status: CapacitySummary;
}

export interface CapacityProgressParams {
  job_id: string;
  message: string;
  percent: number;
  phase: string;
}

/** Capacity profile summary (`estate/capacity/status`, `initialize.capacity`). */
export interface CapacitySummary {
  active_profile_id?: string | null;
  active_profile_label?: string | null;
  completed: boolean;
  /** `healthy` | `degraded` | `unknown` | `no_profile` */
  doctor: string;
  gates_ok: boolean;
  hardware_summary?: string | null;
  last_setup_at?: string | null;
  stale: boolean;
}

export interface ChatSendParams {
  attachments?: Attachment[];
  session_id: string;
  text: string;
}

export interface ClientInfo {
  name: string;
  version?: string;
}

export interface ContextParams {
  budget: number;
  conversation_tokens: number;
  /** M2-2: effective data class for this context snapshot. */
  data_class?: string | null;
  dropped_messages: number;
  estimated: boolean;
  system_tokens: number;
  tools_tokens: number;
  total_tokens: number;
}

export interface DaemonInfo {
  name: string;
  version: string;
}

export interface DiffParams {
  after?: string | null;
  before?: string | null;
  /** `edit` | `create` | `delete`. */
  kind: string;
  path: string;
  tool_call_id: string;
}

export interface DispatchPlacementParams {
  data_class?: string | null;
  decision: string;
  reason?: string | null;
  reason_code?: string | null;
  redacted?: boolean;
  target: string;
}

export interface EgressAllowRule {
  ip: string;
  label: string;
  port?: number | null;
}

export interface EgressEventParams {
  /** `allow` | `deny`. */
  decision: string;
  host: string;
  initiator: string;
  port: number;
  reason: string;
  resolved_ip?: string | null;
}

export interface EgressPolicy {
  allow: EgressAllowRule[];
  /** Always `"deny"` in v1 (default-deny is an invariant, not a setting). */
  default: string;
}

export interface EgressPolicySetParams {
  add?: EgressAllowRule[];
  remove?: string[];
}

export interface EgressPolicySetResult {
  allow: EgressAllowRule[];
  ok: boolean;
}

export interface EstateStatusResult {
  fabric_pooled: boolean;
  policy_mode: string;
  workers_enrolled: number;
}

/** Capacity health summary for a fabric node (mirrors inference-fabric-v1). */
export interface FabricNodeCapacityHealth {
  active_profile_id?: string | null;
  /** `healthy` | `degraded` | `unknown` | `no_profile` */
  doctor: string;
  gates_ok: boolean;
  stale: boolean;
}

/** One node in the fabric topology snapshot. */
export interface FabricNodeInfo {
  capacity?: FabricNodeCapacityHealth | null;
  healthy: boolean;
  id: string;
  label: string;
  models_verified?: boolean;
  queue_depth: number;
  resident_models: string[];
  vram_free_mb: number;
  vram_total_mb: number;
}

/** Result of `fabric/status` (inference-fabric-v1 topology snapshot). */
export interface FabricStatusResult {
  effective_concurrency: number;
  /** RFC 3339 timestamp. */
  generated_at: string;
  nodes: FabricNodeInfo[];
  /** Scheduler mean |actual − predicted| finish error (ms), when samples > 0. */
  scheduler_prediction_mae_ms?: number | null;
  /** Samples contributing to `scheduler_prediction_mae_ms`. */
  scheduler_prediction_samples?: number | null;
}

export interface InitializeParams {
  client_info?: ClientInfo | null;
  protocol_version: number;
  /** Session token printed by `tetonicd` on stderr at startup (SEC-003). */
  rpc_token: string;
  workspace_root: string;
}

export interface InitializeResult {
  capabilities: Capabilities;
  /** Active capacity profile summary (ES5). Omitted when store unavailable. */
  capacity?: CapacitySummary | null;
  daemon_info: DaemonInfo;
  protocol_version: number;
  /** Echo of the accepted RPC session token (editor may reuse on reconnect). */
  rpc_token?: string | null;
}

export interface LogParams {
  level: string;
  message: string;
}

export interface MissingControlParams {
  /** Wire name of the sandbox control, e.g. `network_denial`. */
  control: string;
  reason: string;
  /** `low` | `medium` | `high` | `critical`. */
  risk_level: string;
}

export interface ModelChoice {
  availability: string;
  current: boolean;
  id: string;
  name: string;
  provider_label: string;
}

export interface ModelListResult {
  default_model: string;
  /** The model used for `model_tier: "hard"` sessions (largest tool-capable installed model, or the `LOKAI_MODEL_HARD` override). */
  hard_model: string;
  models: string[];
  /** Whether the currently selected model advertises tool-calling. */
  tool_capable: boolean;
}

export interface PolicyGetResult {
  allow_repository_to_admin_managed: boolean;
  allow_sensitive_to_owner_estate: boolean;
  default_data_class: string;
  mode: string;
  mutations_allowed: boolean;
  policy_epoch: number;
  verify_allowed: boolean;
}

export interface PolicySetParams {
  allow_repository_to_admin_managed?: boolean | null;
  allow_sensitive_to_owner_estate?: boolean | null;
  mode?: string | null;
  mutations_allowed?: boolean | null;
  verify_allowed?: boolean | null;
}

export interface PolicySetResult {
  allow_repository_to_admin_managed: boolean;
  allow_sensitive_to_owner_estate: boolean;
  mode: string;
  mutations_allowed: boolean;
  ok: boolean;
  policy_epoch: number;
  verify_allowed: boolean;
}

export interface ProjectConsolidateParams {
  session_id: string;
}

export interface ProjectConsolidateResult {
  digest_chars?: number | null;
  ok: boolean;
}

export interface RunCancelParams {
  run_id: string;
}

export interface RunResumeGap {
  earliest_available: number;
  reason: string;
  requested_after: number;
  snapshot_sequence?: number | null;
}

export interface RunResumeParams {
  after_sequence: number;
  limit?: number | null;
  run_id: string;
}

export interface RunResumeResult {
  events?: unknown[];
  gap?: RunResumeGap | null;
  run_id: string;
}

export interface RunSnapshotParams {
  run_id: string;
}

export interface RunSnapshotResult {
  run_id: string;
  sequence: number;
  snapshot: unknown;
  state: string;
}

export interface RunStatusParams {
  error?: string | null;
  /** `started` | `ok` | `error` | `canceled`. */
  status: string;
}

export interface SessionCancelParams {
  session_id: string;
}

export interface SessionEndParams {
  error?: string | null;
  session_id: string;
  status?: string | null;
}

export interface SessionEndResult {
  ok: boolean;
}

export interface SessionInferenceParams {
  session_id: string;
}

export interface SessionInferenceResult {
  available_profiles: string[];
  model_fast: string;
  model_hard: string;
  profile: string;
  revision: number;
}

export interface SessionModelsResult {
  models: ModelChoice[];
  revision: number;
}

export interface SessionReclassifyParams {
  data_class: string;
  reason: string;
  session_id: string;
}

export interface SessionReclassifyResult {
  data_class: string;
  ok: boolean;
  previous_data_class?: string | null;
}

export interface SessionSelectModelParams {
  expected_revision: number;
  selection_id: string;
  session_id: string;
}

export interface SessionSetInferenceParams {
  expected_revision: number;
  model_fast: string;
  model_hard: string;
  profile: string;
  session_id: string;
}

export interface SessionStartParams {
  /** Inject repo briefing on first turn (default true when omitted). */
  briefing?: boolean | null;
  /** Post-edit critic pass after mutating specialist work (default true when orchestration is auto). */
  critic?: boolean | null;
  /** Override session data class (M2-2: `public` | `repository_source` | `sensitive_source` | `secret`; legacy aliases `private` | `personal` | `circle_ok` accepted). */
  data_class?: string | null;
  goal?: string | null;
  /** Use one-line LLM router before keyword fallback (default false). */
  llm_router?: boolean | null;
  /** Reserved for the router (Phase C): which capability tier to target. */
  model_tier?: string | null;
  /** Orchestration mode: `single` (default) or `auto` (router + specialists + critic). */
  orchestration?: string | null;
  /** When true, rebuild `Conversation` from audit `messages` instead of starting fresh. */
  resume?: boolean | null;
  /** Session to resume (required when multiple sessions exist; defaults to latest for workspace). */
  session_id?: string | null;
  /** Optional verify-before-finish command run in the workspace when the agent finishes (e.g. "pytest -q"). On failure the agent must fix and finish again. A trusted, host-supplied allowlist entry — not model-driven shell. */
  verify_cmd?: string | null;
}

export interface SessionStartResult {
  /** Effective session data class after floor coercion (SEC2-E2-026). */
  data_class?: string;
  /** Messages loaded into the working conversation (0 when not resuming). */
  messages_loaded?: number;
  /** `fresh` | `continued` | `incomplete` — session continuity hint (AC2-5). */
  resume_state?: string;
  /** True when an existing audit session was reopened (AR1-1). */
  resumed?: boolean;
  session_id: string;
}

export interface TokenParams {
  delta: string;
  role: string;
}

export interface ToolCallParams {
  args: unknown;
  /** `proposed` | `started`. */
  phase: string;
  tool: string;
  tool_call_id: string;
}

export interface ToolResultParams {
  error_kind?: string | null;
  ok: boolean;
  summary: string;
  tool_call_id: string;
}

export const Methods = {
  AGENT_SPAWN: "agent/spawn",
  APPROVAL_RESPOND: "approval/respond",
  CHAT_SEND: "chat/send",
  EGRESS_POLICY_GET: "egress/policy.get",
  EGRESS_POLICY_SET: "egress/policy.set",
  ESTATE_CAPACITY_CANCEL: "estate/capacity/cancel",
  ESTATE_CAPACITY_DOCTOR: "estate/capacity/doctor",
  ESTATE_CAPACITY_JOBS_CANCEL: "estate/capacity/jobs/cancel",
  ESTATE_CAPACITY_JOBS_GET: "estate/capacity/jobs/get",
  ESTATE_CAPACITY_OPTIMIZE: "estate/capacity/optimize",
  ESTATE_CAPACITY_PROFILES_ACTIVATE: "estate/capacity/profiles/activate",
  ESTATE_CAPACITY_PROFILES_EXPORT: "estate/capacity/profiles/export",
  ESTATE_CAPACITY_PROFILES_LIST: "estate/capacity/profiles/list",
  ESTATE_CAPACITY_PROFILES_ROLLBACK: "estate/capacity/profiles/rollback",
  ESTATE_CAPACITY_STATUS: "estate/capacity/status",
  ESTATE_STATUS: "estate/status",
  FABRIC_STATUS: "fabric/status",
  FABRIC_WORKER_TRUST_GET: "fabric/worker.trust.get",
  FABRIC_WORKER_TRUST_SET: "fabric/worker.trust.set",
  INITIALIZE: "initialize",
  MODEL_LIST: "model/list",
  POLICY_GET: "policy/get",
  POLICY_SET: "policy/set",
  PROJECT_CONSOLIDATE: "project/consolidate",
  RUN_CANCEL: "run/cancel",
  RUN_RESUME: "run/resume",
  RUN_SNAPSHOT: "run/snapshot",
  SECRET_FINGERPRINT_ALLOW: "secret/fingerprint/allow",
  SECRET_FINGERPRINT_REVOKE: "secret/fingerprint/revoke",
  SECRET_RULE_ADD: "secret/rule/add",
  SESSION_CANCEL: "session/cancel",
  SESSION_END: "session/end",
  SESSION_INFERENCE: "session/inference",
  SESSION_MODELS: "session/models",
  SESSION_RECLASSIFY: "session/reclassify",
  SESSION_SELECT_MODEL: "session/selectModel",
  SESSION_SET_INFERENCE: "session/setInference",
  SESSION_START: "session/start",
  SHUTDOWN: "shutdown",
} as const;

export const Events = {
  APPROVAL_REQUEST: "event/approval_request",
  CAPACITY_PROGRESS: "event/capacity/progress",
  CONTEXT: "event/context",
  DIFF: "event/diff",
  DISPATCH_PLACEMENT: "event/dispatch_placement",
  EGRESS: "event/egress",
  LOG: "event/log",
  RUN_STATUS: "event/run_status",
  TOKEN: "event/token",
  TOOL_CALL: "event/tool_call",
  TOOL_RESULT: "event/tool_result",
} as const;

export const ErrorCodes = {
  InternalError: -32603,
  InvalidParams: -32602,
  InvalidRequest: -32600,
  MethodNotFound: -32601,
  NotImplemented: -32000,
  NotReady: -32002,
  ParseError: -32700,
  UnknownSession: -32001,
} as const;
