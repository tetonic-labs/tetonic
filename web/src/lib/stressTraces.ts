import type { Agent } from '../types';
import type { Destination } from './mapActivity';
import type { Interaction, InteractionEvent } from './graphMotion';
import type { MotionExample } from './motionPlayback';
import { teammate } from './teammates';

// Deterministic fixtures: replaying a load reproduces the same contention.
export function stressTraces(agents: Agent[], places: Destination[]): MotionExample[] {
  if (!agents.length || !places.length) return [];
  return [
    { id: 'stress-sustained', name: 'Sustained load', seconds: 180, cycle: 16, hotspot: false },
    {
      id: 'stress-contention',
      name: 'Shared-tool contention',
      seconds: 360,
      cycle: 22,
      hotspot: true,
    },
    {
      id: 'stress-burst',
      name: 'Burst, recover & repeat',
      seconds: 600,
      cycle: 28,
      hotspot: false,
    },
  ].map((config) => {
    const events: InteractionEvent[] = [];
    for (let round = 0; round * config.cycle < config.seconds - config.cycle; round++) {
      agents.forEach((agent, index) => {
        const wave = config.id === 'stress-burst';
        const at = round * config.cycle + (wave ? (index % 12) * 0.035 : (index % 11) * 0.19) + 0.2;
        const target =
          places[
            (config.hotspot ? Math.floor(index / 10) + round : index + round * 3) % places.length
          ];
        const interaction: Interaction = {
          id: `${config.id}-${round}-${agent.id}`,
          agentId: agent.id,
          targetId: target.id,
          targetName: target.name,
          tool: target.kind === 'tool' ? 'Terminal' : 'MCP',
          label: `${config.name}: wave ${round + 1}`,
        };
        const emit = (suffix: string, time: number, kind: 'wait' | 'resume' | 'end') => {
          const base = {
            id: `${interaction.id}-${suffix}`,
            at: time,
            agentId: agent.id,
            interactionId: interaction.id,
          };
          if (kind === 'end')
            events.push({
              ...base,
              type: kind,
              outcome:
                (index + round) % 13 === 0
                  ? 'cancelled'
                  : (index + round) % 9 === 0
                    ? 'failed'
                    : 'completed',
            });
          else events.push({ ...base, type: kind });
        };
        events.push({ id: `${interaction.id}-start`, at, type: 'start', interaction });
        if ((index + round) % 4 === 0) {
          emit('wait', at + 3, 'wait');
          emit('resume', at + 7, 'resume');
        }
        const end = at + (wave ? 10 + (index % 4) : config.cycle - 6);
        emit('end', end, 'end');
        // Only the first agent in a pair initiates a peer handoff: no cycles.
        if (index % 2 === 0 && agents[index + 1] && round % 3 === 1) {
          const peer = agents[index + 1];
          const handoff: Interaction = {
            id: `${interaction.id}-handoff`,
            agentId: agent.id,
            targetId: peer.id,
            targetName: teammate(peer).name,
            tool: 'Message',
            label: 'Peer handoff',
          };
          events.push({
            id: `${handoff.id}-start`,
            at: end + 0.4,
            type: 'start',
            interaction: handoff,
          });
          events.push({
            id: `${handoff.id}-end`,
            at: at + config.cycle - 1,
            type: 'end',
            agentId: agent.id,
            interactionId: handoff.id,
            outcome: 'completed',
          });
        }
      });
    }
    events.sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
    return {
      id: config.id,
      name: `${config.name} · ${config.seconds / 60} min`,
      duration: config.seconds,
      events,
      provenance: `Synthetic stress test · ${agents.length} agents · ${places.length} resources · ${events.length.toLocaleString()} events · no engine work is running`,
    };
  });
}
