import {
  waitingAfterAnswer,
  type LocalApproval,
  type EngineTask,
  type EngineAgent,
} from './localEngine';
import type { Agent, ApprovalRequest } from '../types';

export function engineApprovalToUI(
  approval: LocalApproval,
  _agents: Agent[] = [],
): ApprovalRequest {
  return {
    source: 'engine',
    proposalDigest: approval.proposal_digest,
    effectUnavailable: true,
    expiresAt: approval.expires_at,
    id: approval.approval_id,
    agentId: '',
    agentName: 'Your team',
    type: 'effect',
    title: `Approval Required: ${approval.approval_id}`,
    reason:
      'The engine supplied an authorization reference without the proposed action. Approval is unavailable until those details can be reviewed.',
    payload: 'Proposed action details unavailable.',
    status:
      approval.status === 'approved'
        ? 'approved'
        : approval.status === 'rejected'
          ? 'rejected'
          : 'pending',
    requestedAt: '',
    expiresInSecs: Math.max(0, approval.expires_at - Math.floor(Date.now() / 1000)),
    castleOrigin: 'Local Workstation (Governed Engine)',
  };
}

export function engineAgentToUI(engineAgent: EngineAgent, activeTasks: EngineTask[] = []): Agent {
  const isActive = activeTasks.some(
    (t) =>
      t.agent_key === engineAgent.key && ['running', 'starting', 'canceling'].includes(t.state),
  );
  const waiting = activeTasks.some(
    (t) => t.agent_key === engineAgent.key && t.state === 'waiting_human',
  );
  const needsInput = activeTasks.some(
    (t) => t.agent_key === engineAgent.key && t.state === 'waiting_human' && !waitingAfterAnswer(t),
  );
  return {
    id: engineAgent.key,
    name: engineAgent.name || engineAgent.key,
    charter: engineAgent.purpose || 'Autonomous digital worker assistant.',
    model: engineAgent.model,
    status: isActive ? 'executing' : waiting ? 'paused' : 'idle',
    decisionIntervalMs: 1500,
    capabilities: [
      'general',
      'finish',
      engineAgent.provider || 'ollama',
      ...(engineAgent.tools || []),
    ],
    tokensProcessed: 0,
    memoryItemsCount: 1,
    isLocalToCastle: true,
    pledgedTeamId: 'local-work',
    lastActive: isActive
      ? 'Just now'
      : needsInput
        ? 'Needs your input'
        : waiting
          ? 'Waiting to continue'
          : 'Idle',
  };
}
