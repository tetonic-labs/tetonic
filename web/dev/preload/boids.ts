import {
  flockTarget,
  type Bird,
  type PreloadPhase,
} from '../../src/components/preload/flockMotion';

export type Boid = {
  seed: Bird;
  x: number;
  y: number;
  z: number;
  vx: number;
  vy: number;
  vz: number;
  density: number;
};
const CELL = 0.2;
const NEIGHBOR_RADIUS_SQUARED = CELL * CELL;
const SEPARATION_RADIUS_SQUARED = 0.048 ** 2;
const cellKey = (x: number, y: number, z: number) => `${x},${y},${z}`;

/** Weighted dart throwing: a soft density field with a minimum projected dot spacing. */
export function stippleSeeds(count: number): Bird[] {
  let seed = 42813;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  const points: Bird[] = [];
  const spacing = Math.min(0.026, 0.72 / Math.sqrt(count));
  const cells = new Map<string, Bird[]>();
  for (let attempt = 0; points.length < count && attempt < count * 100; attempt++) {
    const x = random() * 2 - 1;
    const y = random() * 2 - 1;
    const radius = x * x + y * y;
    if (radius > 1 || random() > Math.exp(-radius * 1.9)) continue;
    const cx = Math.floor(x / spacing);
    const cy = Math.floor(y / spacing);
    let crowded = false;
    for (let ox = -1; ox <= 1 && !crowded; ox++) {
      for (let oy = -1; oy <= 1 && !crowded; oy++) {
        for (const other of cells.get(`${cx + ox},${cy + oy}`) || []) {
          if ((x - other.x) ** 2 + (y - other.y) ** 2 < spacing * spacing) {
            crowded = true;
            break;
          }
        }
      }
    }
    if (crowded) continue;
    const point: Bird = {
      x,
      y,
      z: (random() * 2 - 1) * Math.sqrt(1 - radius),
      phase: random() * Math.PI * 2,
      orbit: (points.length / count) * Math.PI * 2,
      size: 0.65 + random() * 0.7,
    };
    points.push(point);
    const key = `${cx},${cy}`;
    if (!cells.has(key)) cells.set(key, []);
    cells.get(key)!.push(point);
  }
  return points;
}

/** Local Reynolds-style rules. Read the previous frame, then integrate every bird together. */
export function neighborSteering(boid: Boid, neighbors: readonly Boid[]) {
  let count = 0,
    close = 0;
  let sx = 0,
    sy = 0,
    sz = 0,
    ax = 0,
    ay = 0,
    az = 0,
    cx = 0,
    cy = 0,
    cz = 0;
  for (const other of neighbors) {
    if (other === boid) continue;
    const dx = boid.x - other.x,
      dy = boid.y - other.y,
      dz = boid.z - other.z;
    const distanceSquared = dx * dx + dy * dy + dz * dz;
    if (distanceSquared > NEIGHBOR_RADIUS_SQUARED) continue;
    count++;
    ax += other.vx;
    ay += other.vy;
    az += other.vz;
    cx += other.x;
    cy += other.y;
    cz += other.z;
    if (distanceSquared < SEPARATION_RADIUS_SQUARED) {
      const distance = Math.max(0.0001, Math.sqrt(distanceSquared));
      const pressure = 1 - distance / Math.sqrt(SEPARATION_RADIUS_SQUARED);
      sx += (dx / distance) * pressure;
      sy += (dy / distance) * pressure;
      sz += (dz / distance) * pressure;
      close++;
    }
  }
  return {
    count,
    separation: [sx / Math.max(1, close), sy / Math.max(1, close), sz / Math.max(1, close)],
    alignment: count
      ? [ax / count - boid.vx, ay / count - boid.vy, az / count - boid.vz]
      : [0, 0, 0],
    cohesion: count ? [cx / count - boid.x, cy / count - boid.y, cz / count - boid.z] : [0, 0, 0],
  };
}

export class BoidFlock {
  readonly birds: Boid[];
  phase: PreloadPhase;
  elapsed = 6;
  phaseElapsed = 0;

  constructor(count = 1250, phase: PreloadPhase = 'connecting') {
    this.phase = phase;
    this.birds = stippleSeeds(count).map((seed) => {
      const target = flockTarget(seed, this.elapsed, phase === 'departing' ? 'ready' : phase);
      return { seed, x: target.x, y: target.y, z: target.depth, vx: 0, vy: 0, vz: 0, density: 0 };
    });
  }

  setPhase(phase: PreloadPhase, still = false) {
    this.phase = phase;
    this.phaseElapsed = 0;
    if (still) this.settle();
  }

  settle() {
    for (const bird of this.birds) {
      const target = flockTarget(
        bird.seed,
        this.elapsed,
        this.phase === 'departing' ? 'ready' : this.phase,
      );
      bird.x = target.x;
      bird.y = target.y;
      bird.z = target.depth;
      bird.vx = 0;
      bird.vy = 0;
      bird.vz = 0;
    }
  }

  step(seconds: number, reach = 12) {
    const dt = Math.max(0, Math.min(seconds, 1 / 30));
    if (!dt) return;
    this.elapsed += dt;
    this.phaseElapsed += dt;
    const grid = new Map<string, Boid[]>();
    for (const bird of this.birds) {
      const key = cellKey(
        Math.floor(bird.x / CELL),
        Math.floor(bird.y / CELL),
        Math.floor(bird.z / CELL),
      );
      if (!grid.has(key)) grid.set(key, []);
      grid.get(key)!.push(bird);
    }
    const failed = this.phase === 'unavailable';
    const ready = this.phase === 'ready';
    const departing = this.phase === 'departing';
    const spring = ready ? 32 : failed ? 36 : 3.4;
    const damping = ready ? 10 : failed ? 11 : 2.5;
    const maxSpeed = departing ? reach * 2.8 : failed ? 2.5 : ready ? 4 : 1.1;
    const maxAcceleration = departing ? reach * 5 : failed ? 22 : ready ? 24 : 3.5;
    const accelerations = this.birds.map((bird) => {
      if (departing) {
        const radius = Math.max(0.01, Math.hypot(bird.x, bird.y));
        const force = reach * (3.2 + bird.seed.size * 0.5);
        return [(bird.x / radius) * force, (bird.y / radius) * force, 0];
      }
      const bx = Math.floor(bird.x / CELL),
        by = Math.floor(bird.y / CELL),
        bz = Math.floor(bird.z / CELL);
      const neighbors: Boid[] = [];
      for (let x = -1; x <= 1; x++)
        for (let y = -1; y <= 1; y++)
          for (let z = -1; z <= 1; z++) {
            const bucket = grid.get(cellKey(bx + x, by + y, bz + z));
            if (bucket) neighbors.push(...bucket);
          }
      const rules = neighborSteering(bird, neighbors);
      bird.density = rules.count;
      const target = flockTarget(
        bird.seed,
        this.elapsed,
        this.phase as Exclude<PreloadPhase, 'departing'>,
      );
      const next = flockTarget(
        bird.seed,
        this.elapsed + 0.05,
        this.phase as Exclude<PreloadPhase, 'departing'>,
      );
      const desired = [target.x, target.y, target.depth];
      const velocity = failed
        ? [0, 0, 0]
        : [
            (next.x - target.x) / 0.05,
            (next.y - target.y) / 0.05,
            (next.depth - target.depth) / 0.05,
          ];
      const position = [bird.x, bird.y, bird.z];
      const currentVelocity = [bird.vx, bird.vy, bird.vz];
      const separation = ready || failed ? 0.5 : 2.3;
      const alignment = ready || failed ? 0.45 : 1.8;
      const cohesion = ready || failed ? 0.15 : 0.7;
      return desired.map(
        (value, axis) =>
          (value - position[axis]) * spring +
          (velocity[axis] - currentVelocity[axis]) * damping +
          rules.separation[axis] * separation +
          rules.alignment[axis] * alignment +
          rules.cohesion[axis] * cohesion +
          (failed && axis < 2
            ? Math.sin(this.elapsed * (axis ? 43 : 37) + bird.seed.phase * 2) * 9
            : 0),
      );
    });
    this.birds.forEach((bird, index) => {
      const force = accelerations[index];
      const forceScale = Math.min(1, maxAcceleration / Math.max(0.0001, Math.hypot(...force)));
      bird.vx += force[0] * forceScale * dt;
      bird.vy += force[1] * forceScale * dt;
      bird.vz += force[2] * forceScale * dt;
      const speedScale = Math.min(
        1,
        maxSpeed / Math.max(0.0001, Math.hypot(bird.vx, bird.vy, bird.vz)),
      );
      bird.vx *= speedScale;
      bird.vy *= speedScale;
      bird.vz *= speedScale;
      bird.x += bird.vx * dt;
      bird.y += bird.vy * dt;
      bird.z += bird.vz * dt;
    });
  }
}
