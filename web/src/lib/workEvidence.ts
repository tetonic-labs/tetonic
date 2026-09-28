import type { Interaction, InteractionEvent } from './graphMotion';
import type { MotionExample } from './motionPlayback';
import type { WorkState } from './workScene';
import type { StreamEvent } from '../types';

export interface WorkRecord {
  id: string;
  at: number;
  interaction: Interaction;
  state: 'started' | 'waiting' | 'resumed' | 'completed' | 'failed' | 'cancelled';
}
export const sampleTime = (at: number) =>
  `${Math.floor(at / 60)}:${String(Math.floor(at % 60)).padStart(2, '0')}`;

export function evidenceFor(example: MotionExample | undefined): WorkRecord[] {
  const actions = new Map<string, Interaction>();
  return (example?.events || []).flatMap((event: InteractionEvent) => {
    if (event.type === 'start') actions.set(event.interaction.id, event.interaction);
    const interaction =
      event.type === 'start' ? event.interaction : actions.get(event.interactionId);
    if (!interaction) return [];
    const state =
      event.type === 'start'
        ? 'started'
        : event.type === 'end'
          ? event.outcome
          : event.type === 'wait'
            ? 'waiting'
            : 'resumed';
    return [{ id: event.id, at: event.at, interaction, state }];
  });
}

export function workStatus(work: WorkState | undefined) {
  if (work?.failures.length)
    return work.failures.some((f) => f.retryId) ? 'Recovery in progress' : 'Unresolved work';
  if (work?.waiting) return 'Waiting · cause not recorded';
  if (work?.interaction) return `Working with ${work.interaction.targetName}`;
  if (work?.previous)
    return work.previous.outcome === 'cancelled' ? 'Work cancelled' : 'Last operation completed';
  return 'No activity recorded in this sample';
}

export function evidenceEvents(records: WorkRecord[]): StreamEvent[] {
  return records.map((record) => ({
    id: `playback-${record.id}`,
    agentId: record.interaction.agentId,
    agentName: record.interaction.agentId,
    timestamp: `${sampleTime(record.at)} · sample`,
    type: record.state === 'started' ? 'tool_call' : 'tool_result',
    content: `${record.state.charAt(0).toUpperCase() + record.state.slice(1)}: ${record.interaction.label}\nTarget: ${record.interaction.targetName}${record.interaction.retryOf ? '\nRetry of: ' + record.interaction.retryOf : ''}`,
    metadata: { toolName: record.interaction.targetName, workState: record.state },
  }));
}
