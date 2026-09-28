import { Agent, Team, GraphEdge } from '../types';
import { Destination, Point } from './mapActivity';
interface Neighborhood {
  x: number;
  y: number;
  width: number;
  height: number;
  columns: number;
}
export function homeTeamId(agent: Agent, teams: Team[]) {
  const ids = teams
    .filter((t) => t.pledgedAgentIds.includes(agent.id) || t.id === agent.pledgedTeamId)
    .map((t) => t.id)
    .sort();
  return ids.includes(agent.pledgedTeamId || '') ? agent.pledgedTeamId! : ids[0] || 'unassigned';
}
export class MapLayout {
  private columnCount = 0;
  private groups = new Map<string, Neighborhood>();
  private slots = new Map<string, Map<string, number>>();
  private resources = new Map<string, Point>();
  build(agents: Agent[], teams: Team[], destinations: Destination[], edges: GraphEdge[] = []) {
    const points = new Map<string, Point>(),
      memberships = new Map<string, string[]>(),
      buckets = new Map<string, Agent[]>();
    for (const agent of [...new Map(agents.map((a) => [a.id, a])).values()].sort((a, b) =>
      a.id.localeCompare(b.id),
    )) {
      const ids = teams
        .filter((t) => t.pledgedAgentIds.includes(agent.id) || t.id === agent.pledgedTeamId)
        .map((t) => t.id)
        .sort();
      memberships.set(agent.id, ids);
      const home = homeTeamId(agent, teams);
      if (!buckets.has(home)) buckets.set(home, []);
      buckets.get(home)!.push(agent);
    }
    if (!this.columnCount) this.columnCount = buckets.size > 4 ? 3 : 2;
    const groups = [...buckets].map(([id, members]) => {
      if (!this.slots.has(id)) this.slots.set(id, new Map());
      const slots = this.slots.get(id)!;
      members.forEach((a) => {
        if (!slots.has(a.id)) slots.set(a.id, slots.size);
      });
      if (!this.groups.has(id)) {
        const columns = Math.min(4, Math.max(2, Math.ceil(Math.sqrt(members.length))));
        const width = columns * 230 + 100,
          height = 150 + Math.ceil(members.length / columns) * 230;
        // Two staggered shelves create neighborhoods with different sizes, not a long row.
        const column = this.groups.size % this.columnCount;
        const same = [...this.groups.values()].filter((g) => g.x === 120 + column * 1200);
        this.groups.set(id, {
          x: 120 + column * 1200,
          y: same.length ? Math.max(...same.map((g) => g.y + g.height)) + 300 : 140 + column * 110,
          width,
          height,
          columns,
        });
      }
      const box = this.groups.get(id)!;
      box.height = Math.max(box.height, 150 + Math.ceil(slots.size / box.columns) * 230);
      members.forEach((a) => {
        const slot = slots.get(a.id)!;
        points.set(a.id, {
          x: box.x + 165 + (slot % box.columns) * 230,
          y: box.y + 190 + Math.floor(slot / box.columns) * 230,
        });
      });
      return {
        id,
        name: teams.find((t) => t.id === id)?.name || 'Unassigned',
        ...box,
        count: members.length,
        agentIds: members.map((a) => a.id),
      };
    });
    const places = [...destinations]
      .sort((a, b) => a.id.localeCompare(b.id))
      .map((d) => {
        if (!this.resources.has(d.id)) {
          const users = edges
            .filter((e) => e.source === d.id || e.target === d.id)
            .map((e) => points.get(e.source === d.id ? e.target : e.source))
            .filter((p): p is Point => !!p);
          const desired = users.length
            ? {
                x: users.reduce((n, p) => n + p.x, 0) / users.length,
                y: users.reduce((n, p) => n + p.y, 0) / users.length,
              }
            : { x: 600, y: 700 };
          let best: Point | undefined,
            score = Infinity;
          for (let row = 0; row < 50; row++)
            for (let col = 0; col < this.columnCount * 5; col++) {
              const candidate = { x: 180 + col * 250, y: 180 + row * 230 };
              const inGroup = groups.some(
                (g) =>
                  candidate.x > g.x - 100 &&
                  candidate.x < g.x + g.width + 100 &&
                  candidate.y > g.y - 100 &&
                  candidate.y < g.y + g.height + 100,
              );
              const occupied = [...this.resources.values()].some(
                (p) => Math.abs(p.x - candidate.x) < 230 && Math.abs(p.y - candidate.y) < 210,
              );
              if (inGroup || occupied) continue;
              const cost = Math.hypot(candidate.x - desired.x, candidate.y - desired.y);
              if (cost < score) {
                best = candidate;
                score = cost;
              }
            }
          this.resources.set(d.id, best || { x: 2600, y: 180 + this.resources.size * 230 });
        }
        const point = this.resources.get(d.id)!;
        points.set(d.id, point);
        return { ...d, point };
      });
    return {
      points,
      groups,
      memberships,
      places,
      world: {
        width: Math.max(
          900,
          ...groups.map((g) => g.x + g.width + 150),
          ...places.map((p) => p.point.x + 200),
        ),
        height: Math.max(
          750,
          ...groups.map((g) => g.y + g.height + 180),
          ...places.map((p) => p.point.y + 200),
        ),
      },
    };
  }
}
