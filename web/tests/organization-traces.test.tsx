import { describe, expect, it } from 'vitest';
import { organizationTraces } from '../src/lib/organizationTraces';
import { largeWorkspace } from '../src/store/largeWorkspaces';
import { destinationsFor } from '../src/lib/mapActivity';

describe('organization workflows', () => {
  const data = largeWorkspace('network');
  const places = destinationsFor(data.agents, data.nodes, data.edges);
  const traces = organizationTraces(data.agents, places);
  it('replays deterministically', () => expect(organizationTraces(data.agents, places)).toEqual(traces));
  it.each(traces)('$name changes concurrency and completes every operation', trace => {
    const active = new Map<string,string>();
    const workstreams = new Map<string,number>();
    const concurrency = new Set<number>();
    let crossTeam = 0, overlappingStreams = false;
    const ids = new Set<string>();
    for (const event of trace.events) {
      expect(ids.has(event.id)).toBe(false); ids.add(event.id);
      expect(event.at).toBeLessThan(trace.duration - 5);
      if (event.type === 'start') {
        const action = event.interaction;
        expect(active.has(action.agentId)).toBe(false);
        active.set(action.agentId,action.id);
        const stream = action.label.split(' / ')[0];
        workstreams.set(action.id,Number(stream.replace('Workstream ','')));
        if (new Set(workstreams.values()).size > 1) overlappingStreams = true;
        const peer = data.agents.find(a=>a.id===action.targetId);
        if(peer && peer.pledgedTeamId !== data.agents.find(a=>a.id===action.agentId)!.pledgedTeamId) crossTeam++;
      } else {
        expect(active.get(event.agentId)).toBe(event.interactionId);
        if(event.type === 'end') { active.delete(event.agentId); workstreams.delete(event.interactionId); }
      }
      concurrency.add(active.size);
    }
    expect(active.size).toBe(0);
    expect(concurrency.size).toBeGreaterThan(5);
    expect(overlappingStreams).toBe(true);
    expect(crossTeam).toBeGreaterThan(20);
    expect(trace.events.some(e=>e.type==='start' && e.interaction.label.endsWith('rework'))).toBe(true);
    expect(trace.events.some(e=>e.type==='wait')).toBe(true);
  });
});
