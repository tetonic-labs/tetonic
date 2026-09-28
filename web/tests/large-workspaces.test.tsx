import { describe, expect, it } from 'vitest';
import { largeWorkspace, workspaceSizes } from '../src/store/largeWorkspaces';
import { GraphMotion } from '../src/lib/graphMotion';
import { MapLayout } from '../src/lib/mapLayout';
import { destinationsFor } from '../src/lib/mapActivity';
describe('large workspace examples', () => {
  it.each(Object.keys(workspaceSizes) as (keyof typeof workspaceSizes)[])(
    '%s has complete references and shared identities',
    (key) => {
      const data = largeWorkspace(key),
        size = workspaceSizes[key];
      expect(data.agents).toHaveLength(size.agents);
      expect(data.teams).toHaveLength(size.teams);
      expect(data.nodes).toHaveLength(size.resources);
      const places = destinationsFor(data.agents, data.nodes, data.edges);
      expect(places).toHaveLength(size.resources);
      const layout = new MapLayout().build(data.agents, data.teams, places);
      expect(layout.points.size).toBe(size.agents + size.resources);
      expect([...layout.memberships.values()].some((ids) => ids.length > 1)).toBe(true);
      const world = new GraphMotion([
        ...data.agents.map((a) => ({
          id: a.id,
          kind: 'agent' as const,
          home: layout.points.get(a.id)!,
        })),
        ...layout.places.map((p) => ({ id: p.id, kind: 'destination' as const, home: p.point })),
      ]);
      data.agents.forEach((a) => {
        const step = data.tracks[a.id].steps[0];
        expect(data.nodes.some((n) => n.id === step.targetNodeId)).toBe(true);
        world.dispatch({
          id: a.id,
          type: 'start',
          at: 0,
          interaction: {
            id: a.id,
            agentId: a.id,
            targetId: step.targetNodeId!,
            targetName: step.targetNodeName!,
            label: 'Concurrent work',
          },
        });
      });
      world.advance(0.01, true);
      world.snapshot().forEach((f) => {
        expect(Number.isFinite(f.position.x) && Number.isFinite(f.position.y)).toBe(true);
        expect(f.phase === 'engaged' || f.phase === 'resting').toBe(true);
      });
    },
  );
});
