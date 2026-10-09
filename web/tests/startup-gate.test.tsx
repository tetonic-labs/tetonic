import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { StartupGate } from '../src/components/preload/StartupGate';
import { Murmuration } from '../src/components/preload/Murmuration';
import {
  DISPERSAL_MS,
  READY_HOLD_MS,
  disperse,
  flockTarget,
  makeFlock,
} from '../src/components/preload/flockMotion';
import { LocalEngineProvider, useLocalEngine } from '../src/context/LocalEngineContext';
import { LocalEngine } from '../src/engine/client';
import type { EngineWorkspace } from '../src/engine/contracts';

const workspace: EngineWorkspace = {
  organization: 'Our workspace',
  team_id: 'team',
  team_name: 'Our team',
  agent_id: 'agent',
  agent_name: 'Assistant',
  model: 'test',
  input_limit: 12000,
  agents: [],
  tasks: [],
};

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function mount() {
  const client = new LocalEngine('test');
  const snapshot = vi.spyOn(client, 'snapshot');
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: [],
    harnesses: [],
    max_steps: 1,
    max_seconds: 1,
    max_tokens: 1,
  });
  vi.spyOn(client, 'workItems').mockResolvedValue([]);
  vi.spyOn(client, 'approvals').mockResolvedValue({
    active_stops: [],
    pending_approvals: [],
    effort: [],
  });
  vi.spyOn(client, 'teams').mockResolvedValue([]);
  function Workspace() {
    const engine = useLocalEngine();
    return (
      <main tabIndex={-1} aria-label="Team workspace">
        <h1>{engine.isConnected ? 'Your work' : 'Last seen work'}</h1>
        <button onClick={engine.reconnect}>Reconnect workspace</button>
      </main>
    );
  }
  return {
    snapshot,
    render: () =>
      render(
        <LocalEngineProvider client={client}>
          <StartupGate>
            <Workspace />
          </StartupGate>
        </LocalEngineProvider>,
      ),
  };
}

it('keeps the entry screen until the actual first read settles, then reveals and focuses the workspace', async () => {
  const app = mount();
  let finish!: (value: EngineWorkspace) => void;
  app.snapshot.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  app.render();
  expect(screen.getByRole('heading', { name: 'Gathering the threads.' })).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Team workspace' })).toBeNull();
  await act(() => vi.advanceTimersByTimeAsync(8500));
  expect(screen.getByRole('heading', { name: 'Still gathering the threads.' })).toBeTruthy();
  await act(async () => finish(workspace));
  expect(screen.getByText('Your workspace is ready.')).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Team workspace' })).toBeNull();
  await act(() => vi.advanceTimersByTimeAsync(READY_HOLD_MS));
  expect(
    screen.getByRole('main', { name: 'Opening your workspace' }).getAttribute('data-phase'),
  ).toBe('departing');
  expect(screen.queryByRole('main', { name: 'Team workspace' })).toBeNull();
  await act(() => vi.advanceTimersByTimeAsync(DISPERSAL_MS));
  expect(screen.queryByRole('main', { name: 'Opening your workspace' })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole('main', { name: 'Team workspace' }));
});

it('offers a real retry after a failed first connection and recovers when the engine responds', async () => {
  const app = mount();
  app.snapshot.mockRejectedValueOnce(new Error('Open a fresh connection link.'));
  app.snapshot.mockResolvedValue(workspace);
  app.render();
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(screen.getByRole('heading', { name: 'Let’s find our way back.' })).toBeTruthy();
  expect(screen.getByText('Open a fresh connection link.')).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Team workspace' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(app.snapshot).toHaveBeenCalledTimes(2);
  await act(() => vi.advanceTimersByTimeAsync(READY_HOLD_MS + DISPERSAL_MS));
  expect(screen.getByRole('main', { name: 'Team workspace' })).toBeTruthy();
});

it('retains the workspace through later disconnection and reconnection', async () => {
  const app = mount();
  app.snapshot.mockResolvedValue(workspace);
  app.render();
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(() => vi.advanceTimersByTimeAsync(READY_HOLD_MS + DISPERSAL_MS));
  app.snapshot.mockRejectedValue(new Error('Connection lost'));
  await act(() => vi.advanceTimersByTimeAsync(1500));
  expect(screen.getByRole('heading', { name: 'Last seen work' })).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Opening your workspace' })).toBeNull();
  app.snapshot.mockResolvedValue(workspace);
  fireEvent.click(screen.getByRole('button', { name: 'Reconnect workspace' }));
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(screen.getByRole('heading', { name: 'Your work' })).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Opening your workspace' })).toBeNull();
});

it('cancels arrival if the connection fails during dispersal', async () => {
  const app = mount();
  app.snapshot.mockResolvedValue(workspace);
  app.render();
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(() => vi.advanceTimersByTimeAsync(READY_HOLD_MS));
  expect(
    screen.getByRole('main', { name: 'Opening your workspace' }).getAttribute('data-phase'),
  ).toBe('departing');
  app.snapshot.mockRejectedValue(new Error('Connection lost during arrival'));
  // Flush the failed poll separately from the later arrival deadline.
  await act(() => vi.advanceTimersByTimeAsync(300));
  await act(() => vi.advanceTimersByTimeAsync(DISPERSAL_MS));
  expect(screen.getByRole('heading', { name: 'Let’s find our way back.' })).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Team workspace' })).toBeNull();
  app.snapshot.mockResolvedValue(workspace);
  fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(
    screen.getByRole('main', { name: 'Opening your workspace' }).getAttribute('data-phase'),
  ).toBe('ready');
  await act(() => vi.advanceTimersByTimeAsync(READY_HOLD_MS + DISPERSAL_MS));
  expect(screen.getByRole('main', { name: 'Team workspace' })).toBeTruthy();
});

it('skips orbit and dispersal delays when reduced motion is requested', async () => {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  );
  const app = mount();
  app.snapshot.mockResolvedValue(workspace);
  app.render();
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(() => vi.advanceTimersByTimeAsync(1));
  expect(screen.getByRole('main', { name: 'Team workspace' })).toBeTruthy();
  expect(screen.queryByRole('main', { name: 'Opening your workspace' })).toBeNull();
});

it('uses a filled loading cloud, a bounded buzzing failure shape, and circular ready orbits', () => {
  const birds = makeFlock(860);
  const cloud = birds.map((bird) => flockTarget(bird, 6, 'connecting'));
  expect(cloud.filter((point) => Math.hypot(point.x, point.y) < 0.35).length).toBeGreaterThan(50);
  for (const bird of birds) {
    const a = flockTarget(bird, 6, 'ready');
    const b = flockTarget(bird, 9, 'ready');
    const radius = Math.hypot(a.x, a.y);
    expect(radius).toBeGreaterThan(0.99);
    expect(radius).toBeLessThan(1.09);
    expect(Math.hypot(b.x, b.y)).toBeCloseTo(radius, 10);
    expect(Math.hypot(b.x - a.x, b.y - a.y)).toBeGreaterThan(1);
    const failureA = flockTarget(bird, 6, 'unavailable');
    const failureB = flockTarget(bird, 16, 'unavailable');
    expect(Math.hypot(failureB.x - failureA.x, failureB.y - failureA.y)).toBeLessThan(0.05);
    expect(disperse(a, bird, 0, 10)).toEqual(a);
    const halfway = disperse(a, bird, 0.5, 10);
    const gone = disperse(a, bird, 1, 10);
    expect(Math.hypot(halfway.x, halfway.y)).toBeGreaterThan(radius);
    expect(Math.hypot(gone.x, gone.y)).toBeGreaterThan(10);
  }
});

it('keeps the flock still for reduced motion, pauses in hidden tabs, and releases its animation on exit', () => {
  const colors = vi
    .spyOn(window, 'getComputedStyle')
    .mockReturnValue({ color: '#faf8f3' } as CSSStyleDeclaration);
  const context = {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
  };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
    context as unknown as CanvasRenderingContext2D,
  );
  const media = { matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() };
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => media),
  );
  const disconnect = vi.fn();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = disconnect;
    },
  );
  const request = vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(42);
  const cancel = vi.spyOn(window, 'cancelAnimationFrame');
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(false);
  const view = render(<Murmuration />);
  expect(context.stroke).toHaveBeenCalled();
  expect(context).toMatchObject({ strokeStyle: '#faf8f3', globalAlpha: 1 });
  expect(request).not.toHaveBeenCalled();
  colors.mockReturnValue({ color: '#121212' } as CSSStyleDeclaration);
  fireEvent(document, new Event('tetonic:palettechange'));
  expect(context).toMatchObject({ strokeStyle: '#121212', globalAlpha: 1 });
  context.moveTo.mockClear();
  const canvas = view.container.querySelector('canvas');
  view.rerender(<Murmuration phase="ready" />);
  expect(view.container.querySelector('canvas')).toBe(canvas);
  expect(context.moveTo).toHaveBeenCalledTimes(860);
  expect(request).not.toHaveBeenCalled();
  media.matches = false;
  act(() => media.addEventListener.mock.calls[0][1]());
  expect(request).toHaveBeenCalledOnce();
  hidden.mockReturnValue(true);
  fireEvent(document, new Event('visibilitychange'));
  expect(cancel).toHaveBeenLastCalledWith(42);
  expect(request).toHaveBeenCalledOnce();
  hidden.mockReturnValue(false);
  fireEvent(document, new Event('visibilitychange'));
  expect(request).toHaveBeenCalledTimes(2);
  view.unmount();
  expect(disconnect).toHaveBeenCalledOnce();
  expect(cancel).toHaveBeenLastCalledWith(42);
  expect(media.removeEventListener).toHaveBeenCalledWith('change', expect.any(Function));
});
