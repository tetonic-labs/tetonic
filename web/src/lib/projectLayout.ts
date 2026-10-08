import { dependenciesOf, type ProjectView } from './projectView';

export type Point = { x: number; y: number };
export type Rect = Point & { width: number; height: number };
export type ProjectEdge = { from: string; to: string; reason: string; points: Point[] };
const right = (r: Rect) => r.x + r.width;
const bottom = (r: Rect) => r.y + r.height;

/** Dependency layers preserve parallel work instead of suggesting a serial path.
 * Long edges use outer channels; destinations never occupy graph gutters.
 * This bounds node/edge collisions, not crossings between arbitrarily dense edges.
 */
export function layoutProject(project: ProjectView) {
  const remaining = new Set(project.streams.map((s) => s.id));
  const order: string[] = [];
  const waves: string[][] = [];
  const warnings: string[] = [];
  const known = new Set(remaining);
  const missing = new Set<string>();
  const edges = project.streams.flatMap((s) =>
    dependenciesOf(s).flatMap((d) => {
      if (!known.has(d.id)) {
        missing.add(s.id);
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
      waves.push([...remaining]);
      break;
    }
    waves.push(ready);
    ready.forEach((id) => {
      remaining.delete(id);
      order.push(id);
    });
  }
  const validEdges = edges.filter((e) => !remaining.has(e.from) && !remaining.has(e.to));
  const gutter = 40 + validEdges.length * 8;
  const width = 360;
  const height = Math.max(
    290,
    ...project.streams.map((s) => 185 + Math.ceil(s.agents.length / 2) * 105),
  );
  const streams: Record<string, Rect> = {};
  // Terminal coordination is visually separate, but its real incoming links remain.
  const coordinators = new Set(
    project.streams
      .filter(
        (s) =>
          s.role === 'coordination' &&
          !remaining.has(s.id) &&
          !validEdges.some((e) => e.from === s.id),
      )
      .map((s) => s.id),
  );
  const layers = waves
    .map((ids, rank) => ({
      ids: ids.filter((id) => !coordinators.has(id)),
      rank,
      coordination: false,
    }))
    .filter((layer) => layer.ids.length);
  if (coordinators.size)
    layers.push({ ids: [...coordinators], rank: layers.length, coordination: true });
  let nextX = gutter;
  const nodeTop = 84 + validEdges.length * 8;
  const bands = layers.map((layer, index) => {
    const x = nextX;
    const columns = Math.min(2, layer.ids.length);
    layer.ids.forEach((id, slot) => {
      streams[id] = {
        x: x + (slot % columns) * (width + 60),
        y: nodeTop + Math.floor(slot / columns) * (height + 40),
        width,
        height,
      };
    });
    const bandHeight = Math.ceil(layer.ids.length / columns) * (height + 40) - 40;
    const bandWidth = columns * (width + 60) - 60;
    nextX = x + bandWidth + gutter + 60;
    const unresolved = layer.ids.some((id) => remaining.has(id) || missing.has(id));
    return {
      id: `layer-${index}`,
      x,
      y: nodeTop - 55,
      width: bandWidth,
      height: bandHeight + 70,
      ids: layer.ids,
      title: unresolved
        ? 'Dependencies need review'
        : layer.coordination
          ? 'Coordination & result'
          : layer.rank === 0
            ? validEdges.length
              ? 'Independent contributions'
              : 'Parallel work'
            : 'Builds on earlier work',
      description: unresolved
        ? 'Review the recorded links'
        : layer.coordination
          ? 'Dispatches work and brings results together'
          : `${layer.ids.length} ${layer.ids.length === 1 ? 'contribution' : 'contributions'}${layer.rank > 0 ? ' · follows dependency links' : ' · no ordering implied'}`,
    };
  });
  const routes: ProjectEdge[] = validEdges.map((edge, index) => {
    const a = streams[edge.from],
      b = streams[edge.to];
    let points: Point[];
    // Direct links only when the corridor is clear; otherwise use band gutters.
    const clear =
      a.y === b.y &&
      !Object.values(streams).some(
        (box) =>
          box !== a &&
          box !== b &&
          box.x < b.x &&
          right(box) > right(a) &&
          box.y < a.y + 55 &&
          bottom(box) > a.y + 55,
      );
    if (clear) {
      points = [
        { x: right(a), y: a.y + 55 },
        { x: b.x, y: b.y + 55 },
      ];
    } else {
      const fromBand = bands.find((band) => band.ids.includes(edge.from))!;
      const toBand = bands.find((band) => band.ids.includes(edge.to))!;
      const fromLane = fromBand.x - 20 - index * 8;
      const toLane = toBand.x - 20 - index * 8;
      const laneY = 20 + index * 8;
      const outY = bottom(a) + 20 + (index % 7) * 2;
      const inY = b.y - 20 - (index % 7) * 2;
      points = [
        { x: a.x + width / 2, y: bottom(a) },
        { x: a.x + width / 2, y: outY },
        { x: fromLane, y: outY },
        { x: fromLane, y: laneY },
        { x: toLane, y: laneY },
        { x: toLane, y: inY },
        { x: b.x + width / 2, y: inY },
        { x: b.x + width / 2, y: b.y },
      ];
    }
    return { ...edge, points };
  });
  const places: Record<string, Rect> = {};
  let placeY = 40;
  const graphRight = Math.max(360, ...Object.values(streams).map(right));
  const unplaced = project.people.filter(
    (person) => !project.streams.some((s) => s.agents.includes(person.agent.id)),
  );
  const participantX = graphRight + 100;
  const participantWidth = Math.min(2, unplaced.length) * 180;
  if (order.length && unplaced.length)
    bands.push({
      id: 'other-participants',
      x: participantX,
      y: nodeTop - 55,
      width: participantWidth,
      height: Math.ceil(unplaced.length / 2) * 130 + 100,
      ids: [],
      title: 'Also involved',
      description: 'No assignment shown in this view',
    });
  const placeX =
    order.length && unplaced.length ? participantX + participantWidth + 120 : graphRight + 120;
  project.places.forEach((place) => {
    const users = project.people.filter((p) => p.destination === place.id).length;
    const h = Math.max(200, 90 + Math.ceil(users / 2) * 105);
    places[place.id] = { x: placeX, y: placeY, width: 290, height: h };
    placeY += h + 25;
  });
  const people = project.people.map((person) => {
    const stream = project.streams.find((s) => s.agents.includes(person.agent.id));
    const homeRect = stream ? streams[stream.id] : undefined;
    const slot = stream?.agents.indexOf(person.agent.id) ?? 0;
    const home = homeRect
      ? { x: homeRect.x + 90 + (slot % 2) * 175, y: homeRect.y + 195 + Math.floor(slot / 2) * 105 }
      : {
          x: (order.length ? participantX : gutter) + 90 + (unplaced.indexOf(person) % 2) * 180,
          y: (order.length ? nodeTop + 145 : 80) + Math.floor(unplaced.indexOf(person) / 2) * 130,
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
    bands,
    places,
    people,
    edges: routes,
    warnings,
    width: Math.max(
      project.places.length ? placeX + 320 : graphRight + 40,
      ...people.map((p) => p.point.x + 85),
    ),
    height:
      Math.max(
        360,
        ...Object.values(streams).map(bottom),
        placeY,
        ...people.map((p) => p.point.y + 85),
      ) + 30,
  };
}

export function layoutPortfolio(projects: ProjectView[]) {
  const groups = [...new Set(projects.map((p) => p.area?.id || 'other'))].map((id) => {
    const items = projects.filter((p) => (p.area?.id || 'other') === id);
    const columns = Math.min(6, Math.ceil(items.length / Math.max(1, Math.ceil(items.length / 6))));
    return {
      id,
      area: items[0].area,
      items,
      columns,
      width: 25 + columns * 495,
      height: 110 + Math.ceil(items.length / columns) * 240,
    };
  });
  const rows: number[] = [];
  const positioned = groups.map((group, index) => {
    const row = Math.floor(index / 2);
    if (index % 2 === 0) rows[row] = Math.max(group.height, groups[index + 1]?.height || 0) + 55;
    const y = 25 + rows.slice(0, row).reduce((sum, h) => sum + h, 0);
    const x = index % 2 === 0 ? 35 : 35 + groups[index - 1].width + 55;
    return { ...group, x, y };
  });
  return {
    groups: positioned,
    width: Math.max(590, ...positioned.map((g) => g.x + g.width + 35)),
    height: Math.max(500, ...positioned.map((g) => g.y + g.height)) + 30,
  };
}

export const edgePath = (points: Point[]) =>
  points.map((p, i) => `${i ? 'L' : 'M'}${p.x} ${p.y}`).join(' ');
