import type { Agent } from '../types';
import type { Destination } from './mapActivity';
import type { InteractionEvent } from './graphMotion';
import type { MotionExample } from './motionPlayback';
import { teammate } from './teammates';

export function rushTrace(agents: Agent[], places: Destination[]): MotionExample[] {
  if (!agents.length || !places.length) return [];
  const ordered = [...agents].sort(
    (a, b) =>
      (a.pledgedTeamId || '').localeCompare(b.pledgedTeamId || '') || a.id.localeCompare(b.id),
  );
  // Interleave opposite halves so conversation partners frequently cross teams.
  const half = Math.ceil(ordered.length / 2);
  const events: InteractionEvent[] = [];
  const duration = 180;
  for (let round = 0; round < 55; round++) {
    const at = 0.15 + round * 3.2;
    for (let i = 0; i < half; i++) {
      const first = ordered[i],
        second = ordered[i + half];
      const pair = second ? [first, second] : [first];
      pair.forEach((agent, seat) => {
        const target = places[(i * 3 + round + seat * 5) % places.length];
        const start = at + (i % 5) * 0.025;
        const id = `rush-${round}-${agent.id}`;
        events.push({
          id: `${id}-start`,
          at: start,
          type: 'start',
          interaction: {
            id,
            agentId: agent.id,
            targetId: target.id,
            targetName: target.name,
            tool: target.kind === 'tool' ? 'Terminal' : 'MCP',
            label: `All-hands rush: ${round % 2 ? 'execute' : 'coordinate'} / ${target.name}`,
          },
        });
        events.push({
          id: `${id}-end`,
          at: start + 1.75,
          type: 'end',
          agentId: agent.id,
          interactionId: id,
          outcome: (round + i + seat) % 23 === 0 ? 'failed' : 'completed',
        });
      });
      if (second) {
        const sender = round % 2 ? second : first,
          receiver = round % 2 ? first : second;
        const id = `rush-chat-${round}-${i}`;
        events.push({
          id: `${id}-start`,
          at: at + 2,
          type: 'start',
          interaction: {
            id,
            agentId: sender.id,
            targetId: receiver.id,
            targetName: teammate(receiver).name,
            tool: 'Message',
            label: 'All-hands rush: exchange results / cross-team chat',
          },
        });
        events.push({
          id: `${id}-end`,
          at: at + 3,
          type: 'end',
          agentId: sender.id,
          interactionId: id,
          outcome: 'completed',
        });
      }
    }
  }
  events.sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
  return [
    {
      id: 'stress-rush',
      name: 'All-hands rush · 3 min',
      duration,
      events,
      provenance: `Synthetic maximum-activity trace · ${agents.length} agents · ${events.length.toLocaleString()} events · rapid work and paired conversations · not live`,
    },
  ];
}
