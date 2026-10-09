import type { PreloadPhase } from '../../src/components/preload/flockMotion';
import type { Boid } from './boids';
import { sampleStateMap, type FlockStateMap } from './stateMap';

export type DimensionalView = { horizontal: number; vertical: number; strength: number };
export type DimensionalPoint = { x: number; y: number; z: number; visibility: number };
export const clampAxis = (value: number) => Math.max(-1, Math.min(1, value));

export function dimensionalStrength(phase: PreloadPhase) {
  if (phase === 'ready' || phase === 'departing') return 0;
  return phase === 'unavailable' ? 0.28 : phase === 'waiting' ? 1.08 : 1;
}

/** A five-coordinate embedding of the 3D boids, not a second flock simulation.
 * A 2D state field selects blends of complete folded volumes. Each volume keeps
 * its own evolving motion, so moving across the map cannot jump its time phase.
 */
export function projectDimensions(
  bird: Boid,
  time: number,
  view: DimensionalView,
  state: FlockStateMap = sampleStateMap(view.horizontal, view.vertical),
): DimensionalPoint {
  const strength = Math.max(0, Math.min(1.1, view.strength));
  if (!strength) return { x: bird.x, y: bird.y, z: bird.z, visibility: 1 };
  const { seed } = bird;
  // Correlated hidden coordinates form folded sheets instead of random jitter.
  const w = Math.sin(seed.x * 3.8 + seed.z * 1.6 + time * 0.17) * 0.92 + seed.y * 0.38;
  const v = Math.cos(seed.x * 3.1 - seed.y * 2.2 - time * 0.13) * 0.58 + seed.z * 0.48;
  const wave = seed.x * 4.2 + time * 0.5 + w * 0.5;
  const ribbon = seed.x * 3.6 + time * 0.3;
  const strand = Math.tanh(seed.y * 12);
  // Fixed particle identities carry through every blend, retaining the boids'
  // local motion. These deform the volume without rotating the entire flock.
  const volumes = {
    swell: [bird.x, bird.y, bird.z],
    ripple: [
      bird.x * 1.15 + Math.sin(seed.y * 3 + time * 0.2) * 0.12,
      bird.y * 0.6 + Math.sin(wave) * 0.28,
      bird.z * 0.85 + Math.cos(wave) * 0.12,
    ],
    bloom: [
      bird.x * 0.8 + seed.z * 0.45 + Math.sin(seed.y * 3 + time * 0.17) * 0.3,
      bird.y * 1.4 + seed.y * 0.4 + Math.sin(seed.x * 2.7 + time * 0.21) * 0.32,
      bird.z * 1.4 + v * 0.3,
    ],
    braid: [
      bird.x * 1.1 + Math.sin(ribbon) * seed.z * 0.1,
      bird.y * 0.32 + Math.sin(ribbon) * strand * 0.4 + seed.z * 0.12,
      bird.z * 0.45 + Math.cos(ribbon) * strand * 0.4,
    ],
    shoal: [
      bird.x * 0.6 + Math.tanh(seed.x * 7) * 0.4,
      bird.y * 0.85 + Math.cos(seed.x * 3.8 + time * 0.25) * 0.18,
      bird.z * 0.85 + w * 0.12,
    ],
  };
  let x = 0,
    y = 0,
    z = 0;
  for (const name of Object.keys(state.weights) as (keyof typeof volumes)[]) {
    const weight = state.weights[name];
    x += volumes[name][0] * weight;
    y += volumes[name][1] * weight;
    z += volumes[name][2] * weight;
  }
  const section = Math.exp(
    -((w - state.weights.bloom * 0.8) ** 2 + (v - state.weights.braid * 0.7) ** 2) * 1.7,
  );
  return {
    x: bird.x + (x - bird.x) * strength,
    y: bird.y + (y - bird.y) * strength,
    z: bird.z + (z - bird.z) * strength,
    visibility: 1 - Math.min(1, strength) * 0.8 * (1 - section),
  };
}
