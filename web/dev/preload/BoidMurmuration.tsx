import { useEffect, useRef } from 'react';
import { DISPERSAL_MS, type PreloadPhase } from '../../src/components/preload/flockMotion';
import { BoidFlock } from './boids';

/** Preview-only experiment. The production flock remains untouched. */
export function BoidMurmuration({ phase }: { phase: PreloadPhase }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const initialPhase = useRef(phase);
  const changePhase = useRef<((phase: PreloadPhase) => void) | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const anchor = anchorRef.current;
    if (!canvas || !anchor) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const flock = new BoidFlock(1250, initialPhase.current);
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
      const follow = 1 - Math.exp(-delta * 8);
      center.x += (destination.x - center.x) * follow;
      center.y += (destination.y - center.y) * follow;
      const scale = Math.max(1, Math.min(bounds.width / 4.9, bounds.height / 2.9));
      accumulator += delta;
      while (accumulator >= 1 / 60) {
        flock.step(1 / 60, Math.hypot(width, height) / scale);
        accumulator -= 1 / 60;
      }
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      // Lay distant ink down first, then crisp foreground stipples. Actual boid
      // depth controls perspective, grain size and translucency together.
      const layered = [...flock.birds].sort((a, b) => b.z - a.z);
      for (const bird of layered) {
        const near = Math.max(0, Math.min(1, 0.5 - bird.z * 2.1));
        const perspective = 4.8 / (4.8 + Math.max(-1, Math.min(1, bird.z)) * 1.7);
        const x = center.x + (bird.x + bird.z * 0.14) * scale * perspective;
        const y = center.y + (bird.y - bird.z * 0.08) * scale * perspective;
        const radius = Math.max(
          0.3,
          Math.min(
            2,
            bird.seed.size * Math.max(0.65, scale * 0.0065) * (0.48 + near * 1.6) * perspective,
          ),
        );
        const opacity = Math.min(
          0.84,
          (0.1 + Math.pow(near, 1.15) * 0.72) * (0.9 + Math.min(bird.density, 50) * 0.002),
        );
        const ink = `${Math.round(124 - near * 64)}, ${Math.round(104 - near * 51)}, ${Math.round(108 - near * 61)}`;

        // Seeded fine grain follows each bird without randomized frame-to-frame shimmer.
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
      if (motion.matches) flock.settle();
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

    changePhase.current = (next) => {
      if (next === flock.phase) return;
      flock.setPhase(next, motion.matches);
      syncMotion();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    observer.observe(anchor);
    resize();
    syncMotion();
    motion.addEventListener('change', syncMotion);
    document.addEventListener('visibilitychange', syncMotion);
    return () => {
      changePhase.current = null;
      window.cancelAnimationFrame(frame);
      observer.disconnect();
      motion.removeEventListener('change', syncMotion);
      document.removeEventListener('visibilitychange', syncMotion);
    };
  }, []);

  useEffect(() => {
    changePhase.current?.(phase);
  }, [phase]);

  return (
    <div ref={anchorRef} className="preload-flock" aria-hidden="true">
      <canvas ref={canvasRef} className="preload-vectors" data-phase={phase} data-flock="boids" />
    </div>
  );
}
