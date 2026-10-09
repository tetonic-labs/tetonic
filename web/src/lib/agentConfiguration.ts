import type { WorkspaceResource } from './toolLibrary';

export type AgentToolId = 'read_file' | 'write_file' | 'run_shell' | 'recall';
export type PermissionRule = 'ask' | 'deny' | 'scoped';
export interface AgentConfiguration {
  harness: 'general' | 'coding';
  toolIds: AgentToolId[];
  resourceIds: string[];
  permissions: { fileChanges: PermissionRule; shell: PermissionRule; network: PermissionRule };
  scope: { workspacePath: string; context: 'task' | 'team'; requests: 'owner' | 'team_approval' };
  limits: { maxSteps: number; maxSeconds: number; maxTokens: number };
}
export interface AgentDraft {
  /** Exact connected tool names selected at click time, never a permission grant. */
  tools?: string[];
  provider?: string;
  hostedConsent?: boolean;
  hostedToolsConsent?: boolean;
  expectedWorkspaceRoot?: string;
  workspaceRoot?: string;
  name: string;
  purpose: string;
  teamId: string;
  model: string;
  configuration: AgentConfiguration;
}

// Requested preview configuration, never an execution grant or an installed-tool inventory.
export const agentTools: { id: AgentToolId; name: string; description: string }[] = [
  { id: 'read_file', name: 'Read files', description: 'Read within a working folder' },
  { id: 'write_file', name: 'Write files', description: 'Create and change files' },
  { id: 'run_shell', name: 'Terminal', description: 'Run workspace commands' },
  { id: 'recall', name: 'Recall', description: 'Retrieve permitted context' },
];
export const harnesses = [
  {
    id: 'general',
    name: 'General purpose',
    description: 'Reason, answer, and use the tools you choose.',
  },
  { id: 'coding', name: 'Coding', description: 'Plan, edit, and verify work in a code workspace.' },
] as const;
export function defaultAgentConfiguration(): AgentConfiguration {
  return {
    harness: 'general',
    toolIds: [],
    resourceIds: [],
    permissions: { fileChanges: 'ask', shell: 'ask', network: 'ask' },
    scope: { workspacePath: '', context: 'task', requests: 'owner' },
    limits: { maxSteps: 20, maxSeconds: 120, maxTokens: 4096 },
  };
}
export function availableAgentResources(resources: WorkspaceResource[], teamId: string) {
  return resources.filter((resource) =>
    teamId ? resource.teamIds.includes(teamId) : !resource.teamIds.length,
  );
}
export function configurationForTeam(
  configuration: AgentConfiguration,
  resources: WorkspaceResource[],
  teamId: string,
): AgentConfiguration {
  const available = new Set(
    availableAgentResources(resources, teamId).map((resource) => resource.id),
  );
  return {
    ...configuration,
    resourceIds: configuration.resourceIds.filter((id) => available.has(id)),
    scope: teamId
      ? configuration.scope
      : { ...configuration.scope, context: 'task', requests: 'owner' },
  };
}
export const permissionLabels: Record<PermissionRule, string> = {
  ask: 'Ask first',
  deny: 'Blocked',
  scoped: 'Within selected scope',
};
