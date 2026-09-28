import type { Point } from './mapActivity';

export interface Footprint {
  left: number;
  right: number;
  top: number;
  bottom: number;
}
export interface CollisionNode {
  id: string;
  cluster: string;
  position: Point;
  bounds: Footprint;
}
export const agentFootprint: Footprint = { left: 112, right: 112, top: 70, bottom: 160 };
export const toolFootprint: Footprint = { left: 95, right: 95, top: 55, bottom: 85 };

// Resolve individual visible rectangles, not a large circle around a whole orbit.
// Every correction is shared equally by the two clusters.
export function clearanceTargets(
  nodes: CollisionNode[],
  offsets: Map<string, Point>,
  active: Set<string>,
  relax = 0.97,
) {
  const targets = new Map<string, Point>();
  for (const node of nodes)
    if (!targets.has(node.cluster)) {
      const old = offsets.get(node.cluster) || { x: 0, y: 0 };
      targets.set(node.cluster, { x: old.x * relax, y: old.y * relax });
    }
  for (let pass = 0; pass < 16; pass++) {
    let changed = false;
    for (let i = 0; i < nodes.length; i++)
      for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i],
          b = nodes[j];
        if (a.cluster === b.cluster || (!active.has(a.cluster) && !active.has(b.cluster))) continue;
        const oa = targets.get(a.cluster)!,
          ob = targets.get(b.cluster)!;
        const ax = a.position.x + oa.x,
          ay = a.position.y + oa.y;
        const bx = b.position.x + ob.x,
          by = b.position.y + ob.y;
        const left = ax + a.bounds.right - (bx - b.bounds.left) + 18;
        const right = bx + b.bounds.right - (ax - a.bounds.left) + 18;
        const up = ay + a.bounds.bottom - (by - b.bounds.top) + 18;
        const down = by + b.bounds.bottom - (ay - a.bounds.top) + 18;
        if (Math.min(left, right, up, down) <= 0) continue;
        const dx = left < right ? -left : right,
          dy = up < down ? -up : down;
        if (Math.abs(dx) < Math.abs(dy)) {
          oa.x += dx / 2;
          ob.x -= dx / 2;
        } else {
          oa.y += dy / 2;
          ob.y -= dy / 2;
        }
        active.add(a.cluster);
        active.add(b.cluster);
        changed = true;
      }
    if (!changed) break;
  }
  // An impossible density must not send nodes drifting indefinitely.
  for (const offset of targets.values()) {
    const distance = Math.hypot(offset.x, offset.y);
    if (distance > 650) {
      offset.x *= 650 / distance;
      offset.y *= 650 / distance;
    }
  }
  return targets;
}
