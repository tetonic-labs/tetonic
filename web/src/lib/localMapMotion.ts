import { GraphMotion, type Entity, type MotionFrame, type Interaction } from './graphMotion';
import type { WorkState } from './workScene';
import type { Point } from './mapActivity';
import { agentFootprint, toolFootprint } from './clusterClearance';

export const LOCAL_AGENT_LIMIT = 6;
// Roughly one neighborhood plus its shared resources; remote work uses routes.
const LOCAL_TRAVEL_RADIUS = 1200;

function nearEnough(actor: Entity | undefined, host: Entity | undefined) {
  return (
    !!actor &&
    !!host &&
    Math.hypot(actor.home.x - host.home.x, actor.home.y - host.home.y) <= LOCAL_TRAVEL_RADIUS
  );
}

// A bounded physical lens onto the organization, using the original cluster solver.
// The operational clock belongs to OrganizationActivity, never to this lens.
export class LocalMapMotion {
  world = new GraphMotion([], false);
  private current = new Map<string, { action: Interaction; waiting: boolean }>();
  private revision = 0;
  private time = 0;
  private epoch = '';
  sync(entities: Entity[], states: Map<string, WorkState>, time: number, epoch: string) {
    if (epoch !== this.epoch || time < this.time - 0.001) {
      this.world = new GraphMotion([], false);
      this.current.clear();
      this.epoch = epoch;
    }
    this.time = time;
    this.world.sync(entities);
    const actors = new Set(entities.filter((e) => e.kind === 'agent').map((e) => e.id));
    for (const id of new Set([...actors, ...this.current.keys()])) {
      const work = states.get(id),
        before = this.current.get(id);
      const action =
        actors.has(id) &&
        work?.interaction &&
        nearEnough(this.world.bodies.get(id), this.world.bodies.get(work.interaction.targetId))
          ? work.interaction
          : undefined;
      if (before && before.action.id !== action?.id) {
        this.world.dispatch({
          id: `local-end-${++this.revision}`,
          type: 'end',
          at: time,
          agentId: id,
          interactionId: before.action.id,
          outcome:
            work?.previous?.interaction.id === before.action.id
              ? work.previous.outcome
              : 'cancelled',
        });
        this.current.delete(id);
      }
      if (action && this.world.bodies.has(action.targetId)) {
        if (before?.action.id !== action.id)
          this.world.dispatch({
            id: `local-start-${++this.revision}`,
            type: 'start',
            at: time,
            interaction: action,
          });
        if (before?.action.id !== action.id || before.waiting !== !!work?.waiting)
          this.world.dispatch({
            id: `local-state-${++this.revision}`,
            type: work?.waiting ? 'wait' : 'resume',
            at: time,
            agentId: id,
            interactionId: action.id,
          });
        this.current.set(id, { action, waiting: !!work?.waiting });
      }
    }
  }
  advance(seconds: number, reduced: boolean): MotionFrame[] {
    this.world.advance(seconds, reduced);
    return this.world.snapshot();
  }
}

export function localEntities(
  points: Map<string, Point>,
  actors: string[],
  states: Map<string, WorkState>,
  agentIds: Set<string>,
  names: Map<string, string>,
): Entity[] {
  const actorSet = new Set(actors.slice(0, LOCAL_AGENT_LIMIT));
  const required = new Set(actorSet);
  for (const id of actorSet) {
    const work = states.get(id);
    for (const target of [work?.interaction?.targetId, work?.previous?.interaction.targetId])
      if (target && points.has(target)) required.add(target);
  }
  const anchors = [...required].flatMap((id) => (points.has(id) ? [points.get(id)!] : []));
  const nearby = [...points]
    .filter(([id]) => !required.has(id))
    .map(([id, p]) => ({
      id,
      distance: Math.min(...anchors.map((a) => Math.hypot(a.x - p.x, a.y - p.y))),
    }))
    .filter((p) => p.distance < 700)
    .sort((a, b) => a.distance - b.distance || a.id.localeCompare(b.id))
    .slice(0, 24);
  return [...required, ...nearby.map((p) => p.id)].flatMap((id) => {
    const home = points.get(id);
    if (!home) return [];
    const footprint = agentIds.has(id) ? agentFootprint : toolFootprint;
    // Long names wrap in the DOM; keep their collision footprint within that width.
    const width = Math.min(
      agentIds.has(id) ? 112 : 150,
      Math.max(footprint.right, (names.get(id)?.length || 0) * 4),
    );
    return [
      {
        id,
        kind: actorSet.has(id) ? ('agent' as const) : ('destination' as const),
        home,
        bounds: { ...footprint, left: width, right: width },
      },
    ];
  });
}

export type MapSignal = 'input' | 'blocked' | 'waiting' | 'working' | 'idle';
export function mapSignal(work: WorkState | undefined, needsInput: boolean): MapSignal {
  return needsInput
    ? 'input'
    : work?.failure
      ? 'blocked'
      : work?.waiting
        ? 'waiting'
        : work?.interaction
          ? 'working'
          : 'idle';
}
