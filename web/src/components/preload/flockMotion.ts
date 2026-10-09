export type PreloadPhase = 'connecting' | 'waiting' | 'unavailable' | 'ready' | 'departing';

export const READY_HOLD_MS = 1300;
export const DISPERSAL_MS = 1000;
export const MORPH_SECONDS = 0.8;
const TAU = Math.PI * 2;

export type FlockPoint = { x: number; y: number; depth: number; perspective: number };
export type Bird = { x: number; y: number; z: number; phase: number; orbit: number; size: number };

// Fill the entire volume, including its center. Only the ready state has an orbit.
export function makeFlock(count: number): Bird[] {
  let seed = 7341;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  return Array.from({ length: count }, (_, index) => {
    const azimuth = random() * TAU;
    const elevation = random() * 2 - 1;
    const radius = Math.pow(random(), 0.48);
    const plane = Math.sqrt(1 - elevation * elevation) * radius;
    return {
      x: Math.cos(azimuth) * plane,
      y: Math.sin(azimuth) * plane,
      z: elevation * radius,
      phase: random() * TAU,
      orbit: (index / count) * TAU,
      size: 0.65 + random() * 0.7,
    };
  });
}

export function flockTarget(
  bird: Bird,
  time: number,
  phase: Exclude<PreloadPhase, 'departing'>,
): FlockPoint {
  if (phase === 'ready') {
    const angle = bird.orbit + time * 0.55;
    const radius = 1.04 + bird.z * 0.045;
    return {
      x: Math.cos(angle) * radius,
      y: Math.sin(angle) * radius,
      depth: bird.z * 0.08,
      perspective: 1,
    };
  }
  if (phase === 'unavailable') {
    const x = bird.x * 1.65;
    const zigzag = (2 / Math.PI) * Math.asin(Math.sin(x * 4.8 + 0.4));
    // A stationary, angular silhouette with tiny local vibrations, never flashing.
    return {
      x: x + Math.sin(time * 37 + bird.phase) * 0.014,
      y: zigzag * 0.24 + bird.y * 0.12 + Math.sin(time * 43 + bird.phase * 2) * 0.018,
      depth: bird.z * 0.12,
      perspective: 1,
    };
  }
  const waiting = phase === 'waiting';
  time *= waiting ? 0.62 : 1;
  const breath = 1 + Math.sin(time * 0.54) * (waiting ? 0.12 : 0.075);
  const wave = bird.x * 2.8 - time * 0.43;
  const fullness = 0.82 + Math.sin(wave) * 0.25 + Math.cos(bird.x * 4.7 + time * 0.26) * 0.12;
  const turn = time * 0.12 + bird.x * 0.45;
  const cross = bird.y * Math.cos(turn) - bird.z * Math.sin(turn);
  const back = bird.y * Math.sin(turn) + bird.z * Math.cos(turn);
  const x =
    (bird.x * (waiting ? 1.94 : 1.78) +
      Math.sin(wave * 0.7) * 0.17 +
      Math.sin(bird.phase + time * 0.4) * 0.025) *
    breath;
  const y =
    (cross * (waiting ? 0.54 : 0.48) * fullness +
      Math.sin(bird.x * 2.2 + time * 0.34) * 0.19 +
      Math.sin(wave * 1.8) * 0.06) *
    breath;
  const z = back * 0.34 + Math.sin(wave) * 0.1;
  const yaw = Math.sin(time * 0.11) * 0.12;
  const pitch = Math.cos(time * 0.13) * 0.05 + 0.04;
  const rx = x * Math.cos(yaw) + z * Math.sin(yaw);
  const rz = z * Math.cos(yaw) - x * Math.sin(yaw);
  const ry = y * Math.cos(pitch) - rz * Math.sin(pitch);
  const depth = y * Math.sin(pitch) + rz * Math.cos(pitch);
  const perspective = 6.5 / (6.5 + depth);
  return { x: rx * perspective, y: ry * perspective, depth, perspective };
}

export function disperse(
  source: FlockPoint,
  bird: Bird,
  progress: number,
  reach: number,
): FlockPoint {
  const angle = Math.atan2(source.y, source.x);
  const distance =
    Math.pow(Math.max(0, Math.min(1, progress)), 1.65) * reach * (0.75 + bird.size * 0.25);
  return {
    ...source,
    x: source.x + Math.cos(angle) * distance,
    y: source.y + Math.sin(angle) * distance,
  };
}
