import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent, PointerEvent, MouseEvent } from 'react';
import { Point, WORLD } from '../../lib/mapActivity';
interface Camera {
  zoom: number;
  pan: Point;
  context?: string;
}
interface FocusSpace {
  right?: number;
  bottom?: number;
}
const initial = (): Camera => ({ zoom: 1, pan: { x: 0, y: 0 } });
export function useMapCamera(
  scope: string,
  reduced: boolean,
  world = WORLD,
  scopeBounds?: { x: number; y: number; width: number; height: number },
) {
  const views = useRef(new Map<string, Camera>());
  const previousScope = useRef(scope);
  const retainNextScope = useRef(false);
  const history = useRef<Camera[]>([]);
  const [canGoBack, setCanGoBack] = useState(false);
  const dragged = useRef(false);
  const viewport = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 1440, height: 900 }),
    [measured, setMeasured] = useState(false),
    [camera, setCamera] = useState(initial);
  const shown = useRef(camera),
    target = useRef(camera),
    frame = useRef(0);
  const pointers = useRef(new Map<number, Point>()),
    gesture = useRef<{ center: Point; distance: number; camera: Camera } | null>(null);
  // Team headings sit above their neighborhood; reserve space below the header.
  const topInset = size.width < 700 ? 170 : 140;
  const bottomInset = size.width < 700 ? 250 : 150;
  const availableHeight = Math.max(80, size.height - topInset - bottomInset);
  const centerY = (topInset + size.height - bottomInset) / 2;
  const base = Math.min(size.width / world.width, availableHeight / world.height);
  const geometry = (c: Camera) => {
    const scale = Math.max(0.01, base * c.zoom);
    return {
      scale,
      offset: {
        x: (size.width - world.width * scale) / 2 + c.pan.x,
        y: centerY - (world.height * scale) / 2 + c.pan.y,
      },
    };
  };
  const { scale, offset } = geometry(camera);
  function move(next: Camera, immediate = false) {
    target.current = next;
    if (immediate || reduced) {
      cancelAnimationFrame(frame.current);
      frame.current = 0;
      shown.current = next;
      setCamera(next);
      return;
    }
    if (frame.current) return;
    let last = 0;
    const tick = (now: number) => {
      const dt = last ? Math.min(0.05, (now - last) / 1000) : 1 / 60;
      last = now;
      const a = 1 - Math.exp(-17 * dt),
        from = shown.current,
        to = target.current;
      const next = {
        context: to.context || from.context,
        zoom: from.zoom + (to.zoom - from.zoom) * a,
        pan: {
          x: from.pan.x + (to.pan.x - from.pan.x) * a,
          y: from.pan.y + (to.pan.y - from.pan.y) * a,
        },
      };
      const done =
        Math.abs(to.zoom - next.zoom) < 0.001 &&
        Math.hypot(to.pan.x - next.pan.x, to.pan.y - next.pan.y) < 0.1;
      shown.current = done ? to : next;
      setCamera(shown.current);
      frame.current = done ? 0 : requestAnimationFrame(tick);
    };
    frame.current = requestAnimationFrame(tick);
  }
  function zoomBy(factor: number, point = { x: size.width / 2, y: size.height / 2 }) {
    const old = target.current,
      g = geometry(old),
      zoom = Math.max(0.55, Math.min(Math.max(30, 2.2 / base), old.zoom * factor)),
      nextScale = Math.max(0.01, base * zoom);
    const anchor = { x: (point.x - g.offset.x) / g.scale, y: (point.y - g.offset.y) / g.scale };
    move({
      ...old,
      zoom,
      pan: {
        x: point.x - anchor.x * nextScale - (size.width - world.width * nextScale) / 2,
        y: point.y - anchor.y * nextScale - centerY + (world.height * nextScale) / 2,
      },
    });
  }
  useEffect(() => {
    const el = viewport.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(([entry]) => {
      // The map stays mounted while the operator visits Work. Ignore hidden sizes.
      if (!entry.contentRect.width || !entry.contentRect.height) return;
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
      setMeasured(true);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const retain = previousScope.current !== scope && retainNextScope.current;
    retainNextScope.current = false;
    if (previousScope.current !== scope) views.current.set(previousScope.current, target.current);
    previousScope.current = scope;
    if (retain) return;
    move(views.current.get(scope) || overview(), true);
    history.current = [];
    setCanGoBack(false);
    pointers.current.clear();
    gesture.current = null;
  }, [scope, measured]);
  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      // Zoom over map entities too, but leave inspection and playback controls alone.
      if (!(e.target as Element).closest('.universe-canvas')) return;
      e.preventDefault();
      const r = el.getBoundingClientRect();
      const delta = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? size.height : 1);
      zoomBy(Math.exp(-Math.max(-180, Math.min(180, delta)) * (e.ctrlKey ? 0.007 : 0.002)), {
        x: e.clientX - r.left,
        y: e.clientY - r.top,
      });
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  }, [base, size, reduced, world.width, world.height]);
  function begin() {
    const p = [...pointers.current.values()];
    if (!p.length) {
      gesture.current = null;
      return;
    }
    const center = p.length === 1 ? p[0] : { x: (p[0].x + p[1].x) / 2, y: (p[0].y + p[1].y) / 2 };
    gesture.current = {
      center,
      distance: p.length === 1 ? 0 : Math.hypot(p[1].x - p[0].x, p[1].y - p[0].y),
      camera: shown.current,
    };
  }
  function local(e: PointerEvent) {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }
  function focusScale(bounds?: { width: number; height: number }, space: FocusSpace = {}) {
    return bounds
      ? Math.min(
          1.05,
          Math.max(120, size.width - 100 - (space.right || 0)) / bounds.width,
          Math.max(80, availableHeight - (space.bottom || 0)) / bounds.height,
        )
      : 1.05;
  }
  function framed(
    point: Point,
    bounds?: { width: number; height: number },
    space: FocusSpace = {},
  ): Camera {
    const scale = focusScale(bounds, space);
    const zoom = scale / Math.max(0.01, base);
    const nextScale = Math.max(0.01, base * zoom);
    return {
      zoom,
      pan: {
        x: (world.width / 2 - point.x) * nextScale - (space.right || 0) / 2,
        y: (world.height / 2 - point.y) * nextScale - (space.bottom || 0) / 2,
      },
    };
  }
  function overview() {
    return scopeBounds
      ? framed(
          { x: scopeBounds.x + scopeBounds.width / 2, y: scopeBounds.y + scopeBounds.height / 2 },
          scopeBounds,
        )
      : initial();
  }
  function remember() {
    history.current = [...history.current, target.current].slice(-12);
    setCanGoBack(true);
  }
  function back() {
    const previous = history.current.pop();
    if (previous) move(previous);
    setCanGoBack(history.current.length > 0);
  }
  function fit() {
    remember();
    move(overview());
  }
  // Layout growth and viewport changes preserve the same world point and scale.
  const previousGeometry = useRef({ base, size, world, centerY, measured });
  useEffect(() => {
    const before = previousGeometry.current;
    previousGeometry.current = { base, size, world, centerY, measured };
    if (
      !before.measured ||
      !measured ||
      (before.base === base &&
        before.size === size &&
        before.world.width === world.width &&
        before.world.height === world.height)
    )
      return;
    const remap = (c: Camera): Camera => {
      const oldScale = before.base * c.zoom;
      return {
        ...c,
        zoom: oldScale / base,
        pan: {
          x: c.pan.x + ((world.width - before.world.width) * oldScale) / 2,
          y:
            c.pan.y +
            ((world.height - before.world.height) * oldScale) / 2 +
            before.centerY -
            centerY +
            (size.height - before.size.height) / 2,
        },
      };
    };
    history.current = history.current.map(remap);
    views.current.forEach((c, id) => views.current.set(id, remap(c)));
    move(remap(shown.current), true);
  }, [base, size, world.width, world.height]);
  return {
    viewport,
    scale,
    overviewScale: geometry(overview()).scale,
    organizationScale: base,
    context: camera.context,
    preserveNextScopeChange: () => {
      retainNextScope.current = true;
    },
    offset,
    nodeScale: 1,
    size,
    canGoBack,
    focusScale,
    back,
    focus: (
      point: Point,
      bounds?: { width: number; height: number },
      space?: FocusSpace,
      context?: string,
    ) => {
      remember();
      move({ ...framed(point, bounds, space), context });
    },
    zoomBy,
    fit,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.target !== e.currentTarget) return;
      if (e.altKey && e.key === 'ArrowLeft') {
        e.preventDefault();
        back();
        return;
      }
      const moves: Record<string, Point> = {
        ArrowLeft: { x: 65, y: 0 },
        ArrowRight: { x: -65, y: 0 },
        ArrowUp: { x: 0, y: 65 },
        ArrowDown: { x: 0, y: -65 },
      };
      if (moves[e.key]) {
        e.preventDefault();
        const c = target.current;
        move({ ...c, pan: { x: c.pan.x + moves[e.key].x, y: c.pan.y + moves[e.key].y } });
      }
      if (['+', '=', '-', 'Home'].includes(e.key)) {
        e.preventDefault();
        if (e.key === 'Home') fit();
        else zoomBy(e.key === '-' ? 1 / 1.2 : 1.2);
      }
    },
    onPointerDown: (e: PointerEvent) => {
      // A fresh press must never inherit click suppression from a cancelled drag.
      if (!pointers.current.size) dragged.current = false;
      if (
        e.button !== 0 ||
        (e.target as Element).closest('input, textarea, select, [contenteditable="true"]')
      )
        return;
      const onControl = !!(e.target as Element).closest('button, a');
      if (!onControl) e.preventDefault();
      const selection = window.getSelection();
      if (selection?.anchorNode && e.currentTarget.contains(selection.anchorNode))
        selection.removeAllRanges();
      if (!onControl) (e.currentTarget as HTMLElement).focus({ preventScroll: true });
      move(shown.current, true);
      pointers.current.set(e.pointerId, local(e));
      begin();
      if (!onControl) e.currentTarget.setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: PointerEvent) => {
      if (!pointers.current.has(e.pointerId) || !gesture.current) return;
      pointers.current.set(e.pointerId, local(e));
      const p = [...pointers.current.values()],
        g = gesture.current,
        center = p.length === 1 ? p[0] : { x: (p[0].x + p[1].x) / 2, y: (p[0].y + p[1].y) / 2 };
      if (p.length > 1 || Math.hypot(center.x - g.center.x, center.y - g.center.y) > 4)
        dragged.current = true;
      if (!dragged.current) return;
      e.preventDefault();
      if (!e.currentTarget.hasPointerCapture?.(e.pointerId))
        e.currentTarget.setPointerCapture(e.pointerId);
      const zoom =
        p.length > 1 && g.distance
          ? Math.max(
              0.55,
              Math.min(
                Math.max(30, 2.2 / base),
                (g.camera.zoom * Math.hypot(p[1].x - p[0].x, p[1].y - p[0].y)) / g.distance,
              ),
            )
          : g.camera.zoom;
      const before = geometry(g.camera),
        nextScale = Math.max(0.01, base * zoom);
      const anchor = {
        x: (g.center.x - before.offset.x) / before.scale,
        y: (g.center.y - before.offset.y) / before.scale,
      };
      move(
        {
          ...g.camera,
          zoom,
          pan: {
            x: center.x - anchor.x * nextScale - (size.width - world.width * nextScale) / 2,
            y: center.y - anchor.y * nextScale - centerY + (world.height * nextScale) / 2,
          },
        },
        true,
      );
    },
    onPointerUp: (e: PointerEvent) => {
      pointers.current.delete(e.pointerId);
      if (e.currentTarget.hasPointerCapture?.(e.pointerId))
        e.currentTarget.releasePointerCapture(e.pointerId);
      begin();
    },
    onPointerCancel: (e: PointerEvent) => {
      pointers.current.delete(e.pointerId);
      begin();
    },
    onClickCapture: (e: MouseEvent) => {
      if (dragged.current) {
        e.preventDefault();
        e.stopPropagation();
        dragged.current = false;
      }
    },
  };
}
