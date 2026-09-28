import { describe, it, expect } from 'vitest';
import { GraphMotion, Entity, Interaction, MOTION } from '../src/lib/graphMotion';
import { MotionPlayback, entitiesFor, examplesFor } from '../src/lib/motionPlayback';
import { actionsFor, destinationsFor } from '../src/lib/mapActivity';
import { mockAgents } from '../src/store/mockData';
import { mockAgentTracks, mockGraphEdges, mockGraphNodes } from '../src/store/graphMockData';

const entities: Entity[] = [
  { id: 'a', kind: 'agent', home: { x: 750, y: 500 } },
  { id: 'b', kind: 'agent', home: { x: 800, y: 850 } },
  { id: 'c', kind: 'agent', home: { x: 450, y: 850 } },
  { id: 'github', kind: 'destination', home: { x: 320, y: 250 } },
  { id: 'terminal', kind: 'destination', home: { x: 1200, y: 600 } },
];
const interaction = (id = 'one', agentId = 'a', targetId = 'github'): Interaction => ({
  id,
  agentId,
  targetId,
  targetName: targetId,
  tool: 'MCP',
  label: 'Test interaction',
});
const start = (world: GraphMotion, action = interaction()) =>
  world.dispatch({ id: action.id + '-start', at: 0, type: 'start', interaction: action });
function run(world: GraphMotion, seconds: number, hz = 60) {
  for (let i = 0; i < Math.round(seconds * hz); i++) world.advance(1 / hz);
}
const distance = (a: { x: number; y: number }, b: { x: number; y: number }) =>
  Math.hypot(a.x - b.x, a.y - b.y);
describe('persistent graph motion', () => {
  it('retains a wait that arrives while the previous interaction is detaching', () => {
    const world = new GraphMotion(entities);
    start(world);
    run(world, 1.5);
    start(world, interaction('queued', 'a', 'terminal'));
    world.dispatch({
      id: 'queued-wait',
      at: 0,
      type: 'wait',
      agentId: 'a',
      interactionId: 'queued',
    });
    run(world, 2);
    expect(world.bodies.get('a')!.phase).toBe('waiting');
  });
  it('captures promptly, settles with its satellite, releases, then rests', () => {
    const world = new GraphMotion(entities);
    start(world);
    run(world, 1.5);
    const docked = world.snapshot().find((f) => f.id === 'a')!;
    expect(docked.phase).toBe('engaged');
    expect(Math.abs(distance(docked.position, docked.host!) - MOTION.orbitRadius)).toBeLessThan(5);
    expect(docked.satellite).toBeCloseTo(1, 2);
    world.dispatch({
      id: 'end',
      at: 1.5,
      type: 'end',
      agentId: 'a',
      interactionId: 'one',
      outcome: 'completed',
    });
    expect(world.bodies.get('a')!.phase).toBe('detaching');
    expect(world.snapshot().find((f) => f.id === 'a')!.position).toEqual(docked.position);
    run(world, 2.5);
    const resting = world.snapshot().find((f) => f.id === 'a')!;
    expect(resting.phase).toBe('resting');
    expect(resting.interaction).toBeUndefined();
    expect(resting.satellite).toBeCloseTo(0, 3);
    expect(distance(resting.position, entities[0].home)).toBeLessThan(12);
  });
  it('preserves position and velocity through cancellation and immediate redirection', () => {
    const world = new GraphMotion(entities);
    start(world);
    run(world, 0.2);
    const body = world.bodies.get('a')!,
      position = { ...body.position },
      velocity = { ...body.velocity };
    world.dispatch({
      id: 'cancel',
      at: 0.2,
      type: 'end',
      agentId: 'a',
      interactionId: 'one',
      outcome: 'cancelled',
    });
    start(world, interaction('two', 'a', 'terminal'));
    expect(body.position).toEqual(position);
    expect(body.velocity).toEqual(velocity);
    expect(body.phase).toBe('detaching');
    run(world, 2);
    expect(body.attachment?.interaction.id).toBe('two');
    expect(body.phase).toBe('engaged');
    world.dispatch({
      id: 'late',
      at: 3,
      type: 'end',
      agentId: 'a',
      interactionId: 'one',
      outcome: 'failed',
    });
    expect(body.attachment?.interaction.id).toBe('two');
  });
  it('allocates distinct stable slots and does not reshuffle survivors on departure', () => {
    const world = new GraphMotion(entities);
    start(world, interaction('one', 'a'));
    start(world, interaction('two', 'b'));
    start(world, interaction('three', 'c'));
    run(world, 2);
    const bodies = ['a', 'b', 'c'].map((id) => world.bodies.get(id)!);
    expect(new Set(bodies.map((b) => b.attachment!.slot)).size).toBe(3);
    for (let i = 0; i < 3; i++)
      for (let j = i + 1; j < 3; j++)
        expect(distance(bodies[i].position, bodies[j].position)).toBeGreaterThan(145);
    const slot = bodies[1].attachment!.slot;
    world.dispatch({
      id: 'end-a',
      at: 2,
      type: 'end',
      agentId: 'a',
      interactionId: 'one',
      outcome: 'completed',
    });
    run(world, 0.5);
    expect(bodies[1].attachment!.slot).toBe(slot);
  });
  it('keeps peer docking distinct from initiating work and honors explicit waits', () => {
    const world = new GraphMotion(entities);
    start(world, interaction('peer', 'a', 'b'));
    run(world, 2);
    expect(world.bodies.get('a')!.phase).toBe('engaged');
    expect(world.bodies.get('b')!.phase).toBe('resting');
    world.dispatch({ id: 'wait', at: 2, type: 'wait', agentId: 'a', interactionId: 'peer' });
    expect(world.bodies.get('a')!.phase).toBe('waiting');
    world.dispatch({ id: 'resume', at: 3, type: 'resume', agentId: 'a', interactionId: 'peer' });
    expect(world.bodies.get('a')!.phase).toBe('engaged');
  });
  it('has the same trajectory at 30, 60 and 120Hz', () => {
    const results = [30, 60, 120].map((hz) => {
      const world = new GraphMotion(entities);
      start(world);
      run(world, 0.8, hz);
      return world.bodies.get('a')!.position;
    });
    expect(distance(results[0], results[1])).toBeLessThan(0.0001);
    expect(distance(results[1], results[2])).toBeLessThan(0.0001);
  });
  it('keeps ambient drift bounded and never turns it into operational activity', () => {
    const world = new GraphMotion(entities);
    run(world, 25);
    for (const body of world.bodies.values()) {
      expect(body.phase).toBe('resting');
      expect(body.attachment).toBeUndefined();
      expect(distance(body.position, body.home)).toBeLessThan(12);
    }
  });
  it('reduced motion keeps semantic attachment and termination without drift', () => {
    const world = new GraphMotion(entities);
    start(world);
    world.advance(0.1, true);
    const snapshot = world.snapshot();
    expect(world.bodies.get('a')!.phase).toBe('engaged');
    world.advance(5, true);
    expect(world.snapshot()).toEqual(snapshot);
    world.dispatch({
      id: 'failure',
      at: 5,
      type: 'end',
      agentId: 'a',
      interactionId: 'one',
      outcome: 'failed',
    });
    world.advance(0.01, true);
    expect(world.bodies.get('a')!.phase).toBe('resting');
    expect(world.snapshot().find((f) => f.id === 'a')!.outcome).toBe('failed');
  });
  it('deduplicates input, cancels missing hosts, and never propagates invalid coordinates', () => {
    const world = new GraphMotion(entities);
    start(world);
    run(world, 0.2);
    const position = { ...world.bodies.get('a')!.position };
    start(world);
    expect(world.bodies.get('a')!.position).toEqual(position);
    world.sync(entities.filter((e) => e.id !== 'github'));
    run(world, 2);
    expect(world.bodies.get('a')!.attachment).toBeUndefined();
    for (const frame of world.snapshot())
      expect(Number.isFinite(frame.position.x) && Number.isFinite(frame.position.y)).toBe(true);
  });
});
describe('sample event adapter', () => {
  const agents = mockAgents.filter((a) => a.pledgedTeamId === 'team-platform');
  const places = destinationsFor(agents, mockGraphNodes, mockGraphEdges),
    actions = actionsFor(agents, places, mockAgentTracks),
    examples = examplesFor(agents, places, actions);
  it('uses trace targets and terminal status rather than active topology flags', () => {
    expect(actions).toHaveLength(4);
    const trace = examples.find((e) => e.id === 'trace')!;
    expect(trace.events.filter((e) => e.type === 'start')).toHaveLength(4);
    expect(trace.events.filter((e) => e.type === 'end')).toHaveLength(2);
    expect(actionsFor(agents, places, {})).toEqual([]);
    const player = new MotionPlayback(entitiesFor(agents, places), trace);
    player.seek(trace.duration);
    expect(player.world.bodies.get('agt-doc')!.phase).toBe('engaged');
    player.reset();
    expect(player.world.bodies.get('agt-builder')!.phase).toBe('resting');
    player.advance(0.1, true);
    expect(player.world.bodies.get('agt-builder')!.phase).toBe('approaching');
  });
  it('ends synthetic failure/cancel and shared-orbit examples without ghost attachments', () => {
    for (const example of examples.filter((e) => ['flow', 'shared', 'interrupt'].includes(e.id))) {
      const player = new MotionPlayback(entitiesFor(agents, places), example);
      while (player.elapsed < example.duration) player.advance(1 / 60, true);
      expect(player.world.snapshot().filter((f) => f.interaction)).toEqual([]);
    }
  });
});
