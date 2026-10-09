import { useEffect, useId, useRef } from 'react';
import { DISPERSAL_MS, type PreloadPhase } from '../../src/components/preload/flockMotion';
import { BoidFlock } from './boids';
import { clampAxis, dimensionalStrength, projectDimensions } from './dimensions';
import { advancePointerAxis } from './pointerMomentum';
import { sampleStateMap } from './stateMap';

/** Independent preview experiment; the two earlier renderers remain available. */
export function DimensionalMurmuration({ phase }: { phase: PreloadPhase }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const initialPhase = useRef(phase);
  const changePhase = useRef<((phase: PreloadPhase) => void) | null>(null);
  const hintId = useId();

  useEffect(() => {
    const canvas = canvasRef.current;
    const anchor = anchorRef.current;
    if (!canvas || !anchor) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const flock = new BoidFlock(1250, initialPhase.current);
    const targetView = { horizontal: 0, vertical: 0 };
    const momentum = {
      horizontal: { position: 0, velocity: 0 },
      vertical: { position: 0, velocity: 0 },
    };
    const view = { ...targetView, strength: dimensionalStrength(initialPhase.current) };
    let frame = 0,
      previous = 0,
      accumulator = 0,
      width = 0,
      height = 0;
    let center: { x: number; y: number } | undefined;

    function draw(delta = 0) {
      if (!context || !anchor) return;
      const bounds = anchor.getBoundingClientRect();
      const destination = { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 };
      if (!center || motion.matches) center = destination;
      const follow = 1 - Math.exp(-delta * 5.5);
      center.x += (destination.x - center.x) * follow;
      center.y += (destination.y - center.y) * follow;
      advancePointerAxis(momentum.horizontal, targetView.horizontal, delta);
      advancePointerAxis(momentum.vertical, targetView.vertical, delta, 4.6);
      view.horizontal = momentum.horizontal.position;
      view.vertical = momentum.vertical.position;
      const strength = dimensionalStrength(flock.phase);
      view.strength = motion.matches
        ? strength
        : view.strength + (strength - view.strength) * follow;
      const scale = Math.max(1, Math.min(bounds.width / 4.9, bounds.height / 2.9));
      accumulator += delta;
      while (accumulator >= 1 / 60) {
        flock.step(1 / 60, Math.hypot(width, height) / scale);
        accumulator -= 1 / 60;
      }
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      const state = sampleStateMap(view.horizontal, view.vertical);
      const points = flock.birds
        .map((bird) => {
          const point = projectDimensions(bird, flock.elapsed, view, state);
          const perspective = 4.8 / (4.8 + clampAxis(point.z) * 1.7);
          return {
            bird,
            ...point,
            perspective,
            projectedX: (point.x + point.z * 0.14) * perspective,
            projectedY: (point.y - point.z * 0.08) * perspective,
          };
        })
        .sort((a, b) => b.z - a.z);
      // Each region carries a complete volume footprint. Fit its blended section
      // to the page without magnifying the dots or assigning effects to axes.
      const open = (value: number) => {
        const amount = Math.max(0, Math.min(1, value * view.strength));
        return amount * amount * (3 - 2 * amount);
      };
      const spread = open(state.spread);
      const swell = open(state.fullness);
      let left = Infinity,
        right = -Infinity,
        top = Infinity,
        bottom = -Infinity;
      for (const point of points) {
        left = Math.min(left, point.projectedX);
        right = Math.max(right, point.projectedX);
        top = Math.min(top, point.projectedY);
        bottom = Math.max(bottom, point.projectedY);
      }
      const pageWidth = width * 0.92;
      const pageHeight = Math.max(100, height - 170);
      const scaleX = scale + (pageWidth / Math.max(0.1, right - left) - scale) * spread;
      const scaleY = scale + (pageHeight / Math.max(0.1, bottom - top) - scale) * swell;
      const originX = center.x + (width / 2 - center.x - ((left + right) / 2) * scaleX) * spread;
      const originY = center.y + (height / 2 - center.y - ((top + bottom) / 2) * scaleY) * swell;
      for (const point of points) {
        const { bird, z, perspective } = point;
        const near = Math.max(0, Math.min(1, 0.5 - z * 1.8));
        const x = originX + point.projectedX * scaleX;
        const y = originY + point.projectedY * scaleY;
        const radius = Math.max(
          0.8,
          Math.min(
            2.1,
            bird.seed.size * Math.max(0.65, scale * 0.0065) * (0.48 + near * 1.6) * perspective,
          ),
        );
        const depthOpacity =
          Math.min(
            0.86,
            (0.1 + Math.pow(near, 1.15) * 0.74) * (0.9 + Math.min(bird.density, 50) * 0.002),
          ) *
          (point.visibility + (1 - point.visibility) * spread * swell * 0.2);
        // Strong ink throughout the flock; depth comes primarily from size and layering.
        const opacity = 0.88 + (depthOpacity / 0.86) * 0.12;
        const ink = `${Math.round(52 - near * 24)}, ${Math.round(49 - near * 19)}, ${Math.round(47 - near * 23)}`;
        if (bird.seed.phase > Math.PI) {
          const offset = radius * 2 + 1.3;
          context.fillStyle = `rgba(${ink}, ${opacity * 0.38})`;
          context.beginPath();
          context.arc(
            x + Math.cos(bird.seed.phase) * offset,
            y + Math.sin(bird.seed.phase) * offset,
            Math.max(0.25, radius * 0.3),
            0,
            Math.PI * 2,
          );
          context.fill();
        }
        if (near > 0.72) {
          context.fillStyle = `rgba(${ink}, ${opacity * 0.075})`;
          context.beginPath();
          context.arc(x, y, radius * 1.8, 0, Math.PI * 2);
          context.fill();
        }
        context.fillStyle = `rgba(${ink}, ${opacity})`;
        context.beginPath();
        context.arc(x, y, radius, 0, Math.PI * 2);
        context.fill();
      }
    }

    function animate(now: number) {
      const delta = previous ? Math.min((now - previous) / 1000, 0.05) : 0;
      previous = now;
      draw(delta);
      if (flock.phase !== 'departing' || flock.phaseElapsed < DISPERSAL_MS / 1000) {
        frame = window.requestAnimationFrame(animate);
      }
    }

    function syncMotion() {
      window.cancelAnimationFrame(frame);
      previous = 0;
      accumulator = 0;
      if (motion.matches) {
        flock.settle();
        targetView.horizontal = targetView.vertical = view.horizontal = view.vertical = 0;
        momentum.horizontal.position = momentum.horizontal.velocity = 0;
        momentum.vertical.position = momentum.vertical.velocity = 0;
      }
      draw();
      if (!motion.matches && !document.hidden) frame = window.requestAnimationFrame(animate);
    }

    function resize() {
      if (!canvas) return;
      const bounds = canvas.getBoundingClientRect();
      width = bounds.width;
      height = bounds.height;
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      draw();
    }

    function movePointer(event: PointerEvent) {
      if (motion.matches || flock.phase === 'departing') return;
      if (
        event.target instanceof Element &&
        event.target.closest('button, select, input, a, summary')
      )
        return;
      if (event.pointerType === 'touch' && !anchor?.contains(event.target as Node)) return;
      targetView.horizontal = clampAxis((event.clientX / Math.max(1, width)) * 2 - 1);
      targetView.vertical = clampAxis((event.clientY / Math.max(1, height)) * 2 - 1);
    }

    function resetView() {
      targetView.horizontal = targetView.vertical = 0;
    }
    function leavePointer(event: PointerEvent) {
      if (!event.relatedTarget) resetView();
    }
    function moveKey(event: KeyboardEvent) {
      if (motion.matches || flock.phase === 'departing') return;
      if (event.key === 'Home') resetView();
      else if (event.key === 'ArrowLeft')
        targetView.horizontal = clampAxis(targetView.horizontal - 0.2);
      else if (event.key === 'ArrowRight')
        targetView.horizontal = clampAxis(targetView.horizontal + 0.2);
      else if (event.key === 'ArrowUp') targetView.vertical = clampAxis(targetView.vertical - 0.2);
      else if (event.key === 'ArrowDown')
        targetView.vertical = clampAxis(targetView.vertical + 0.2);
      else return;
      event.preventDefault();
    }

    changePhase.current = (next) => {
      if (next === flock.phase) return;
      // A preview replay starts a fresh arrival after the particles have left
      // the viewport. Ordinary stage transitions still retain their momentum.
      flock.setPhase(next, motion.matches || flock.phase === 'departing');
      syncMotion();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    observer.observe(anchor);
    resize();
    syncMotion();
    motion.addEventListener('change', syncMotion);
    document.addEventListener('visibilitychange', syncMotion);
    window.addEventListener('pointermove', movePointer, { passive: true });
    window.addEventListener('pointerout', leavePointer);
    window.addEventListener('blur', resetView);
    anchor.addEventListener('keydown', moveKey);
    return () => {
      changePhase.current = null;
      window.cancelAnimationFrame(frame);
      observer.disconnect();
      motion.removeEventListener('change', syncMotion);
      document.removeEventListener('visibilitychange', syncMotion);
      window.removeEventListener('pointermove', movePointer);
      window.removeEventListener('pointerout', leavePointer);
      window.removeEventListener('blur', resetView);
      anchor.removeEventListener('keydown', moveKey);
    };
  }, []);

  useEffect(() => {
    changePhase.current?.(phase);
  }, [phase]);

  return (
    <div
      ref={anchorRef}
      className="preload-flock dimensional-flock"
      tabIndex={0}
      role="group"
      aria-label="Explore flock dimensions"
      aria-describedby={hintId}
    >
      <canvas
        ref={canvasRef}
        className="preload-vectors"
        data-phase={phase}
        data-flock="5d"
        aria-hidden="true"
      />
      <p id={hintId} className="dimensional-hint">
        <span className="dimensional-motion-hint">
          Explore the field · each place reveals a different flock
        </span>
        <span className="dimensional-still-hint">A still view through five dimensions</span>
        <span className="dimensional-key-hint">Arrow keys explore · Home recenters</span>
      </p>
    </div>
  );
}
