import { describe, it, expect } from 'vitest';
import {
  LocalMapMotion,
  localEntities,
  mapSignal,
  LOCAL_AGENT_LIMIT,
} from '../src/lib/localMapMotion';
import type { Entity, Interaction } from '../src/lib/graphMotion';
import type { WorkState } from '../src/lib/workScene';
import { workExceptions } from '../src/lib/workExceptions';

const entities: Entity[] = [
  { id: 'a', kind: 'agent', home: { x: 0, y: 800 } },
  { id: 'b', kind: 'agent', home: { x: 600, y: 800 } },
  { id: 'host', kind: 'destination', home: { x: 300, y: 100 } },
  { id: 'next', kind: 'destination', home: { x: 550, y: 100 } },
];
const action = (id = 'work', agentId = 'a', targetId = 'host'): Interaction => ({
  id,
  agentId,
  targetId,
  targetName: targetId,
  label: 'Read the evidence',
});
const state = (interaction: Interaction, waiting = false): WorkState => ({
  id: interaction.agentId,
  interaction,
  waiting,
  started: 0,
  failures: [],
});
const run = (motion: LocalMapMotion, seconds: number) => {
  for (let i = 0; i < seconds * 60; i++) motion.advance(1 / 60, false);
};

describe('bounded physical map lens', () => {
  it('keeps remote cross-organization work at its home landmark instead of chasing rapid distant destinations', () => {
    const motion = new LocalMapMotion();
    const far = [
      ...entities,
      { id: 'remote', kind: 'destination' as const, home: { x: 5000, y: 4000 } },
    ];
    const states = new Map([['a', state(action('remote-work', 'a', 'remote'))]]);
    motion.sync(far, states, 1, 'trace');
    run(motion, 3);
    expect(motion.world.snapshot().find((f) => f.id === 'a')!.position).toEqual(entities[0].home);
    expect(states.get('a')!.interaction!.targetId).toBe('remote');
  });
  it('preserves bodies, velocity and attachment through camera-independent resyncs', () => {
    const motion = new LocalMapMotion(),
      states = new Map([['a', state(action())]]);
    motion.sync(entities, states, 1, 'trace');
    run(motion, 0.2);
    const body = motion.world.bodies.get('a')!,
      position = { ...body.position },
      velocity = { ...body.velocity };
    motion.sync(
      entities.map((e) => ({ ...e, home: { ...e.home } })),
      states,
      1,
      'trace',
    );
    expect(motion.world.bodies.get('a')).toBe(body);
    expect(body.position).toEqual(position);
    expect(body.velocity).toEqual(velocity);
    run(motion, 2);
    expect(body.phase).toBe('engaged');
  });
  it('resolves simultaneous attachments, waiting, cancellation and return without idle wandering', () => {
    const motion = new LocalMapMotion();
    const states = new Map([
      ['a', state(action())],
      ['b', state(action('two', 'b'))],
    ]);
    motion.sync(entities, states, 1, 'trace');
    run(motion, 3);
    const a = motion.world.bodies.get('a')!,
      b = motion.world.bodies.get('b')!;
    expect(Math.hypot(a.position.x - b.position.x, a.position.y - b.position.y)).toBeGreaterThan(
      200,
    );
    states.set('a', state(action(), true));
    motion.sync(entities, states, 4, 'trace');
    run(motion, 1);
    expect(a.phase).toBe('waiting');
    states.clear();
    motion.sync(entities, states, 5, 'trace');
    run(motion, 6);
    for (const e of entities) {
      const body = motion.world.snapshot().find((f) => f.id === e.id)!;
      expect(Math.hypot(body.position.x - e.home.x, body.position.y - e.home.y)).toBeLessThan(15);
    }
    run(motion, 15);
    const before = motion.world.snapshot();
    run(motion, 3);
    before.forEach((f) => {
      const now = motion.world.snapshot().find((n) => n.id === f.id)!;
      expect(Math.hypot(now.position.x - f.position.x, now.position.y - f.position.y)).toBeLessThan(
        0.1,
      );
    });
  });
  it('reattaches if a host disappears and comes back, and resets only for a seek or new trace', () => {
    const motion = new LocalMapMotion(),
      states = new Map([['a', state(action())]]);
    motion.sync(entities, states, 1, 'trace');
    run(motion, 2);
    const original = motion.world;
    motion.sync(
      entities.filter((e) => e.id !== 'host'),
      states,
      2,
      'trace',
    );
    run(motion, 1);
    motion.sync(entities, states, 3, 'trace');
    run(motion, 3);
    expect(motion.world).toBe(original);
    expect(motion.world.bodies.get('a')!.phase).toBe('engaged');
    motion.sync(entities, new Map(), 0, 'trace');
    expect(motion.world).not.toBe(original);
    expect(motion.world.bodies.get('a')!.position).toEqual(entities[0].home);
  });
  it('limits physical actors and neighbors while retaining distant interaction targets and long labels', () => {
    const points = new Map(
      Array.from(
        { length: 200 },
        (_, i) => ['a' + i, { x: (i % 10) * 230, y: Math.floor(i / 10) * 230 }] as const,
      ),
    );
    const actors = [...points.keys()],
      states = new Map([['a0', state(action('work', 'a0', 'a199'))]]);
    const result = localEntities(
      points,
      actors,
      states,
      new Set(actors),
      new Map([['a0', 'A very long agent name '.repeat(8)]]),
    );
    expect(result.filter((e) => e.kind === 'agent')).toHaveLength(LOCAL_AGENT_LIMIT);
    expect(result.length).toBeLessThanOrEqual(LOCAL_AGENT_LIMIT + 1 + 24);
    expect(result.some((e) => e.id === 'a199')).toBe(true);
    expect(result.find((e) => e.id === 'a0')!.bounds!.right).toBe(112);
  });
  it('provides distinct truthful signals and an immediate reduced-motion layout', () => {
    const waiting = state(action(), true),
      blocked = { ...waiting, failure: { interaction: action(), at: 0 } };
    expect(mapSignal(waiting, false)).toBe('waiting');
    expect(workExceptions(new Map([['a', waiting]]), [])).toEqual([]);
    expect(mapSignal(blocked, false)).toBe('blocked');
    expect(mapSignal(blocked, true)).toBe('input');
    expect(mapSignal(undefined, false)).toBe('idle');
    const motion = new LocalMapMotion();
    motion.sync(entities, new Map([['a', waiting]]), 1, 'trace');
    const frames = motion.advance(0, true);
    expect(frames.find((f) => f.id === 'a')!.phase).toBe('waiting');
    expect(motion.advance(1, true)).toEqual(frames);
  });
});
