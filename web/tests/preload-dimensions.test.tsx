import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { BoidFlock } from '../dev/preload/boids';
import { dimensionalStrength, projectDimensions } from '../dev/preload/dimensions';
import * as dimensions from '../dev/preload/dimensions';
import { DimensionalMurmuration } from '../dev/preload/DimensionalMurmuration';
import { advancePointerAxis } from '../dev/preload/pointerMomentum';
import { sampleStateMap } from '../dev/preload/stateMap';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it('carries momentum through pointer reversals, gently overshoots, and settles consistently across frame rates', () => {
  const axis = { position: 0, velocity: 0 };
  for (let i = 0; i < 12; i++) advancePointerAxis(axis, 0.7, 1 / 60);
  expect(axis.position).toBeGreaterThan(0);
  expect(axis.position).toBeLessThan(0.35);
  expect(axis.velocity).toBeGreaterThan(0);
  const beforeReversal = axis.position;
  advancePointerAxis(axis, -0.7, 1 / 60);
  expect(axis.position).toBeGreaterThan(beforeReversal);
  let furthest = axis.position;
  for (let i = 0; i < 240; i++) {
    advancePointerAxis(axis, -0.7, 1 / 60);
    furthest = Math.min(furthest, axis.position);
  }
  expect(furthest).toBeLessThan(-0.72);
  expect(furthest).toBeGreaterThan(-0.85);
  expect(axis.position).toBeCloseTo(-0.7, 4);
  expect(Math.abs(axis.velocity)).toBeLessThan(0.001);

  const sample = (fps: number) => {
    const state = { position: 0, velocity: 0 };
    for (let i = 0; i < fps / 2; i++) advancePointerAxis(state, 0.8, 1 / fps);
    for (let i = 0; i < fps / 2; i++) advancePointerAxis(state, -0.3, 1 / fps);
    return state;
  };
  expect(sample(30).position).toBeCloseTo(sample(144).position, 10);
  expect(sample(30).velocity).toBeCloseTo(sample(144).velocity, 10);
});

it('maps whole regions jointly, with continuous blends and a different horizontal response at each height', () => {
  const center = sampleStateMap(0, 0);
  const upperRight = sampleStateMap(0.68, -0.6);
  const lowerRight = sampleStateMap(0.72, 0.62);
  expect(center.weights.swell).toBeGreaterThan(0.8);
  expect(upperRight.weights.bloom).toBeGreaterThan(0.9);
  expect(upperRight.spread).toBeGreaterThan(0.9);
  expect(upperRight.fullness).toBeGreaterThan(0.9);
  expect(lowerRight.weights.shoal).toBeGreaterThan(0.9);
  expect(lowerRight.spread).toBeLessThan(0.15);
  expect(lowerRight.fullness).toBeLessThan(0.15);
  const topResponse = sampleStateMap(0.7, -0.6).spread - sampleStateMap(-0.7, -0.6).spread;
  const bottomResponse = sampleStateMap(0.7, 0.6).spread - sampleStateMap(-0.7, 0.6).spread;
  expect(Math.abs(topResponse - bottomResponse)).toBeGreaterThan(0.35);
  for (const x of [-1, -0.5, 0, 0.5, 1]) {
    for (const y of [-1, -0.5, 0, 0.5, 1]) {
      const a = sampleStateMap(x, y);
      const b = sampleStateMap(x + 0.0001, y + 0.0001);
      expect(Object.values(a.weights).reduce((sum, weight) => sum + weight, 0)).toBeCloseTo(1, 12);
      expect(Object.values(a.weights).every((weight) => weight >= 0 && weight <= 1)).toBe(true);
      expect(Math.abs(a.spread - b.spread) + Math.abs(a.fullness - b.fullness)).toBeLessThan(0.001);
    }
  }
});

it('blends distinct volumes continuously without rotating or moving the simulated boids', () => {
  const flock = new BoidFlock(90);
  const before = structuredClone(flock.birds);
  const view = { horizontal: 0, vertical: 0, strength: 1 };
  let formationChange = 0,
    deformation = 0;
  for (const bird of flock.birds) {
    const center = projectDimensions(bird, 6, view);
    const bloom = projectDimensions(bird, 6, { ...view, horizontal: 0.68, vertical: -0.6 });
    const braid = projectDimensions(bird, 6, { ...view, horizontal: -0.64, vertical: 0.65 });
    formationChange += Math.hypot(bloom.x - braid.x, bloom.y - braid.y, bloom.z - braid.z);
    const adjacent = projectDimensions(bird, 6, { ...view, horizontal: 0.0001 });
    expect(
      Math.hypot(adjacent.x - center.x, adjacent.y - center.y, adjacent.z - center.z),
    ).toBeLessThan(0.001);
    for (const horizontal of [-1, 0, 1]) {
      for (const vertical of [-1, 0, 1]) {
        const point = projectDimensions(bird, 6, { horizontal, vertical, strength: 1.08 });
        expect(Object.values(point).every(Number.isFinite)).toBe(true);
        expect(point.visibility).toBeGreaterThan(0);
        expect(point.visibility).toBeLessThanOrEqual(1);
        expect(Math.hypot(point.x, point.y)).toBeLessThan(3.5);
      }
    }
  }
  expect(formationChange / flock.birds.length).toBeGreaterThan(0.3);
  // A rigid camera rotation would preserve every pairwise 3D distance.
  for (let i = 1; i < flock.birds.length; i++) {
    const distance = (horizontal: number, vertical: number) => {
      const a = projectDimensions(flock.birds[i - 1], 6, { ...view, horizontal, vertical });
      const b = projectDimensions(flock.birds[i], 6, { ...view, horizontal, vertical });
      return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
    };
    deformation += Math.abs(distance(0.68, -0.6) - distance(0, 0));
  }
  expect(deformation / flock.birds.length).toBeGreaterThan(0.1);
  expect(flock.birds).toEqual(before);
});

it('preserves the ready orbit and outward departure regardless of the selected dimensions', () => {
  const flock = new BoidFlock(60, 'ready');
  for (const bird of flock.birds) {
    const point = projectDimensions(bird, 6, {
      horizontal: 1,
      vertical: -1,
      strength: dimensionalStrength('ready'),
    });
    expect(Math.abs(Math.hypot(point.x, point.y) - 1.04)).toBeLessThan(0.05);
  }
  flock.setPhase('departing');
  for (let i = 0; i < 60; i++) flock.step(1 / 60, 10);
  expect(
    flock.birds.every((bird) => {
      const point = projectDimensions(bird, 7, {
        horizontal: -1,
        vertical: 1,
        strength: dimensionalStrength('departing'),
      });
      return Math.hypot(point.x, point.y) > 8;
    }),
  ).toBe(true);
  expect(dimensionalStrength('unavailable')).toBeLessThan(dimensionalStrength('connecting'));
});

it('supports pointer and keyboard exploration, respects reduced motion, and cleans up on switching experiments', () => {
  let observedView = { horizontal: 0, vertical: 0, strength: 1 };
  const project = dimensions.projectDimensions;
  vi.spyOn(dimensions, 'projectDimensions').mockImplementation((bird, time, view, state) => {
    observedView = { ...view };
    return project(bird, time, view, state);
  });
  const context = {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
  };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
    context as unknown as CanvasRenderingContext2D,
  );
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    top: 0,
    width: 900,
    height: 600,
    right: 900,
    bottom: 600,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  const media = { matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() };
  vi.stubGlobal('matchMedia', () => media);
  const disconnect = vi.fn();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = disconnect;
    },
  );
  let callback: FrameRequestCallback = () => {};
  const request = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((next) => {
    callback = next;
    return 42;
  });
  const cancel = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(false);
  const remove = vi.spyOn(window, 'removeEventListener');
  const mount = render(<DimensionalMurmuration phase="connecting" />);
  const flock = screen.getByRole('group', { name: 'Explore flock dimensions' });
  const frameAt = (time: number) => {
    context.arc.mockClear();
    act(() => callback(time));
    return context.arc.mock.calls.map((call) => call.slice(0, 2));
  };
  const initial = frameAt(100);
  // MouseEvent provides coordinates in jsdom, whose PointerEvent support is limited.
  fireEvent(window, new MouseEvent('pointermove', { clientX: 900, clientY: 50 }));
  const moved = frameAt(100 + 1000 / 60);
  expect(moved).not.toEqual(initial);
  expect(observedView.horizontal).toBeGreaterThan(0);
  expect(observedView.vertical).toBeLessThan(0);
  fireEvent.keyDown(flock, { key: 'ArrowDown' });
  expect(frameAt(100 + 2000 / 60)).not.toEqual(moved);
  hidden.mockReturnValue(true);
  const calls = request.mock.calls.length;
  fireEvent(document, new Event('visibilitychange'));
  expect(request).toHaveBeenCalledTimes(calls);
  expect(cancel).toHaveBeenLastCalledWith(42);
  media.matches = true;
  hidden.mockReturnValue(false);
  act(() => media.addEventListener.mock.calls[0][1]());
  context.arc.mockClear();
  fireEvent.keyDown(flock, { key: 'ArrowRight' });
  fireEvent(window, new MouseEvent('pointermove', { clientX: 50, clientY: 900 }));
  expect(context.arc).not.toHaveBeenCalled();
  expect(request).toHaveBeenCalledTimes(calls);
  mount.rerender(<DimensionalMurmuration phase="ready" />);
  expect(context.arc).toHaveBeenCalled();
  expect(request).toHaveBeenCalledTimes(calls);
  mount.unmount();
  expect(disconnect).toHaveBeenCalledOnce();
  expect(remove).toHaveBeenCalledWith('pointermove', expect.any(Function));
  expect(remove).toHaveBeenCalledWith('pointerout', expect.any(Function));
  expect(remove).toHaveBeenCalledWith('blur', expect.any(Function));
});
