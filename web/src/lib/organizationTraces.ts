import type { Agent } from '../types';
import type { Destination } from './mapActivity';
import type { InteractionEvent, Outcome } from './graphMotion';
import type { MotionExample } from './motionPlayback';
import { teammate } from './teammates';

// Dependency-driven work items compete for agents, rather than moving everyone
// on one global beat. Reserving both handoff participants avoids peer cycles.
export function organizationTraces(agents: Agent[], places: Destination[]): MotionExample[] {
  if (!agents.length || !places.length) return [];
  const teams = new Map<string, Agent[]>();
  agents.forEach((a) => {
    const team = a.pledgedTeamId || 'unassigned';
    if (!teams.has(team)) teams.set(team, []);
    teams.get(team)!.push(a);
  });
  const groups = [...teams.values()];
  return [
    {
      id: 'org-release',
      name: 'Product delivery across teams',
      stages: ['Discover', 'Build', 'Review', 'Release'],
      spacing: 6.7,
      count: 30,
    },
    {
      id: 'org-incident',
      name: 'Incident response & recovery',
      stages: ['Triage', 'Investigate', 'Mitigate', 'Verify'],
      spacing: 4.3,
      count: 34,
    },
    {
      id: 'org-research',
      name: 'Research, synthesis & validation',
      stages: ['Collect', 'Analyze', 'Synthesize', 'Validate'],
      spacing: 8.9,
      count: 28,
    },
  ].map((config) => {
    const events: InteractionEvent[] = [];
    const available = new Map<string, number>();
    let serial = 0;
    const pick = (team: number, seat: number) => {
      const group = groups[team % groups.length];
      return group[seat % group.length];
    };
    function schedule(
      agent: Agent,
      target: Destination | Agent,
      earliest: number,
      length: number,
      label: string,
      outcome: Outcome = 'completed',
      wait = false,
      retryOf?: string,
    ) {
      const peer = 'charter' in target;
      const start = Math.max(
        earliest,
        available.get(agent.id) || 0,
        peer ? available.get(target.id) || 0 : 0,
      );
      const end = start + length;
      const id = `${config.id}-${serial++}`;
      available.set(agent.id, end + 0.7);
      if (peer) available.set(target.id, end + 0.7);
      events.push({
        id: `${id}-start`,
        at: start,
        type: 'start',
        interaction: {
          id,
          agentId: agent.id,
          targetId: target.id,
          targetName: peer ? teammate(target).name : target.name,
          tool: peer ? 'Message' : target.kind === 'tool' ? 'Terminal' : 'MCP',
          label,
          retryOf,
          workflowId: `${config.id}:${label.split(' / ')[0]}`,
        },
      });
      if (wait) {
        events.push({
          id: `${id}-wait`,
          at: start + length * 0.3,
          type: 'wait',
          agentId: agent.id,
          interactionId: id,
        });
        events.push({
          id: `${id}-resume`,
          at: start + length * 0.7,
          type: 'resume',
          agentId: agent.id,
          interactionId: id,
        });
      }
      events.push({
        id: `${id}-end`,
        at: end,
        type: 'end',
        agentId: agent.id,
        interactionId: id,
        outcome,
      });
      return end;
    }
    for (let item = 0; item < config.count; item++) {
      const ticket = `Workstream ${item + 1}`;
      let ready = 0.2 + item * config.spacing + (item % 4) * 1.3;
      let owner = pick(item, item);
      for (let stage = 0; stage < config.stages.length; stage++) {
        const label = `${ticket} / ${config.stages[stage]}`;
        const lead = pick(item + stage, item + stage * 2);
        if (lead.id !== owner.id)
          ready = schedule(
            owner,
            lead,
            ready + 0.8,
            2.5 + (item % 3),
            `${label}: cross-team handoff`,
          );
        owner = lead;
        // Different branch counts and durations create changing overlap. The next
        // stage starts only after all branches and any rework have finished.
        const workers = [
          ...new Map(
            Array.from({ length: 2 + ((item + stage) % 4) }, (_, branch) => {
              const agent = pick(item + stage + branch, item * 3 + branch);
              return [agent.id, agent] as const;
            }),
          ).values(),
        ];
        const completions = workers.map((agent, branch) => {
          const hotspot = (Math.floor(item / 5) + stage) % places.length;
          const target =
            places[(item % 3 === 0 ? hotspot : item * 2 + stage * 3 + branch) % places.length];
          const failed = (item + stage + branch) % 11 === 0;
          const firstAttemptId = `${config.id}-${serial}`;
          let end = schedule(
            agent,
            target,
            ready + branch * 0.65,
            5 + ((item * 7 + stage * 3 + branch * 5) % 14),
            `${label}: branch ${branch + 1}`,
            failed ? 'failed' : 'completed',
            (item + branch + stage) % 5 === 0,
          );
          if (failed)
            end = schedule(
              agent,
              places[(hotspot + 1) % places.length],
              end + 1.6,
              6 + branch,
              `${label}: rework`,
              'completed',
              false,
              firstAttemptId,
            );
          return end;
        });
        ready = Math.max(...completions);
        if (stage === 2)
          ready = schedule(
            lead,
            places[(item + 2) % places.length],
            ready + 0.5,
            9 + (item % 6),
            `${label}: approval gate`,
            'completed',
            true,
          );
      }
    }
    events.sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
    const duration = Math.ceil((events.at(-1)?.at || 0) + 6);
    return {
      id: config.id,
      name: `${config.name} · ${Math.ceil(duration / 60)} min`,
      duration,
      events,
      provenance: `Synthetic organization workflow · ${config.count} workstreams · ${groups.length} teams · ${events.length} events · dependencies, shared resources, handoffs and rework · not live`,
    };
  });
}
