import { describe, expect, it } from 'vitest';
import { MapLayout } from '../src/lib/mapLayout';
import { mockAgents } from '../src/store/mockData';
import { destinationsFor } from '../src/lib/mapActivity';
import { mockGraphNodes, mockGraphEdges } from '../src/store/graphMockData';

describe('map layout clearance', () => {
  it.each([1, 6, 12])('keeps resting resources compact for %i agents', (count) => {
    const agents = Array.from({ length: count }, (_, i) => ({ ...mockAgents[0], id: `a-${i}` }));
    const destinations = destinationsFor(mockAgents, mockGraphNodes, mockGraphEdges);
    const layout = new MapLayout().build(agents, [], destinations);
    const clearance = 95;
    layout.places.forEach((place, i) => {
      layout.places.slice(i + 1).forEach((other) => {
        expect(
          Math.hypot(place.point.x - other.point.x, place.point.y - other.point.y),
        ).toBeGreaterThan(clearance * 2);
      });
      layout.groups.forEach((group) => {
        expect(
          place.point.x < group.x ||
            place.point.x > group.x + group.width ||
            place.point.y < group.y ||
            place.point.y > group.y + group.height,
        ).toBe(true);
      });
      expect(place.point.x - clearance).toBeGreaterThanOrEqual(0);
      expect(place.point.x + clearance).toBeLessThan(layout.world.width);
      expect(place.point.y + clearance).toBeLessThan(layout.world.height);
    });
  });
  it('keeps agent positions stable as agents are added or removed', () => {
    const allocator = new MapLayout();
    const before = allocator.build(mockAgents, [], []);
    const after = allocator.build(
      [...mockAgents.slice(1).reverse(), { ...mockAgents[0], id: 'new' }],
      [],
      [],
    );
    mockAgents
      .slice(1)
      .forEach((agent) => expect(after.points.get(agent.id)).toEqual(before.points.get(agent.id)));
  });
});
