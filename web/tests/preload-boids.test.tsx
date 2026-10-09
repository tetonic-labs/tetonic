import { expect, it } from 'vitest';
import { BoidFlock, neighborSteering, stippleSeeds, type Boid } from '../dev/preload/boids';
import { flockTarget } from '../src/components/preload/flockMotion';

it('seeds a reproducible stipple field with a filled center and minimum point spacing', () => {
  const seeds = stippleSeeds(256);
  expect(seeds).toHaveLength(256);
  expect(seeds).toEqual(stippleSeeds(256));
  expect(seeds.filter((seed) => Math.hypot(seed.x, seed.y) < 0.4).length).toBeGreaterThan(45);
  for (let i = 0; i < seeds.length; i++) {
    for (let j = i + 1; j < seeds.length; j++) {
      expect(Math.hypot(seeds[i].x - seeds[j].x, seeds[i].y - seeds[j].y)).toBeGreaterThanOrEqual(
        0.026,
      );
    }
  }
});

it('separates close neighbors, aligns velocities, and steers toward local centers only', () => {
  const seed = stippleSeeds(1)[0];
  const bird: Boid = { seed, x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, density: 0 };
  const neighbor = { ...bird, x: 0.02, vx: 0.5 };
  const rules = neighborSteering(bird, [bird, neighbor]);
  expect(rules.count).toBe(1);
  expect(rules.separation[0]).toBeLessThan(0);
  expect(rules.alignment[0]).toBeGreaterThan(0);
  expect(rules.cohesion[0]).toBeGreaterThan(0);
  const distant = neighborSteering(bird, [bird, { ...neighbor, x: 10 }]);
  expect(distant.count).toBe(0);
  expect(distant.separation).toEqual([0, 0, 0]);
  expect(distant.alignment).toEqual([0, 0, 0]);
  expect(distant.cohesion).toEqual([0, 0, 0]);
});

it('preserves momentum at stage changes, settles into the targets, and disperses outward', () => {
  const flock = new BoidFlock(160);
  const advance = (frames: number) => {
    for (let i = 0; i < frames; i++) flock.step(1 / 60, 10);
    for (const bird of flock.birds) {
      expect([bird.x, bird.y, bird.z, bird.vx, bird.vy, bird.vz].every(Number.isFinite)).toBe(true);
    }
  };
  const starting = flock.birds.map((bird) => [bird.x, bird.y]);
  advance(120);
  expect(flock.birds.map((bird) => [bird.x, bird.y])).not.toEqual(starting);
  flock.setPhase('waiting');
  advance(120);
  const before = flock.birds.map((bird) => [bird.x, bird.y, bird.vx, bird.vy]);
  flock.setPhase('unavailable');
  expect(flock.birds.map((bird) => [bird.x, bird.y, bird.vx, bird.vy])).toEqual(before);
  advance(120);
  const failureError =
    flock.birds.reduce((sum, bird) => {
      const target = flockTarget(bird.seed, flock.elapsed, 'unavailable');
      return sum + Math.hypot(bird.x - target.x, bird.y - target.y);
    }, 0) / flock.birds.length;
  expect(failureError).toBeLessThan(0.06);
  flock.setPhase('ready');
  advance(78);
  const ringError =
    flock.birds.reduce((sum, bird) => sum + Math.abs(Math.hypot(bird.x, bird.y) - 1.04), 0) /
    flock.birds.length;
  expect(ringError).toBeLessThan(0.12);
  flock.setPhase('departing');
  advance(60);
  expect(flock.birds.every((bird) => Math.hypot(bird.x, bird.y) > 8)).toBe(true);
});

it('provides still target formations without integrating motion', () => {
  const flock = new BoidFlock(50);
  flock.step(1 / 60);
  flock.setPhase('ready', true);
  expect(flock.birds.every((bird) => bird.vx === 0 && bird.vy === 0 && bird.vz === 0)).toBe(true);
  expect(flock.birds.every((bird) => Math.abs(Math.hypot(bird.x, bird.y) - 1.04) < 0.05)).toBe(
    true,
  );
});
