export type AgentStatus = 'idle' | 'thinking' | 'executing' | 'waiting_approval' | 'paused';

export interface Agent {
  id: string;
  name: string;
  charter: string;
  model: string;
  status: AgentStatus;
  decisionIntervalMs: number;
  capabilities: string[];
  tokensProcessed: number;
  memoryItemsCount: number;
  isLocalToCastle: boolean;
  pledgedTeamId?: string;
  lastActive: string;
}

export type ApprovalType =
  | 'bash_command'
  | 'file_write'
  | 'cross_castle_request'
  | 'effect'
  | 'network_egress';

export interface CodeDiff {
  filepath: string;
  oldLines: { num: number; text: string }[];
  newLines: { num: number; text: string }[];
  summary: string;
}

export interface ApprovalRequest {
  source?: 'engine';
  proposalDigest?: string;
  effectUnavailable?: boolean;
  expiresAt?: number;
  id: string;
  agentId: string;
  agentName: string;
  type: ApprovalType;
  title: string;
  reason: string;
  payload: string; // e.g. "cargo test -p tetonic-core" or target path
  diff?: CodeDiff;
  status: 'pending' | 'approved' | 'rejected';
  requestedAt: string;
  expiresInSecs: number;
  castleOrigin: string; // e.g. "Alice's MacBook (Local)" or "Susan's Workstation (Remote)"
}

export interface CastleNode {
  hostname: string;
  castleName: string;
  ip: string;
  mode: 'sovereign_castle';
  inferenceProvider: {
    name: string;
    endpoint: string;
    model: string;
    status: 'online' | 'degraded' | 'offline';
    latencyMs: number;
    contextTokensLimit: number;
  };
  resources: {
    cpuPercent: number;
    memoryMbUsed: number;
    memoryMbTotal: number;
    activeProcessesCount: number;
  };
  policy: {
    requireApprovalForShell: boolean;
    requireApprovalForFileWrites: boolean;
    allowRemoteCastleExec: boolean;
    sandboxingLevel: 'strict' | 'standard' | 'unrestricted';
  };
}

export interface TeamMember {
  id: string;
  name: string;
  handle: string;
  avatarUrl: string;
  role: 'owner' | 'lead' | 'member' | 'guest';
  isCurrentUser: boolean;
}

export interface Team {
  id: string;
  name: string;
  tagline: string;
  isPersonal: boolean;
  members: TeamMember[];
  pledgedAgentIds: string[];
  createdAt: string;
}

export interface StreamEvent {
  recipientAgentId?: string;
  teamId?: string;
  id: string;
  timestamp: string;
  agentId: string;
  agentName: string;
  type: 'thought' | 'tool_call' | 'tool_result' | 'message' | 'approval_badge';
  content: string;
  metadata?: {
    toolName?: string;
    durationMs?: number;
    exitCode?: number;
    tokenCount?: number;
    diffSnippet?: string;
    workState?: string;
  };
}

export type GraphNodeType = 'agent' | 'tool' | 'connector' | 'storage' | 'inference' | 'human';

export interface GraphNode {
  id: string;
  type: GraphNodeType;
  name: string;
  label: string;
  category: 'castle' | 'team' | 'external';
  x: number; // Virtual coordinate
  y: number;
  status: AgentStatus | 'active' | 'offline';
  statusLabel?: string;
  iconType: string;
  metadata?: {
    model?: string;
    charter?: string;
    capabilities?: string[];
    endpoint?: string;
    latencyMs?: number;
    connectorType?: 'github' | 'jira' | 'google' | 'sqlite' | 'ollama';
    details?: string;
  };
}

export type GraphEdgeType =
  | 'tool_usage'
  | 'mcp_query'
  | 'agent_comm'
  | 'pledge_link'
  | 'castle_gate';

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  type: GraphEdgeType;
  label?: string;
  isActive: boolean;
  activityLabel?: string;
  pulseColor?: string;
}

export type TrackStepType =
  | 'directive'
  | 'thought'
  | 'mcp_query'
  | 'tool_execution'
  | 'peer_comm'
  | 'gate_approval'
  | 'outcome';

export interface TrackStep {
  id: string;
  stepNumber: number;
  timestamp: string;
  type: TrackStepType;
  title: string;
  targetNodeId?: string;
  targetNodeName?: string;
  inputPayload?: string;
  outputPayload?: string;
  durationMs?: number;
  tokens?: number;
  status: 'success' | 'running' | 'pending' | 'failed';
  diffSnippet?: string;
}

export interface AgentTrack {
  agentId: string;
  agentName: string;
  currentStatus: string;
  totalTokens: number;
  steps: TrackStep[];
}
