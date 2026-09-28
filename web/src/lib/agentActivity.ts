import { Agent, ApprovalRequest, StreamEvent } from '../types';

export function eventSummary(event: StreamEvent) {
  if (event.metadata?.workState) return event.content.split('\n')[0];
  if (event.type === 'tool_result')
    return (
      (event.metadata?.toolName || 'Tool') +
      (event.metadata?.exitCode === undefined
        ? ' returned a result'
        : ' finished · exit ' + event.metadata.exitCode)
    );
  if (event.type === 'tool_call')
    return 'Tool request · ' + (event.metadata?.toolName || 'Open for details');
  if (event.type === 'approval_badge') return 'Asked for a decision';
  return event.content.split('\n')[0].replaceAll('**', '').replaceAll('`', '');
}

export function latestRecordedEvent(agentId: string, events: StreamEvent[]) {
  return [...events].reverse().find((e) => e.agentId === agentId && e.type !== 'thought');
}

export function activityLabel(agent: Agent, approvals: ApprovalRequest[]) {
  const pending = approvals.filter((a) => a.agentId === agent.id && a.status === 'pending').length;
  if (pending) return { label: 'Needs your say', kind: 'waiting', pending };
  const labels = {
    idle: 'Ready',
    thinking: 'Thinking',
    executing: 'Working',
    waiting_approval: 'Awaiting an update',
    paused: 'Paused',
  };
  return {
    label: labels[agent.status],
    kind: agent.status === 'thinking' || agent.status === 'executing' ? 'working' : 'quiet',
    pending: 0,
  };
}
