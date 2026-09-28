import { describe, it, expect } from 'vitest';
import { GraphMotion, Entity } from '../src/lib/graphMotion';
import { agentFootprint, toolFootprint } from '../src/lib/clusterClearance';

const entities: Entity[] = [
  { id: 'tool', kind: 'destination', home: { x: 500, y: 500 } },
  { id: 'neighbor', kind: 'destination', home: { x: 800, y: 500 } },
  { id: 'far', kind: 'destination', home: { x: 2200, y: 200 } },
  ...Array.from({ length: 8 }, (_, i) => ({
    id: `a${i}`,
    kind: 'agent' as const,
    home: { x: 200 + i * 240, y: 100 },
  })),
];
function attach(world: GraphMotion, index: number, target = 'tool') {
  world.dispatch({
    id: `start${index}`,
    at: 0,
    type: 'start',
    interaction: {
      id: `work${index}`,
      agentId: `a${index}`,
      targetId: target,
      targetName: target,
      label: 'Work',
    },
  });
}
function run(world: GraphMotion, seconds: number) {
  for (let i = 0; i < seconds * 120; i++) world.advance(1 / 120);
}
function overlap(world: GraphMotion, a: string, b: string) {
  const frames = world.snapshot();
  const pa = frames.find((f) => f.id === a)!.position,
    pb = frames.find((f) => f.id === b)!.position;
  const ba = a.startsWith('a') ? agentFootprint : toolFootprint,
    bb = b.startsWith('a') ? agentFootprint : toolFootprint;
  return Math.min(
    pa.x + ba.right - pb.x + bb.left,
    pb.x + bb.right - pa.x + ba.left,
    pa.y + ba.bottom - pb.y + bb.top,
    pb.y + bb.bottom - pa.y + ba.top,
  );
}
describe('local attachment clearance', () => {
  it('moves both the cluster and its neighbor, keeping the attachment intact', () => {
    const world = new GraphMotion(entities);
    attach(world, 0);
    run(world, 8);
    expect(overlap(world, 'a0', 'neighbor')).toBeLessThan(1);
    const frames = world.snapshot();
    const a = frames.find((f) => f.id === 'a0')!,
      host = frames.find((f) => f.id === 'tool')!;
    expect(a.host).toEqual(host.position);
    expect(
      Math.abs(
        Math.hypot(a.position.x - host.position.x, a.position.y - host.position.y) - a.radius!,
      ),
    ).toBeLessThan(2);
    expect(Math.hypot(host.position.x - 500, host.position.y - 500)).toBeGreaterThan(10);
    expect(
      Math.hypot(
        frames.find((f) => f.id === 'neighbor')!.position.x - 800,
        frames.find((f) => f.id === 'neighbor')!.position.y - 500,
      ),
    ).toBeGreaterThan(10);
    expect(
      Math.hypot(
        frames.find((f) => f.id === 'far')!.position.x - 2200,
        frames.find((f) => f.id === 'far')!.position.y - 200,
      ),
    ).toBeLessThan(6);
  });
  it('returns displaced tools home after cancellation', () => {
    const world = new GraphMotion(entities);
    attach(world, 0);
    run(world, 6);
    world.dispatch({
      id: 'cancel',
      at: 6,
      type: 'end',
      agentId: 'a0',
      interactionId: 'work0',
      outcome: 'cancelled',
    });
    run(world, 25);
    for (const id of ['tool', 'neighbor']) {
      const p = world.snapshot().find((f) => f.id === id)!.position,
        home = entities.find((e) => e.id === id)!.home;
      expect(Math.hypot(p.x - home.x, p.y - home.y)).toBeLessThan(12);
    }
  });
  it('settles reduced motion deterministically with multiple neighboring attachments', () => {
    const world = new GraphMotion(entities);
    attach(world, 0);
    attach(world, 1, 'neighbor');
    world.advance(0.01, true);
    const first = world.snapshot();
    world.advance(0.01, true);
    expect(world.snapshot()).toEqual(first);
    expect(overlap(world, 'a0', 'neighbor')).toBeLessThanOrEqual(0);
    expect(overlap(world, 'a0', 'a1')).toBeLessThanOrEqual(0);
  });
  it('keeps shared-host seats ordered and visible across outer rings', () => {
    const world = new GraphMotion(entities);
    for (let i = 0; i < 8; i++) attach(world, i);
    world.advance(0.01, true);
    for (let i = 0; i < 8; i++)
      for (let j = i + 1; j < 8; j++)
        expect(overlap(world, `a${i}`, `a${j}`)).toBeLessThanOrEqual(0);
    expect(
      new Set([...world.bodies.values()].filter((b) => b.attachment).map((b) => b.attachment!.slot))
        .size,
    ).toBe(8);
  });
});
