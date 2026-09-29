import { Agent, Team, GraphEdge } from '../types';
import { Destination, Point } from './mapActivity';
interface Neighborhood {
  x: number;
  y: number;
  width: number;
  height: number;
  columns: number;
}
const STEP = 230,
  GAP_X = 280,
  GAP_Y = 260;
const dimensions = (count: number, maxColumns = 4) => {
  const columns = Math.min(maxColumns, Math.max(2, Math.ceil(Math.sqrt(count))));
  return { columns, width: columns * STEP + 100, height: 150 + Math.ceil(count / columns) * STEP };
};
export function homeTeamId(agent: Agent, teams: Team[]) {
  const ids = teams
    .filter((t) => t.pledgedAgentIds.includes(agent.id) || t.id === agent.pledgedTeamId)
    .map((t) => t.id)
    .sort();
  return ids.includes(agent.pledgedTeamId || '') ? agent.pledgedTeamId! : ids[0] || 'unassigned';
}
export class MapLayout {
  private columns: { x: number; width: number }[] = [];
  private groups = new Map<string, Neighborhood>();
  private slots = new Map<string, Map<string, number>>();
  private resourceSlots = new Map<string, Map<string, number>>();
  private resourceColumns = new Map<string, { x: number; columns: number }>();
  private resourceTop = 0;
  // Choose geography once. Resize and incoming events must not shuffle the organization.
  constructor(private readonly aspectRatio = 1.8) {}
  private arrange(buckets: [string, Agent[]][]) {
    if (!buckets.length || this.columns.length) return;
    const sizes = buckets.map(([, members]) => dimensions(members.length));
    let best = { count: 1, widths: [0], heights: [0], score: Infinity };
    for (let count = 1; count <= Math.min(6, buckets.length); count++) {
      const widths = Array(count).fill(0) as number[];
      const heights = Array(Math.ceil(buckets.length / count)).fill(0) as number[];
      sizes.forEach((s, i) => {
        widths[i % count] = Math.max(widths[i % count], s.width);
        heights[Math.floor(i / count)] = Math.max(heights[Math.floor(i / count)], s.height);
      });
      const width = widths.reduce((a, b) => a + b, 0) + GAP_X * (count - 1);
      const height = heights.reduce((a, b) => a + b, 0) + GAP_Y * (heights.length - 1);
      const score = Math.max(width / Math.max(0.6, Math.min(3, this.aspectRatio)), height);
      if (score < best.score) best = { count, widths, heights, score };
    }
    let x = 120;
    this.columns = best.widths.map((width) => {
      const column = { x, width };
      x += width + GAP_X;
      return column;
    });
    buckets.forEach(([id], i) => {
      const row = Math.floor(i / best.count);
      const y = 180 + best.heights.slice(0, row).reduce((a, b) => a + b + GAP_Y, 0);
      this.groups.set(id, { ...sizes[i], x: this.columns[i % best.count].x, y });
    });
  }
  build(agents: Agent[], teams: Team[], destinations: Destination[], _edges: GraphEdge[] = []) {
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
    const rank = (id: string) => {
      const i = teams.findIndex((t) => t.id === id);
      return i < 0 ? teams.length : i;
    };
    const ordered = [...buckets].sort(
      (a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0]),
    );
    this.arrange(ordered);
    for (const [id, members] of ordered) {
      if (!this.slots.has(id)) this.slots.set(id, new Map());
      const slots = this.slots.get(id)!;
      members.forEach((a) => {
        if (!slots.has(a.id)) slots.set(a.id, slots.size);
      });
      if (!this.groups.has(id)) {
        const choices = this.columns.map((column) => ({
          ...column,
          y: Math.max(
            180,
            ...[...this.groups.values()]
              .filter((g) => g.x === column.x)
              .map((g) => g.y + g.height + GAP_Y),
          ),
        }));
        const column = choices.sort((a, b) => a.y - b.y || a.x - b.x)[0];
        this.groups.set(id, {
          ...dimensions(members.length, Math.floor((column.width - 100) / STEP)),
          x: column.x,
          y: column.y,
        });
      }
      const box = this.groups.get(id)!;
      box.height = Math.max(box.height, 150 + Math.ceil(slots.size / box.columns) * STEP);
    }
    // Growth yields only the teams below it in the same column. Never pull homes back
    // on removal: an agent returning to a team should find the same seat.
    for (const column of this.columns) {
      let bottom = 180;
      for (const box of [...this.groups.values()]
        .filter((g) => g.x === column.x)
        .sort((a, b) => a.y - b.y)) {
        box.y = Math.max(box.y, bottom);
        bottom = box.y + box.height + GAP_Y;
      }
    }
    const groups = ordered.map(([id, members]) => {
      const box = this.groups.get(id)!;
      members.forEach((a) => {
        const slot = this.slots.get(id)!.get(a.id)!;
        points.set(a.id, {
          x: box.x + 165 + (slot % box.columns) * STEP,
          y: box.y + 190 + Math.floor(slot / box.columns) * STEP,
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
    const right = Math.max(900, ...groups.map((g) => g.x + g.width));
    const bottom = Math.max(750, ...groups.map((g) => g.y + g.height));
    // Resources share a legible address below the teams. Stable slots keep a busy
    // tool in the same place; local attachment physics can still yield around it.
    this.resourceTop = Math.max(this.resourceTop, bottom + 300);
    const families = [
      {
        id: 'mcp',
        name: 'MCP connections',
        members: destinations.filter((d) => d.kind === 'connector'),
      },
      {
        id: 'tools',
        name: 'Tools & storage',
        members: destinations.filter((d) => d.kind !== 'connector'),
      },
    ].filter((f) => f.members.length);
    const resourceAreas = families.map((family) => {
      if (!this.resourceColumns.has(family.id)) {
        const width = Math.max(
          620,
          (right - 120 - GAP_X * (families.length - 1)) / families.length,
        );
        const columns = Math.min(12, Math.max(2, Math.floor((width - 120) / 250)));
        const x = Math.max(
          120,
          ...[...this.resourceColumns.values()].map((c) => c.x + c.columns * 250 + 120 + GAP_X),
        );
        this.resourceColumns.set(family.id, { x, columns });
        this.resourceSlots.set(family.id, new Map());
      }
      const area = this.resourceColumns.get(family.id)!;
      const slots = this.resourceSlots.get(family.id)!;
      [...family.members]
        .sort((a, b) => a.id.localeCompare(b.id))
        .forEach((d) => {
          if (!slots.has(d.id)) slots.set(d.id, slots.size);
          const slot = slots.get(d.id)!;
          points.set(d.id, {
            x: area.x + 125 + (slot % area.columns) * 250,
            y: this.resourceTop + 160 + Math.floor(slot / area.columns) * STEP,
          });
        });
      return {
        id: family.id,
        name: family.name,
        count: family.members.length,
        x: area.x,
        y: this.resourceTop,
        width: area.columns * 250 + 120,
        height: Math.ceil(slots.size / area.columns) * STEP + 100,
      };
    });
    const places = destinations.map((d) => ({ ...d, point: points.get(d.id)! }));
    return {
      points,
      groups,
      memberships,
      places,
      resourceAreas,
      world: {
        width: Math.max(
          900,
          ...groups.map((g) => g.x + g.width + 150),
          ...places.map((p) => p.point.x + 200),
          ...resourceAreas.map((a) => a.x + a.width + 100),
        ),
        height: Math.max(
          750,
          ...groups.map((g) => g.y + g.height + 180),
          ...places.map((p) => p.point.y + 200),
          ...resourceAreas.map((a) => a.y + a.height + 100),
        ),
      },
    };
  }
}
