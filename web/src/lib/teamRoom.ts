import type { Team } from '../types';
import type { WorkRecord } from './workEvidence';

export interface RoomMessage {
  id: string;
  teamId: string;
  threadId?: string;
  text: string;
  at: string;
}

export interface TeamWorkflow {
  id: string;
  title: string;
  teamIds: string[];
  agentIds: string[];
  records: WorkRecord[];
  operations: number;
  state: 'working' | 'waiting' | 'blocked' | 'settled';
}

// Project the same organization-wide evidence into every participating room.
// Never infer a causal link from timing alone, or duplicate a multi-team agent.
export function teamWorkflows(records: WorkRecord[], teams: Team[], exampleId: string) {
  const membership = new Map<string, string[]>();
  for (const team of teams)
    for (const id of team.pledgedAgentIds)
      membership.set(id, [...(membership.get(id) || []), team.id]);
  const groups = new Map<string, TeamWorkflow>();
  for (const record of records) {
    const interaction = record.interaction;
    const id = `trace:${exampleId}:${interaction.workflowId || interaction.id}`;
    let group = groups.get(id);
    if (!group) {
      group = {
        id,
        title: interaction.label.split(' / ')[0],
        teamIds: [],
        agentIds: [],
        records: [],
        operations: 0,
        state: 'working',
      };
      groups.set(id, group);
    }
    group.records.push(record);
    for (const agentId of [interaction.agentId, interaction.targetId]) {
      if (membership.has(agentId) && !group.agentIds.includes(agentId))
        group.agentIds.push(agentId);
      for (const teamId of membership.get(agentId) || [])
        if (!group.teamIds.includes(teamId)) group.teamIds.push(teamId);
    }
  }
  for (const group of groups.values()) {
    const latest = new Map<string, WorkRecord>();
    for (const record of group.records) latest.set(record.interaction.id, record);
    group.operations = latest.size;
    const states = [...latest.values()];
    // A completed retry clears its parent's failure; starting one does not.
    const recovered = new Set(
      states.filter((r) => r.state === 'completed').map((r) => r.interaction.retryOf),
    );
    group.state = states.some((r) => r.state === 'failed' && !recovered.has(r.interaction.id))
      ? 'blocked'
      : states.some((r) => r.state === 'started' || r.state === 'resumed')
        ? 'working'
        : states.some((r) => r.state === 'waiting')
          ? 'waiting'
          : 'settled';
  }
  return [...groups.values()];
}

export const workflowLabels = {
  working: 'Working',
  waiting: 'Waiting',
  blocked: 'Needs review',
  settled: 'Recorded steps ended',
};
