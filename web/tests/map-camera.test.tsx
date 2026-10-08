import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { useMapCamera } from '../src/components/graph/useMapCamera';

let camera: ReturnType<typeof useMapCamera>;
let resize: (entries: { contentRect: { width: number; height: number } }[]) => void;
function Harness({ world = { width: 2000, height: 1500 }, scope = 'all' }) {
  camera = useMapCamera(scope, true, world);
  return (
    <div ref={camera.viewport}>
      <div className="universe-canvas" data-testid="map" tabIndex={0} onKeyDown={camera.onKeyDown}>
        <button>Portrait</button>
      </div>
    </div>
  );
}
const anchor = (point = { x: 400, y: 300 }) => ({
  x: (point.x - camera.offset.x) / camera.scale,
  y: (point.y - camera.offset.y) / camera.scale,
});
beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: typeof resize) {
        resize = callback;
      }
      observe() {
        resize([{ contentRect: { width: 1000, height: 800 } }]);
      }
      disconnect() {}
    },
  );
});
afterEach(() => vi.unstubAllGlobals());
describe('continuous map camera', () => {
  it('returns to the original map after inspecting several assignments in the same panel', () => {
    render(<Harness />);
    const start = { scale: camera.scale, offset: { ...camera.offset } };
    act(() =>
      camera.focus(
        { x: 400, y: 300 },
        { width: 500, height: 400 },
        { right: 420 },
        'inspector',
        true,
      ),
    );
    act(() =>
      camera.focus(
        { x: 900, y: 600 },
        { width: 500, height: 400 },
        { right: 420 },
        'inspector',
        true,
      ),
    );
    act(() => camera.back());
    expect(camera.scale).toBeCloseTo(start.scale, 6);
    expect(camera.offset.x).toBeCloseTo(start.offset.x, 6);
    expect(camera.offset.y).toBeCloseTo(start.offset.y, 6);
    expect(camera.canGoBack).toBe(false);
  });
  it('frames a team clear of the resource shelf on wide and narrow viewports', () => {
    render(<Harness />);
    const point = { x: 700, y: 600 },
      bounds = { width: 700, height: 600 };
    act(() => camera.focus(point, bounds, { right: 290 }));
    expect(point.x * camera.scale + camera.offset.x).toBeCloseTo((1000 - 290) / 2, 6);
    expect((point.x + bounds.width / 2) * camera.scale + camera.offset.x).toBeLessThanOrEqual(
      1000 - 290,
    );
    act(() => resize([{ contentRect: { width: 390, height: 844 } }]));
    act(() => camera.focus(point, bounds, { bottom: 142 }));
    expect(point.x * camera.scale + camera.offset.x).toBeCloseTo(195, 6);
    expect((point.y + bounds.height / 2) * camera.scale + camera.offset.y).toBeLessThanOrEqual(
      844 - 250 - 142,
    );
  });
  it('anchors wheel zoom at the pointer even over a portrait and preserves exact view history', () => {
    render(<Harness />);
    const start = anchor(),
      scale = camera.scale;
    fireEvent.wheel(screen.getByRole('button', { name: 'Portrait' }), {
      deltaY: -100,
      clientX: 400,
      clientY: 300,
    });
    expect(camera.scale).toBeGreaterThan(scale);
    expect(anchor().x).toBeCloseTo(start.x, 6);
    expect(anchor().y).toBeCloseTo(start.y, 6);
    const prior = { scale: camera.scale, offset: camera.offset };
    act(() => camera.focus({ x: 200, y: 200 }, { width: 500, height: 400 }));
    expect(camera.canGoBack).toBe(true);
    act(() => camera.back());
    expect(camera.scale).toBe(prior.scale);
    expect(camera.offset).toEqual(prior.offset);
  });
  it('keeps the world point at viewport center and absolute scale through resize and layout growth', () => {
    const view = render(<Harness />);
    act(() => camera.zoomBy(2));
    const before = anchor({ x: 500, y: 400 }),
      scale = camera.scale;
    act(() => resize([{ contentRect: { width: 600, height: 700 } }]));
    expect(camera.scale).toBeCloseTo(scale, 8);
    expect(anchor({ x: 300, y: 350 }).x).toBeCloseTo(before.x, 6);
    expect(anchor({ x: 300, y: 350 }).y).toBeCloseTo(before.y, 6);
    view.rerender(<Harness world={{ width: 2600, height: 2200 }} />);
    expect(camera.scale).toBeCloseTo(scale, 8);
    expect(anchor({ x: 300, y: 350 }).x).toBeCloseTo(before.x, 6);
    expect(anchor({ x: 300, y: 350 }).y).toBeCloseTo(before.y, 6);
  });
  it('supports keyboard pan, zoom, overview and previous view without moving focused inputs', () => {
    render(<Harness />);
    const el = screen.getByTestId('map'),
      scale = camera.scale;
    fireEvent.keyDown(el, { key: '+' });
    expect(camera.scale).toBeGreaterThan(scale);
    const x = camera.offset.x;
    fireEvent.keyDown(el, { key: 'ArrowRight' });
    expect(camera.offset.x).toBe(x - 65);
    const prior = camera.scale;
    fireEvent.keyDown(el, { key: 'Home' });
    expect(camera.scale).toBe(scale);
    fireEvent.keyDown(el, { key: 'ArrowLeft', altKey: true });
    expect(camera.scale).toBe(prior);
    fireEvent.keyDown(screen.getByRole('button'), { key: '-' });
    expect(camera.scale).toBe(prior);
  });
  it('continues one-finger panning after a pinch ends and suppresses the accidental activation', () => {
    render(<Harness />);
    const el = screen.getByTestId('map');
    el.setPointerCapture = vi.fn();
    el.hasPointerCapture = () => false;
    const event = (id: number, x: number, y: number) =>
      ({
        pointerId: id,
        pointerType: 'touch',
        button: 0,
        clientX: x,
        clientY: y,
        currentTarget: el,
        target: el,
        preventDefault: vi.fn(),
      }) as unknown as ReactPointerEvent;
    const scale = camera.scale;
    act(() => {
      camera.onPointerDown(event(1, 300, 300));
      camera.onPointerDown(event(2, 500, 300));
    });
    act(() => camera.onPointerMove(event(2, 700, 300)));
    expect(camera.scale).toBeCloseTo(scale * 2, 6);
    act(() => camera.onPointerUp(event(2, 700, 300)));
    act(() => camera.onPointerCancel(event(2, 700, 300)));
    const offset = camera.offset.x;
    act(() => camera.onPointerMove(event(1, 320, 300)));
    expect(camera.offset.x).toBeCloseTo(offset + 20, 6);
    const click = { preventDefault: vi.fn(), stopPropagation: vi.fn() };
    camera.onClickCapture(click as never);
    expect(click.preventDefault).toHaveBeenCalledOnce();
  });
  it('clears map text selection and permits repeated drags without stealing normal button clicks', () => {
    render(<Harness />);
    const el = screen.getByTestId('map');
    const button = screen.getByRole('button', { name: 'Portrait' });
    el.setPointerCapture = vi.fn();
    el.hasPointerCapture = () => false;
    const event = (x: number, target: Element = el) =>
      ({
        pointerId: 1,
        pointerType: 'mouse',
        button: 0,
        clientX: x,
        clientY: 300,
        currentTarget: el,
        target,
        preventDefault: vi.fn(),
      }) as unknown as ReactPointerEvent;
    const range = document.createRange();
    range.selectNodeContents(button);
    window.getSelection()!.addRange(range);
    expect(window.getSelection()!.toString()).toBe('Portrait');
    const start = camera.offset.x;
    for (const target of [el, button]) {
      const down = event(300, target);
      act(() => camera.onPointerDown(down));
      expect(window.getSelection()!.toString()).toBe('');
      act(() => camera.onPointerMove(event(350, target)));
      act(() => camera.onPointerUp(event(350, target)));
      const click = { preventDefault: vi.fn(), stopPropagation: vi.fn() };
      camera.onClickCapture(click as never);
      expect(click.preventDefault).toHaveBeenCalledOnce();
    }
    expect(camera.offset.x).toBeCloseTo(start + 100, 6);
    const down = event(300, button);
    act(() => camera.onPointerDown(down));
    act(() => camera.onPointerMove(event(302, button)));
    act(() => camera.onPointerUp(event(302, button)));
    const click = { preventDefault: vi.fn(), stopPropagation: vi.fn() };
    camera.onClickCapture(click as never);
    expect(down.preventDefault).not.toHaveBeenCalled();
    expect(click.preventDefault).not.toHaveBeenCalled();
    expect(camera.offset.x).toBeCloseTo(start + 100, 6);
  });
});

it('restores the right framing when moving between differently sized project maps', () => {
  const view = render(<Harness />);
  act(() => camera.zoomBy(1.3));
  const original = { scale: camera.scale, offset: { ...camera.offset } };
  view.rerender(<Harness scope="project" world={{ width: 1200, height: 500 }} />);
  const worldCenter = {
    x: 600 * camera.scale + camera.offset.x,
    y: 250 * camera.scale + camera.offset.y,
  };
  expect(worldCenter.x).toBeCloseTo(500, 6);
  expect(worldCenter.y).toBeCloseTo(395, 6);
  view.rerender(<Harness />);
  expect(camera.scale).toBeCloseTo(original.scale, 6);
  expect(camera.offset.x).toBeCloseTo(original.offset.x, 6);
  expect(camera.offset.y).toBeCloseTo(original.offset.y, 6);
});
