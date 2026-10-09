// Local API v1 wire declarations. Server authority lives in tetonic-app; these
// types describe responses and requests, not runtime validation or UI state.

export interface WorkBrief {
  work_id: string;
  revision: number;
  body: string;
  request_id: string;
  created_by: string;
}

export interface PlanAssignment {
  key: string;
  title: string;
  instructions: string;
  agent_key: string;
  depends_on: string[];
  tools: string[];
  deliverable: string;
  token_budget: number;
}

export interface PlanContent {
  title: string;
  summary: string;
  token_budget: number;
  open_questions: string[];
  assignments: PlanAssignment[];
}

export interface HuddlePlan {
  work_id: string;
  revision: number;
  brief_revision: number;
  request_id: string;
  generation_id: string;
  status: 'drafting' | 'draft' | 'agreed';
  content: PlanContent | null;
  created_by: string;
  agreed_by: string | null;
  agreement_id: string | null;
}

export interface PlanExecutionView {
  coordinator?: CoordinationModel | null;
  directions?: PlanDirection[];
  receipt: {
    source_work_id: string;
    request_id: string;
    revision: number;
    brief?: string;
    brief_revision?: number;
    root_work_id: string;
    content: PlanContent;
    assignments: {
      assignment_key: string;
      work_id: string;
      agent_key: string;
      definition_digest: string;
    }[];
  };
  state: EngineTaskState;
  root: EngineTask | null;
  assignments: EngineTask[];
  error: string | null;
}

export interface PlanTaskLink {
  source_work_id: string;
  root_work_id: string;
  assignment_key: string | null;
  title: string;
  depends_on: string[];
}

export interface PlanView {
  continuation_from?: PlanContinuation | null;
  continuation_to?: PlanContinuation | null;
  recovery?: {
    available: boolean;
    reason: string | null;
    retained_count: number;
    unfinished_count: number;
  } | null;
  coordinator?: CoordinationModel | null;
  setup_issues?: { agent_key: string; message: string }[];
  execution_max_seconds?: number | null;
  execution?: PlanExecutionView | null;
  plans: HuddlePlan[];
  generation: EngineTask | null;
  brief_revision: number;
  readiness: string[];
  execution_available: boolean;
}

export interface CoordinationModel {
  provider: string;
  model: string;
}

export interface StartPlanRequest {
  request_id: string;
  revision: number;
  coordinator?: CoordinationModel;
  hosted_coordination_consent?: boolean;
  reviewed_previous_actions?: boolean;
}

export interface ContinuePlanRequest {
  request_id: string;
  expected_root_work_id: string;
}

export interface PlanContinuation {
  source_work_id: string;
  root_work_id: string;
  continuation_work_id: string;
  request_id: string;
  retained: { work_id: string; title: string }[];
  review_before_repeat: { work_id: string; title: string }[];
  created_by: string;
}

export type PlanCommand =
  | {
      action: 'prepare';
      request_id: string;
      expected_revision: number;
      expected_brief_revision: number;
      body: string;
    }
  | { action: 'generate'; request_id: string; expected_revision: number; brief_revision: number }
  | { action: 'capture'; revision: number }
  | {
      action: 'revise';
      request_id: string;
      expected_revision: number;
      brief_revision: number;
      content: PlanContent;
    }
  | { action: 'agree'; request_id: string; revision: number };

export interface EngineMessage {
  id: number;
  role: 'assistant' | 'tool';
  content: string;
}

export type EngineTaskState =
  | 'waiting_human'
  | 'not_started'
  | 'starting'
  | 'running'
  | 'canceling'
  | 'canceled'
  | 'failed'
  | 'completed'
  | 'recovery_required';

export interface EngineTask {
  work_team?: WorkTeam | null;
  human_questions?: WorkHumanQuestion[];
  plan?: PlanTaskLink | null;
  planning_for?: string | null;
  purpose?: 'work' | 'explore';
  parent_id?: string | null;
  error?: string | null;
  id: string;
  input: string;
  agent_key: string;
  agent_name: string;
  state: EngineTaskState;
  run_id: string | null;
  sequence: number;
  messages: EngineMessage[];
}

export interface EngineWorkspace {
  work_teams?: WorkTeam[];
  usage?: WorkUsage[];
  budget_setting?: BudgetSetting;
  budget_max_tokens?: number;
  planning_tasks?: EngineTask[];
  shaping_agent_key?: string;
  organization: string;
  team_id: string;
  team_name: string;
  agent_id: string;
  agent_name: string;
  model: string;
  input_limit: number;
  agents: EngineAgent[];
  tasks: EngineTask[];
}

export interface BudgetSetting {
  revision: number;
  token_limit: number | null;
}

export interface WorkUsage {
  work_id: string;
  title: string;
  purpose: string;
  budget: {
    token_limit: number;
    reserved_tokens: number;
    delegated_tokens: number;
    available_tokens: number;
    root_work_id: string;
  } | null;
  input_tokens: number;
  output_tokens: number;
  calls: number;
  pending_calls: number;
  unknown_calls: number;
  held_tokens: number;
  released_tokens: number;
  over_limit: boolean;
}

export interface EngineAgent {
  definition_digest?: string;
  editable?: boolean;
  tool_disclosure?: {
    version: number;
    provider: string;
    endpoint: string;
    tools: string[];
    workspace: string | null;
  } | null;
  hosted_workspace?: string | null;
  workspace_root?: string | null;
  plan_coordinator?: boolean;
  provider?: string;
  hosted_consent?: boolean;
  key: string;
  id: string;
  name: string;
  purpose: string;
  model: string;
  harness: string;
  max_steps: number;
  max_seconds: number;
  max_tokens: number;
  tools?: string[];
}

export interface WorkspaceSkill {
  id: string;
  name: string;
  description: string;
  source: string;
  enabled: boolean;
  created_at: string;
}

export interface AgentCatalog {
  workspace_folders?: string[];
  default_steps?: number;
  default_seconds?: number;
  mcp_management?: boolean;
  skills?: WorkspaceSkill[];
  mcp_connections?: McpConnection[];
  workspace_root?: string | null;
  runtime_profiles?: AgentRuntimeProfile[];
  providers?: EngineProvider[];
  local_error?: string | null;
  models: string[];
  harnesses: string[];
  tools?: string[];
  max_steps: number;
  max_seconds: number;
  max_tokens: number;
}

export interface McpTool {
  approved?: boolean;
  id: string;
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
}

export interface McpConnection {
  id: string;
  name: string;
  endpoint: string;
  status: 'unchecked' | 'discovered' | 'unavailable' | 'disconnected';
  editable?: boolean;
  revision?: number;
  auth?: 'none' | 'bearer';
  enabled?: boolean;
  message: string;
  tools: McpTool[];
}

export interface AgentRuntimeProfile {
  requires_tool_consent?: boolean;
  provider: string;
  harness: string;
  tools: string[];
  tool_restriction: string | null;
}

export interface EngineProvider {
  id: string;
  name: string;
  key_saved: boolean;
}

export interface ProviderModelCatalog {
  provider: string;
  models: string[];
  entries?: { id: string; display_name: string | null; created_at: number | null }[];
  fetched_at?: string;
  capabilities_verified: boolean;
}

export type CreateEngineAgent = Omit<EngineAgent, 'id' | 'key'> & {
  request_id: string;
  tools?: string[];
  hosted_tools_consent?: boolean;
  expected_workspace_root?: string;
};

export interface WorkHumanQuestion {
  id: string;
  work_id: string;
  source_work_id: string;
  attempt_id: string;
  content: { question: string; why: string; options: string[] };
  deadline: number;
  answer: string | null;
  response_id: string | null;
}

export interface AnswerPlanQuestion {
  request_id: string;
  question_id: string;
  answer: string;
}

export interface AmendPlanAssignment {
  request_id: string;
  expected_revision: number;
  assignment_key: string;
  instructions: string;
}

export interface PlanDirection {
  revision: number;
  request_id: string;
  assignment_key: string;
  instructions: string;
  affected_work_ids: string[];
  retained_work_ids: string[];
  actor: string;
}

export interface LocalWorkItem {
  id: string;
  title: string;
  status: string;
  agent_key?: string | null;
  goal_id?: string | null;
  run_id?: string | null;
  request_id: string;
  version: number;
  notes?: string[];
  lead_id?: string | null;
  agent_ids?: string[];
}

export interface LocalApproval {
  proposal?: {
    command: string;
    working_directory: string;
    shell: string;
    attempt_id: string;
    call_id: string;
    parameter_digest: string;
    confinement_warnings: string[];
  } | null;
  org_id: string;
  team_id: string;
  approval_id: string;
  work_id?: string | null;
  proposal_digest: string;
  status: string;
  request_id: string;
  expires_at: number;
  created_by?: string;
  resolved_by?: string | null;
}

export interface LocalControlStop {
  org_id: string;
  scope_kind: string;
  scope_id: string;
  mode: string;
  reason: string;
}

export interface LocalApprovalsInspection {
  active_stops: LocalControlStop[];
  pending_approvals: LocalApproval[];
  effort: unknown[];
}

export interface WorkTeamSelection {
  id: string;
  revision: number;
}

export interface WorkTeam extends WorkTeamSelection {
  name: string;
  purpose: string;
  agent_keys: string[];
}

export interface SaveWorkTeam {
  id: string;
  request_id: string;
  expected_revision: number;
  name: string;
  purpose: string;
  agent_keys: string[];
}

export interface LocalTeamInfo {
  id: string;
  name: string;
  org_id: string;
}

export interface LocalDigestResponse {
  summary: string;
  total_work_items: number;
  completed_items: number;
  active_items: number;
  pending_approvals_count: number;
  highlights: string[];
}
