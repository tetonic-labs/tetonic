export type MissionStatus = 'proposed' | 'in_progress' | 'settled' | 'needs_attention';

export interface LineageTradeoff {
  option: string;
  pros: string;
  cons: string;
  chosen?: boolean;
}

export interface LineageAction {
  id: string;
  timestamp: string;
  agentId: string;
  agentName: string;
  actionType: 'read' | 'edit' | 'test' | 'command' | 'review' | 'decision';
  summary: string;
  target?: string;
  diff?: string;
  rawLog?: string;
}

export interface LineageMilestone {
  id: string;
  number: string;
  title: string;
  summary: string;
  status: 'done' | 'active' | 'pending';
  timestamp?: string;
  tradeoffs?: LineageTradeoff[];
  actions?: LineageAction[];
  verification?: {
    suite: string;
    passed: number;
    failed: number;
    benchmark?: string;
    details?: string;
  };
  artifact?: {
    branch?: string;
    commitHash?: string;
    filesCount?: number;
    summary?: string;
  };
}

export interface MissionWithLineage {
  id: string;
  title: string;
  context: string;
  status: MissionStatus;
  stageLabel: string;
  leadAgentId: string;
  leadAgentName: string;
  collaboratorIds: string[];
  startedAt: string;
  completedAt?: string;
  executiveSummary: string;
  whyThisMatters?: string;
  error?: string | null;
  runId?: string | null;
  model?: string;
  harness?: string;
  milestones: LineageMilestone[];
  liveElapsedSeconds?: number;
  liveToolSnippet?: string;
  progressPercent?: number;
}
