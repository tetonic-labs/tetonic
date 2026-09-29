import { expect, it } from 'vitest';
import { rushTrace } from '../src/lib/rushTrace';
import { largeWorkspace } from '../src/store/largeWorkspaces';
import { destinationsFor } from '../src/lib/mapActivity';

it('runs all 80 agents with rapid work and paired conversations, then releases every attachment', () => {
  const data = largeWorkspace('network');
  const [trace] = rushTrace(data.agents, destinationsFor(data.agents, data.nodes, data.edges));
  const active = new Map<string, string>();
  let peak = 0,
    messages = 0;
  for (const event of trace.events) {
    expect(event.at).toBeLessThan(trace.duration - 2);
    if (event.type === 'start') {
      expect(active.has(event.interaction.agentId)).toBe(false);
      active.set(event.interaction.agentId, event.interaction.id);
      if (event.interaction.tool === 'Message') messages++;
    } else if (event.type === 'end') {
      expect(active.get(event.agentId)).toBe(event.interactionId);
      active.delete(event.agentId);
    }
    peak = Math.max(peak, active.size);
  }
  expect(peak).toBe(80);
  expect(messages).toBe(2200);
  expect(trace.events).toHaveLength(13200);
  expect(active.size).toBe(0);
});
