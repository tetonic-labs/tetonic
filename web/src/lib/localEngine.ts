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
  execution_max_seconds?: number | null;
  execution?: PlanExecutionView | null;
  plans: HuddlePlan[];
  generation: EngineTask | null;
  brief_revision: number;
  readiness: string[];
  execution_available: boolean;
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
export interface AgentCatalog {
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
  id: string;
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
}
export interface McpConnection {
  id: string;
  name: string;
  endpoint: string;
  status: 'unchecked' | 'discovered' | 'unavailable';
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
  capabilities_verified: boolean;
}
export type CreateEngineAgent = Omit<EngineAgent, 'id' | 'key'> & {
  request_id: string;
  tools?: string[];
  hosted_tools_consent?: boolean;
  expected_workspace_root?: string;
};
export const engineStates: Record<EngineTaskState, string> = {
  waiting_human: 'Needs your input',
  not_started: 'Not started',
  starting: 'Starting',
  running: 'Working',
  canceling: 'Stopping',
  canceled: 'Stopped',
  failed: 'Failed',
  completed: 'Completed',
  recovery_required: 'Interrupted · needs review',
};
export const taskIsActive = (task: EngineTask) =>
  ['starting', 'running', 'waiting_human', 'canceling'].includes(task.state);

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

const tokenKey = 'tetonic_local_session';
const scopeKey = 'tetonic_draft_session';
export function connectionDraftScope() {
  try {
    let scope = sessionStorage.getItem(scopeKey);
    if (!scope) {
      scope = crypto.randomUUID();
      sessionStorage.setItem(scopeKey, scope);
    }
    return scope;
  } catch {
    return 'this-tab';
  }
}
export function takeConnectionToken() {
  const fragment = new URLSearchParams(window.location.hash.slice(1));
  const supplied = fragment.get('connect');
  if (supplied) {
    // Remove the credential from the visible URL before issuing requests.
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
    if (!/^[a-f0-9]{64}$/.test(supplied)) return '';
    try {
      if (sessionStorage.getItem(tokenKey) !== supplied)
        sessionStorage.setItem(scopeKey, crypto.randomUUID());
      sessionStorage.setItem(tokenKey, supplied);
    } catch {
      /* This tab can still connect. */
    }
    return supplied;
  }
  try {
    return sessionStorage.getItem(tokenKey) || '';
  } catch {
    return '';
  }
}

export class EngineRequestError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

export class LocalEngine {
  constructor(private token: string) {}
  async request<T>(path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    if (!this.token) throw new Error('Open the connection link printed by the local engine.');
    const deadline = new AbortController();
    const abort = () => deadline.abort();
    signal?.addEventListener('abort', abort, { once: true });
    if (signal?.aborted) abort();
    const timer = setTimeout(abort, body === undefined ? 10000 : 30000);
    try {
      const response = await fetch(`/api/local${path}`, {
        method: body === undefined ? 'GET' : 'POST',
        headers: {
          Authorization: `Bearer ${this.token}`,
          ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: deadline.signal,
      });
      let value: unknown;
      try {
        value = await response.json();
      } catch {
        throw new Error('The local engine is unavailable. Start it and reconnect.');
      }
      if (!response.ok)
        throw new EngineRequestError(
          (value as { error?: string }).error ||
            'The local engine could not complete this request.',
          response.status,
        );
      return value as T;
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
    }
  }
  snapshot(signal?: AbortSignal) {
    return this.request<EngineWorkspace>('/workspace', undefined, signal);
  }
  setBudgetSetting(input: {
    request_id: string;
    expected_revision: number;
    token_limit: number | null;
  }) {
    return this.request<BudgetSetting>('/budget-settings', input);
  }
  agentCatalog(signal?: AbortSignal) {
    return this.request<AgentCatalog>('/agent-catalog', undefined, signal);
  }
  discoverMcp(id: string) {
    return this.request<McpConnection>(`/mcp-discover/${encodeURIComponent(id)}`, {});
  }
  providerModels(provider: string, signal?: AbortSignal) {
    return this.request<ProviderModelCatalog>(
      `/provider-models/${encodeURIComponent(provider)}`,
      undefined,
      signal,
    );
  }
  createAgent(input: CreateEngineAgent) {
    return this.request<EngineAgent>('/agents', input);
  }
  updateAgent(agent: EngineAgent, configuration: CreateEngineAgent) {
    return this.request<EngineAgent>('/agents/update', {
      agent_key: agent.key,
      expected_definition_digest: agent.definition_digest,
      configuration,
    });
  }
  saveProviderKey(provider: string, api_key: string) {
    return this.request<EngineProvider>('/provider-key', { provider, api_key });
  }
  removeProviderKey(provider: string) {
    return this.request<EngineProvider>('/provider-key/remove', { provider });
  }
  submit(
    request_id: string,
    input: string,
    agent_key: string,
    parent_id?: string,
    purpose?: 'work' | 'explore',
  ) {
    return this.request<EngineTask>('/tasks', {
      request_id,
      input,
      agent_key,
      ...(parent_id ? { parent_id } : {}),
      ...(purpose ? { purpose } : {}),
    });
  }
  briefs(id: string, signal?: AbortSignal) {
    return this.request<WorkBrief[]>(`/briefs/${encodeURIComponent(id)}`, undefined, signal);
  }
  startPlan(id: string, request: { request_id: string; revision: number }) {
    return this.request<PlanExecutionView>(`/plans/${encodeURIComponent(id)}/start`, request);
  }
  answerPlanQuestion(id: string, request: AnswerPlanQuestion) {
    return this.request<WorkHumanQuestion>(`/tasks/${encodeURIComponent(id)}/answer`, request);
  }
  amendPlanAssignment(id: string, request: AmendPlanAssignment) {
    return this.request<PlanDirection>(`/plans/${encodeURIComponent(id)}/direction`, request);
  }
  plan(id: string, signal?: AbortSignal) {
    return this.request<PlanView>(`/plans/${encodeURIComponent(id)}`, undefined, signal);
  }
  updatePlan(id: string, command: PlanCommand) {
    return this.request<HuddlePlan>(`/plans/${encodeURIComponent(id)}`, command);
  }
  saveBrief(id: string, request: { request_id: string; expected_revision: number; body: string }) {
    return this.request<WorkBrief>(`/briefs/${encodeURIComponent(id)}`, request);
  }
  cancel(id: string) {
    return this.request<EngineTask>(`/tasks/${encodeURIComponent(id)}/cancel`, {});
  }
  workItems(signal?: AbortSignal) {
    return this.request<LocalWorkItem[]>('/work-items', undefined, signal);
  }
  createWorkItem(item: {
    id: string;
    title: string;
    goal_id?: string;
    agent_key?: string;
    lead_id?: string;
    agent_ids?: string[];
  }) {
    return this.request<LocalWorkItem>('/work-items', item);
  }
  patchWorkItem(
    id: string,
    patch: {
      notes?: string[];
      status?: string;
      lead_id?: string;
      agent_ids?: string[];
    },
  ) {
    return this.request<{ ok: boolean }>(`/work-items/${encodeURIComponent(id)}`, patch);
  }
  approvals(signal?: AbortSignal) {
    return this.request<LocalApprovalsInspection>('/approvals', undefined, signal);
  }
  resolveApproval(approval_id: string, allow: boolean, proposal_digest: string) {
    return this.request<LocalApproval>(`/approvals/${encodeURIComponent(approval_id)}/resolve`, {
      allow,
      proposal_digest,
    });
  }
  teams(signal?: AbortSignal) {
    return this.request<LocalTeamInfo[]>('/teams', undefined, signal);
  }
  digest(signal?: AbortSignal) {
    return this.request<LocalDigestResponse>('/digest', {}, signal);
  }
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
