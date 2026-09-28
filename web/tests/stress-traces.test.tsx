import { describe, expect, it } from 'vitest';
import { stressTraces } from '../src/lib/stressTraces';
import { largeWorkspace } from '../src/store/largeWorkspaces';
import { destinationsFor } from '../src/lib/mapActivity';

describe('busy long traces', () => {
  const data = largeWorkspace('network');
  const places = destinationsFor(data.agents, data.nodes, data.edges);
  const examples = stressTraces(data.agents, places);
  it('is deterministic with thousands of events and 3, 6, 10 minute durations', () => {
    expect(examples).toEqual(stressTraces(data.agents, places));
    expect(examples.map((e) => e.duration)).toEqual([180, 360, 600]);
    examples.forEach((e) => expect(e.events.length).toBeGreaterThan(2000));
  });
  it.each(examples)('$name overlaps work but always terminates every interaction', (example) => {
    const active = new Map<string, string>();
    let peak = 0,
      previous = 0;
    const ids = new Set<string>();
    for (const event of example.events) {
      expect(event.at).toBeGreaterThanOrEqual(previous);
      previous = event.at;
      expect(event.at).toBeLessThan(example.duration - 2);
      expect(ids.has(event.id)).toBe(false);
      ids.add(event.id);
      if (event.type === 'start') {
        expect(active.has(event.interaction.agentId)).toBe(false);
        active.set(event.interaction.agentId, event.interaction.id);
      } else {
        expect(active.get(event.agentId)).toBe(event.interactionId);
        if (event.type === 'end') active.delete(event.agentId);
      }
      peak = Math.max(peak, active.size);
    }
    expect(peak).toBe(80);
    expect(active.size).toBe(0);
    expect(example.events.some((e) => e.type === 'wait')).toBe(true);
    expect(example.events.some((e) => e.type === 'end' && e.outcome === 'failed')).toBe(true);
    expect(example.events.some((e) => e.type === 'end' && e.outcome === 'cancelled')).toBe(true);
  });
});
