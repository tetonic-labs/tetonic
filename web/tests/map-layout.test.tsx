import { describe, expect, it } from 'vitest';
import { MapLayout } from '../src/lib/mapLayout';
import { mockAgents } from '../src/store/mockData';
import { destinationsFor } from '../src/lib/mapActivity';
import { mockGraphNodes, mockGraphEdges } from '../src/store/graphMockData';
import { largeWorkspace } from '../src/store/largeWorkspaces';

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
  it('organizes crowded teams into aligned rows that suit the initial viewport', () => {
    const data = largeWorkspace('network');
    const wide = new MapLayout(3).build(data.agents, data.teams, []);
    const narrow = new MapLayout(0.8).build(data.agents, data.teams, []);
    const columns = (layout: typeof wide) => new Set(layout.groups.map((g) => g.x)).size;
    expect(columns(wide)).toBeGreaterThan(columns(narrow));
    expect(wide.world.width / wide.world.height).toBeGreaterThan(
      narrow.world.width / narrow.world.height,
    );
    const firstRow = wide.groups.slice(0, columns(wide));
    expect(new Set(firstRow.map((g) => g.y)).size).toBe(1);
    expect(wide.groups.map((g) => g.id)).toEqual(data.teams.map((t) => t.id));
  });
  it('gives tools and MCPs distinct shared areas with stable seats when connections change', () => {
    const data = largeWorkspace('network');
    const allocator = new MapLayout(3);
    const destinations = destinationsFor(data.agents, data.nodes, data.edges);
    const before = allocator.build(data.agents, data.teams, destinations);
    expect(before.resourceAreas.map((a) => a.id)).toEqual(['mcp', 'tools']);
    before.places.forEach((p) => {
      const area = before.resourceAreas.find(
        (a) => a.id === (p.kind === 'connector' ? 'mcp' : 'tools'),
      )!;
      expect(p.point.x).toBeGreaterThan(area.x);
      expect(p.point.x).toBeLessThan(area.x + area.width);
      expect(p.point.y).toBeGreaterThan(Math.max(...before.groups.map((g) => g.y + g.height)));
    });
    const added = { ...destinations[0], id: 'new-connection' };
    const after = allocator.build(data.agents, data.teams, [
      ...destinations.slice(1).reverse(),
      added,
    ]);
    destinations
      .slice(1)
      .forEach((d) => expect(after.points.get(d.id)).toEqual(before.points.get(d.id)));
    expect(after.groups).toEqual(before.groups);
  });
  it('makes room for a growing team without moving other columns or duplicating shared agents', () => {
    const data = largeWorkspace('network');
    const allocator = new MapLayout(3);
    const destinations = destinationsFor(data.agents, data.nodes, data.edges);
    const before = allocator.build(data.agents, data.teams, destinations, data.edges);
    const first = before.groups[0];
    const added = Array.from({ length: 30 }, (_, i) => ({
      ...data.agents[0],
      id: `extra-${i}`,
      pledgedTeamId: first.id,
    }));
    const after = allocator.build([...data.agents, ...added], data.teams, destinations, data.edges);
    expect(after.points.size).toBe(data.agents.length + added.length + destinations.length);
    after.groups.forEach((g, i) => {
      after.groups.slice(i + 1).forEach((other) => {
        expect(
          g.x + g.width <= other.x ||
            other.x + other.width <= g.x ||
            g.y + g.height <= other.y ||
            other.y + other.height <= g.y,
        ).toBe(true);
      });
      if (g.x !== first.x) {
        expect(g).toEqual(before.groups.find((previous) => previous.id === g.id));
        g.agentIds.forEach((id) => expect(after.points.get(id)).toEqual(before.points.get(id)));
      }
    });
    after.places.forEach((p) =>
      after.groups.forEach((g) => {
        expect(
          p.point.x <= g.x - 100 ||
            p.point.x >= g.x + g.width + 100 ||
            p.point.y <= g.y - 100 ||
            p.point.y >= g.y + g.height + 100,
        ).toBe(true);
      }),
    );
    const unchanged = allocator.build(
      [...data.agents].reverse(),
      [...data.teams].reverse(),
      destinations,
      [],
    );
    data.agents.forEach((a) => expect(unchanged.points.get(a.id)).toEqual(after.points.get(a.id)));
  });
});
