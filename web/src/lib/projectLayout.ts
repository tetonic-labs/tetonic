import { dependenciesOf, type ProjectView } from './projectView';

export type Point = { x: number; y: number };
export type Rect = Point & { width: number; height: number };
export type ProjectEdge = { from: string; to: string; reason: string; points: Point[] };
const right = (r: Rect) => r.x + r.width;
const bottom = (r: Rect) => r.y + r.height;

/** Stable topological placement in a folded two-column reading order. Long
 * edges use reserved outer channels; destinations never occupy graph gutters.
 * This bounds node/edge collisions, not crossings between arbitrarily dense edges.
 */
export function layoutProject(project: ProjectView) {
  const remaining = new Set(project.streams.map((s) => s.id));
  const order: string[] = [];
  const warnings: string[] = [];
  const known = new Set(remaining);
  const edges = project.streams.flatMap((s) =>
    dependenciesOf(s).flatMap((d) => {
      if (!known.has(d.id)) {
        warnings.push(`Missing dependency for ${s.name}: ${d.id}`);
        return [];
      }
      return [{ from: d.id, to: s.id, reason: d.reason }];
    }),
  );
  while (remaining.size) {
    const ready = [...remaining].filter(
      (id) => !edges.some((e) => e.to === id && remaining.has(e.from)),
    );
    if (!ready.length) {
      warnings.push('Circular dependencies need review. Their links are hidden.');
      order.push(...remaining);
      break;
    }
    ready.forEach((id) => {
      remaining.delete(id);
      order.push(id);
    });
  }
  const validEdges = edges.filter((e) => !remaining.has(e.from) && !remaining.has(e.to));
  const gutter = 40 + validEdges.length * 14;
  const width = 360;
  const height = Math.max(
    290,
    ...project.streams.map((s) => 185 + Math.ceil(s.agents.length / 2) * 105),
  );
  const streams: Record<string, Rect> = {};
  order.forEach((id, index) => {
    const row = Math.floor(index / 2);
    const column = row % 2 ? 1 - (index % 2) : index % 2;
    streams[id] = {
      x: gutter + column * (width + 100),
      y: 40 + row * (height + 100),
      width,
      height,
    };
  });
  const routes: ProjectEdge[] = validEdges.map((edge, index) => {
    const a = streams[edge.from],
      b = streams[edge.to];
    let points: Point[];
    if (a.y === b.y) {
      const forward = a.x < b.x;
      points = [
        { x: forward ? right(a) : a.x, y: a.y + 55 },
        { x: forward ? b.x : right(b), y: b.y + 55 },
      ];
    } else if (a.x === b.x && b.y - a.y === height + 100) {
      points = [
        { x: a.x + width / 2, y: bottom(a) },
        { x: b.x + width / 2, y: b.y },
      ];
    } else {
      const lane = 20 + index * 14;
      points = [
        { x: a.x + width / 2, y: bottom(a) },
        { x: a.x + width / 2, y: bottom(a) + 24 + index * 2 },
        { x: lane, y: bottom(a) + 24 + index * 2 },
        { x: lane, y: b.y - 24 - index * 2 },
        { x: b.x + width / 2, y: b.y - 24 - index * 2 },
        { x: b.x + width / 2, y: b.y },
      ];
    }
    return { ...edge, points };
  });
  const places: Record<string, Rect> = {};
  let placeY = 40;
  const placeX = gutter + width * 2 + 190;
  project.places.forEach((place) => {
    const users = project.people.filter((p) => p.destination === place.id).length;
    const h = Math.max(200, 90 + Math.ceil(users / 2) * 105);
    places[place.id] = { x: placeX, y: placeY, width: 290, height: h };
    placeY += h + 25;
  });
  const unplaced = project.people.filter(
    (person) => !project.streams.some((s) => s.agents.includes(person.agent.id)),
  );
  const people = project.people.map((person) => {
    const stream = project.streams.find((s) => s.agents.includes(person.agent.id));
    const homeRect = stream ? streams[stream.id] : undefined;
    const slot = stream?.agents.indexOf(person.agent.id) ?? 0;
    const home = homeRect
      ? { x: homeRect.x + 90 + (slot % 2) * 175, y: homeRect.y + 195 + Math.floor(slot / 2) * 105 }
      : {
          x: gutter + 90 + (unplaced.indexOf(person) % 4) * 180,
          y:
            80 +
            Math.ceil(order.length / 2) * (height + 100) +
            Math.floor(unplaced.indexOf(person) / 4) * 130,
        };
    const destination = person.destination ? places[person.destination] : undefined;
    const seat = project.people
      .filter((p) => p.destination === person.destination)
      .findIndex((p) => p.agent.id === person.agent.id);
    const point = destination
      ? {
          x: destination.x + 72 + (seat % 2) * 145,
          y: destination.y + 112 + Math.floor(seat / 2) * 105,
        }
      : home;
    return { person, home, point, destination };
  });
  return {
    streams,
    places,
    people,
    edges: routes,
    warnings,
    width: project.places.length ? placeX + 320 : gutter + 2 * width + 130,
    height:
      Math.max(
        440,
        ...Object.values(streams).map(bottom),
        placeY,
        ...people.map((p) => p.point.y + 85),
      ) + 30,
  };
}

export function layoutPortfolio(projects: ProjectView[]) {
  const groups = [...new Set(projects.map((p) => p.area?.id || 'other'))].map((id) => {
    const items = projects.filter((p) => (p.area?.id || 'other') === id);
    return { id, area: items[0].area, items, height: 110 + items.length * 240 };
  });
  const rows: number[] = [];
  const positioned = groups.map((group, index) => {
    const row = Math.floor(index / 2);
    if (index % 2 === 0) rows[row] = Math.max(group.height, groups[index + 1]?.height || 0) + 55;
    const y = 25 + rows.slice(0, row).reduce((sum, h) => sum + h, 0);
    return { ...group, x: 35 + (index % 2) * 575, y, width: 520 };
  });
  return {
    groups: positioned,
    width: groups.length > 1 ? 1170 : 590,
    height: Math.max(500, ...positioned.map((g) => g.y + g.height)) + 30,
  };
}

export const edgePath = (points: Point[]) =>
  points.map((p, i) => `${i ? 'L' : 'M'}${p.x} ${p.y}`).join(' ');
