import { Point } from './mapActivity';
import {
  agentFootprint,
  toolFootprint,
  clearanceTargets,
  CollisionNode,
  Footprint,
} from './clusterClearance';

// Presentation physics only. Operational state comes exclusively from dispatch().
export const MOTION = {
  step: 1 / 120,
  approach: { stiffness: 85, damping: 18 },
  capture: { stiffness: 230, damping: 22 },
  rest: { stiffness: 42, damping: 12 },
  orbitRadius: 220,
  peerRadius: 220,
  captureDistance: 75,
  releaseSeconds: 0.32,
  maxSpeed: 1800,
  ambientAmplitude: 7,
  separation: 145,
  satellite: { stiffness: 280, damping: 21 },
} as const;

export type Phase =
  | 'resting'
  | 'approaching'
  | 'docking'
  | 'engaged'
  | 'waiting'
  | 'detaching'
  | 'returning';
export type Outcome = 'completed' | 'failed' | 'cancelled';
export interface Interaction {
  id: string;
  workflowId?: string;
  retryOf?: string;
  agentId: string;
  targetId: string;
  targetName: string;
  tool?: string;
  label: string;
}
export type InteractionEvent =
  | { id: string; at: number; type: 'start'; interaction: Interaction }
  | {
      id: string;
      at: number;
      type: 'end';
      agentId: string;
      interactionId: string;
      outcome: Outcome;
    }
  | { id: string; at: number; type: 'wait' | 'resume'; agentId: string; interactionId: string };
export interface Entity {
  id: string;
  kind: 'agent' | 'destination';
  home: Point;
  bounds?: Footprint;
}
interface Attachment {
  interaction: Interaction;
  slot: number;
}
export interface Body extends Entity {
  position: Point;
  velocity: Point;
  phase: Phase;
  age: number;
  seed: number;
  attachment?: Attachment;
  departing?: Attachment;
  queued?: Interaction;
  queuedWaiting?: boolean;
  waiting: boolean;
  releaseGoal?: Point;
  satellite: number;
  satelliteVelocity: number;
  outcome?: Outcome;
  outcomeAge: number;
}
export interface MotionFrame {
  id: string;
  position: Point;
  phase: Phase;
  interaction?: Interaction;
  host?: Point;
  radius?: number;
  angle?: number;
  attachment: number;
  satellite: number;
  outcome?: Outcome;
}
const length = (v: Point) => Math.hypot(v.x, v.y);
const difference = (a: Point, b: Point): Point => ({ x: a.x - b.x, y: a.y - b.y });
const angles = [-0.22, -0.22 - Math.PI / 2, -0.22 + Math.PI, -0.22 + Math.PI / 2];
const seedFor = (id: string) =>
  ([...id].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 7) / 4294967296) * Math.PI * 2;

export class GraphMotion {
  readonly bodies = new Map<string, Body>();
  private seen = new Set<string>();
  private accumulator = 0;
  private reducedMotion = false;
  private offsets = new Map<string, Point>();
  private offsetVelocity = new Map<string, Point>();
  time = 0;
  constructor(entities: Entity[]) {
    this.sync(entities);
  }

  sync(entities: Entity[]) {
    const ids = new Set(entities.map((e) => e.id));
    for (const id of this.bodies.keys()) if (!ids.has(id)) this.bodies.delete(id);
    for (const entity of entities) {
      const existing = this.bodies.get(entity.id);
      if (existing) existing.home = { ...entity.home };
      else
        this.bodies.set(entity.id, {
          ...entity,
          home: { ...entity.home },
          position: { ...entity.home },
          velocity: { x: 0, y: 0 },
          phase: 'resting',
          age: 0,
          seed: seedFor(entity.id),
          waiting: false,
          satellite: 0,
          satelliteVelocity: 0,
          outcomeAge: 0,
        });
    }
    for (const body of this.bodies.values()) {
      const host = body.attachment?.interaction.targetId;
      if (host && !ids.has(host)) this.release(body, 'cancelled');
    }
  }

  dispatch(event: InteractionEvent) {
    if (this.seen.has(event.id)) return;
    this.seen.add(event.id);
    if (event.type === 'start') {
      const action = event.interaction,
        body = this.bodies.get(action.agentId);
      if (
        !body ||
        body.kind !== 'agent' ||
        !this.bodies.has(action.targetId) ||
        action.targetId === action.agentId
      )
        return;
      if (body.attachment?.interaction.id === action.id || body.queued?.id === action.id) return;
      if (body.attachment || body.departing) {
        if (body.attachment) this.release(body);
        body.queued = action;
        body.queuedWaiting = false;
      } else this.attach(body, action);
      return;
    }
    const body = this.bodies.get(event.agentId);
    if (!body) return;
    // A late result from previous work must not end or resume a newer interaction.
    if (body.queued?.id === event.interactionId) {
      if (event.type === 'end') {
        body.queued = undefined;
        body.queuedWaiting = false;
        body.outcome = event.outcome;
        body.outcomeAge = 0;
      } else body.queuedWaiting = event.type === 'wait';
      return;
    }
    if (body.attachment?.interaction.id !== event.interactionId) return;
    if (event.type === 'end') this.release(body, event.outcome);
    else {
      body.waiting = event.type === 'wait';
      if (body.phase === 'engaged' || body.phase === 'waiting')
        body.phase = body.waiting ? 'waiting' : 'engaged';
    }
  }

  private attach(body: Body, action: Interaction) {
    const occupied = new Set(
      [...this.bodies.values()].flatMap((b) =>
        [b.attachment, b.departing]
          .filter((a) => a?.interaction.targetId === action.targetId)
          .map((a) => a!.slot),
      ),
    );
    let slot = 0;
    while (occupied.has(slot)) slot++;
    body.attachment = { interaction: action, slot };
    body.departing = undefined;
    body.queued = undefined;
    body.queuedWaiting = false;
    body.phase = 'approaching';
    body.age = 0;
    body.waiting = false;
    body.outcome = undefined;
    // Velocity is deliberately retained through redirection and new work.
  }

  private release(body: Body, outcome?: Outcome) {
    const old = body.attachment;
    if (!old) return;
    const host = this.bodies.get(old.interaction.targetId)?.position || body.home;
    const radial = difference(body.position, host),
      distance = Math.max(1, length(radial));
    const direction = { x: radial.x / distance, y: radial.y / distance };
    body.releaseGoal = {
      x: body.position.x + direction.x * 48 - direction.y * 24 + body.velocity.x * 0.075,
      y: body.position.y + direction.y * 48 + direction.x * 24 + body.velocity.y * 0.075,
    };
    body.departing = old;
    body.attachment = undefined;
    body.queued = undefined;
    body.phase = 'detaching';
    body.age = 0;
    body.waiting = false;
    body.outcome = outcome;
    body.outcomeAge = 0;
  }

  private orbit(body: Body, attachment: Attachment, ambient: boolean) {
    const hostBody = this.bodies.get(attachment.interaction.targetId);
    const host = hostBody?.position || body.home;
    const radius =
      (hostBody?.kind === 'agent' ? MOTION.peerRadius : MOTION.orbitRadius) +
      Math.floor(attachment.slot / angles.length) * 260;
    const angle =
      angles[attachment.slot % angles.length] +
      (ambient ? Math.sin(this.time * 0.66 + body.seed) * 0.025 : 0);
    return {
      host,
      radius,
      angle,
      point: { x: host.x + Math.cos(angle) * radius, y: host.y + Math.sin(angle) * radius },
    };
  }

  advance(seconds: number, reduced = false) {
    this.reducedMotion = reduced;
    if (reduced) {
      for (const body of this.bodies.values()) body.outcomeAge += seconds;
      this.settle();
      return;
    }
    // Fixed substeps make springs independent of display refresh rate; a long stall
    // is bounded. Visibility handling in the driver prevents background catch-up.
    this.accumulator += Math.min(0.25, Math.max(0, seconds));
    while (this.accumulator + 1e-9 >= MOTION.step) {
      this.integrate(MOTION.step);
      this.accumulator -= MOTION.step;
    }
  }

  private integrate(dt: number) {
    this.time += dt;
    const forces = new Map<string, Point>();
    for (const b of this.bodies.values()) {
      b.age += dt;
      b.outcomeAge += dt;
      if (b.phase === 'detaching' && b.age >= MOTION.releaseSeconds) {
        b.departing = undefined;
        if (b.queued) {
          const waiting = b.queuedWaiting;
          this.attach(b, b.queued);
          b.waiting = !!waiting;
        } else {
          b.phase = 'returning';
          b.age = 0;
        }
      }
      const amplitude = b.kind === 'agent' ? MOTION.ambientAmplitude : 3.5;
      let goal = {
        x: b.home.x + Math.sin(this.time * 0.47 + b.seed) * amplitude,
        y: b.home.y + Math.cos(this.time * 0.61 + b.seed * 1.7) * amplitude,
      };
      let spring: { stiffness: number; damping: number } = MOTION.rest;
      if (b.attachment) {
        const orbit = this.orbit(b, b.attachment, true);
        goal = orbit.point;
        const distance = length(difference(goal, b.position));
        if (b.phase === 'approaching' && distance < MOTION.captureDistance) {
          b.phase = 'docking';
          b.age = 0;
          const host = this.bodies.get(b.attachment.interaction.targetId);
          if (host?.kind === 'destination') {
            host.velocity.x += b.velocity.x * 0.018;
            host.velocity.y += b.velocity.y * 0.018;
          }
        }
        if (b.phase === 'docking' && distance < 4 && length(b.velocity) < 22) {
          b.phase = b.waiting ? 'waiting' : 'engaged';
          b.age = 0;
        }
        spring =
          b.phase === 'approaching'
            ? MOTION.approach
            : b.phase === 'docking'
              ? MOTION.capture
              : MOTION.rest;
      } else if (b.phase === 'detaching') {
        goal = b.releaseGoal!;
        spring = MOTION.approach;
      } else if (b.phase === 'returning') {
        spring = MOTION.approach;
        if (length(difference(goal, b.position)) < 5 && length(b.velocity) < 20) {
          b.phase = 'resting';
          b.age = 0;
        }
      }
      const delta = difference(goal, b.position);
      const force = {
        x: delta.x * spring.stiffness - b.velocity.x * spring.damping,
        y: delta.y * spring.stiffness - b.velocity.y * spring.damping,
      };
      if (b.phase === 'approaching') {
        const d = Math.max(1, length(delta)),
          bend = 1100 * Math.exp(-b.age * 5);
        force.x -= (delta.y / d) * bend;
        force.y += (delta.x / d) * bend;
      }
      forces.set(b.id, force);
      const toolTarget =
        b.attachment && ['docking', 'engaged', 'waiting'].includes(b.phase) ? 1 : 0;
      b.satelliteVelocity +=
        ((toolTarget - b.satellite) * MOTION.satellite.stiffness -
          b.satelliteVelocity * MOTION.satellite.damping) *
        dt;
      b.satellite += b.satelliteVelocity * dt;
    }
    const bodies = [...this.bodies.values()];
    for (const b of bodies) {
      const force = forces.get(b.id)!;
      b.velocity.x += force.x * dt;
      b.velocity.y += force.y * dt;
      const speed = length(b.velocity);
      if (speed > MOTION.maxSpeed) {
        b.velocity.x *= MOTION.maxSpeed / speed;
        b.velocity.y *= MOTION.maxSpeed / speed;
      }
      b.position.x += b.velocity.x * dt;
      b.position.y += b.velocity.y * dt;
    }
    this.resolveClearance(dt);
  }

  private clusterFor(body: Body): string {
    const chain = new Set<string>();
    let current = body;
    while (!chain.has(current.id)) {
      chain.add(current.id);
      const attachment = current.attachment || current.departing;
      const host = attachment && this.bodies.get(attachment.interaction.targetId);
      if (!host) return current.id;
      current = host;
    }
    return [...chain].sort()[0];
  }

  private resolveClearance(dt: number, immediate = false) {
    const nodes: CollisionNode[] = [];
    const active = new Set<string>();
    for (const body of this.bodies.values()) {
      const cluster = this.clusterFor(body);
      nodes.push({
        id: body.id,
        cluster,
        position: body.position,
        bounds: body.bounds || (body.kind === 'agent' ? agentFootprint : toolFootprint),
      });
      if (body.attachment || body.departing || body.phase === 'returning') active.add(cluster);
      const offset = this.offsets.get(cluster);
      if (offset && Math.hypot(offset.x, offset.y) > 1) active.add(cluster);
    }
    const targets = clearanceTargets(nodes, this.offsets, active, immediate ? 0 : 0.97);
    const resolved = new Map<string, Point>();
    for (const [id, target] of targets) {
      const offset = { ...(this.offsets.get(id) || { x: 0, y: 0 }) };
      const velocity = { ...(this.offsetVelocity.get(id) || { x: 0, y: 0 }) };
      if (immediate) {
        offset.x = target.x;
        offset.y = target.y;
        velocity.x = velocity.y = 0;
      } else {
        velocity.x += ((target.x - offset.x) * 100 - velocity.x * 20) * dt;
        velocity.y += ((target.y - offset.y) * 100 - velocity.y * 20) * dt;
        offset.x += velocity.x * dt;
        offset.y += velocity.y * dt;
      }
      resolved.set(id, offset);
      this.offsetVelocity.set(id, velocity);
    }
    for (const node of nodes) {
      this.offsets.set(node.id, { ...resolved.get(node.cluster)! });
      this.offsetVelocity.set(node.id, { ...this.offsetVelocity.get(node.cluster)! });
    }
  }

  settle() {
    this.reducedMotion = true;
    // Manual steps/reduced motion use identical attachments, without travel or drift.
    for (const b of this.bodies.values()) {
      if (b.departing) {
        b.departing = undefined;
        if (b.queued) {
          const waiting = b.queuedWaiting;
          this.attach(b, b.queued);
          b.waiting = !!waiting;
        }
      }
      b.position = { ...b.home };
      b.velocity = { x: 0, y: 0 };
      b.satelliteVelocity = 0;
      if (!b.attachment) {
        b.phase = 'resting';
        b.satellite = 0;
      }
    }
    // Destinations are placed first; peer hosts then resolve from their current position.
    const visited = new Set<string>();
    const place = (b: Body) => {
      if (visited.has(b.id)) return;
      visited.add(b.id);
      if (b.attachment) {
        const host = this.bodies.get(b.attachment.interaction.targetId);
        if (host) place(host);
        b.position = this.orbit(b, b.attachment, false).point;
        b.phase = b.waiting ? 'waiting' : 'engaged';
        b.satellite = 1;
      }
    };
    for (const b of this.bodies.values()) place(b);
    this.resolveClearance(0, true);
  }

  snapshot(): MotionFrame[] {
    return [...this.bodies.values()].map((b) => {
      const attachment = b.attachment || b.departing;
      const orbit = attachment ? this.orbit(b, attachment, !this.reducedMotion) : undefined;
      return {
        id: b.id,
        position: {
          x: b.position.x + (this.offsets.get(b.id)?.x || 0),
          y: b.position.y + (this.offsets.get(b.id)?.y || 0),
        },
        phase: b.phase,
        interaction: attachment?.interaction,
        host: orbit
          ? {
              x: orbit.host.x + (this.offsets.get(b.id)?.x || 0),
              y: orbit.host.y + (this.offsets.get(b.id)?.y || 0),
            }
          : undefined,
        radius: orbit?.radius,
        angle: orbit?.angle,
        attachment: b.departing
          ? Math.max(0, 1 - b.age / MOTION.releaseSeconds)
          : b.attachment
            ? 1
            : 0,
        satellite: b.satellite,
        outcome: b.outcomeAge < 2.4 ? b.outcome : undefined,
      };
    });
  }
}
