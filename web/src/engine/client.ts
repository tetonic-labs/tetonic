import type {
  WorkBrief,
  HuddlePlan,
  PlanExecutionView,
  PlanView,
  StartPlanRequest,
  ContinuePlanRequest,
  PlanContinuation,
  PlanCommand,
  EngineTask,
  EngineWorkspace,
  BudgetSetting,
  EngineAgent,
  WorkspaceSkill,
  AgentCatalog,
  McpConnection,
  EngineProvider,
  ProviderModelCatalog,
  CreateEngineAgent,
  WorkHumanQuestion,
  AnswerPlanQuestion,
  AmendPlanAssignment,
  PlanDirection,
  LocalWorkItem,
  LocalApproval,
  LocalApprovalsInspection,
  WorkTeamSelection,
  WorkTeam,
  SaveWorkTeam,
  ScopedCapabilityPolicy,
  SaveCapabilityPolicy,
  LocalTeamInfo,
  LocalDigestResponse,
} from './contracts';
import { readEngineFailure, EngineRequestError } from './failure';

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
          'X-Tetonic-Api-Version': '1',
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
      const version = response.headers?.get('x-tetonic-api-version');
      if (version && version !== '1')
        throw new Error(
          'The engine and this app use different API versions. Update the app and reconnect.',
        );
      if (!response.ok) {
        const failure = readEngineFailure(value);
        throw new EngineRequestError(
          failure.message,
          response.status,
          failure.code,
          failure.recovery,
          failure.recoveryHint,
        );
      }
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
  importSkill(input: { content: string; source: string }) {
    return this.request<WorkspaceSkill>('/skills', input);
  }
  revokeSkill(id: string) {
    return this.request<WorkspaceSkill[]>('/skills/revoke', { id });
  }
  skillContent(id: string, signal?: AbortSignal) {
    return this.request<{ content: string }>(
      `/skills/${encodeURIComponent(id)}`,
      undefined,
      signal,
    );
  }
  discoverMcp(id: string) {
    return this.request<McpConnection>(`/mcp-discover/${encodeURIComponent(id)}`, {});
  }
  saveMcpConnection(input: {
    id: string;
    expected_revision: number;
    name: string;
    endpoint: string;
    auth: 'none' | 'bearer';
    token?: string;
    enabled: boolean;
    approved_tools?: string[];
  }) {
    return this.request<McpConnection>('/mcp-connections', input);
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
    work_team?: WorkTeamSelection,
  ) {
    return this.request<EngineTask>('/tasks', {
      request_id,
      input,
      agent_key,
      ...(parent_id ? { parent_id } : {}),
      ...(purpose ? { purpose } : {}),
      ...(work_team ? { work_team } : {}),
    });
  }
  saveWorkTeam(request: SaveWorkTeam) {
    return this.request<WorkTeam>('/work-teams', request);
  }
  capabilityPolicies(signal?: AbortSignal) {
    return this.request<ScopedCapabilityPolicy[]>('/capability-policies', undefined, signal);
  }
  saveCapabilityPolicy(request: SaveCapabilityPolicy) {
    return this.request<ScopedCapabilityPolicy>('/capability-policies', request);
  }
  briefs(id: string, signal?: AbortSignal) {
    return this.request<WorkBrief[]>(`/briefs/${encodeURIComponent(id)}`, undefined, signal);
  }
  startPlan(id: string, request: StartPlanRequest) {
    return this.request<PlanExecutionView>(`/plans/${encodeURIComponent(id)}/start`, request);
  }
  continuePlan(id: string, request: ContinuePlanRequest) {
    return this.request<PlanContinuation>(`/plans/${encodeURIComponent(id)}/continue`, request);
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
