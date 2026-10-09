import { useEffect, useRef } from 'react';
import {
  DISPERSAL_MS,
  MORPH_SECONDS,
  disperse,
  flockTarget,
  makeFlock,
  type FlockPoint,
  type PreloadPhase,
} from './flockMotion';

export function Murmuration({ phase = 'connecting' }: { phase?: PreloadPhase }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const initialPhase = useRef(phase);
  const changePhase = useRef<((next: PreloadPhase) => void) | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const anchor = anchorRef.current;
    if (!canvas || !anchor) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    let ink = getComputedStyle(canvas).color;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let activePhase = initialPhase.current;
    const particles = makeFlock(860).map((bird) => {
      const point = flockTarget(bird, 6, activePhase === 'departing' ? 'ready' : activePhase);
      return { bird, point, source: point, angle: bird.orbit + Math.PI / 2 };
    });
    let width = 0;
    let height = 0;
    let frame = 0;
    let elapsed = 0;
    let stageStarted = activePhase === 'departing' ? 0 : -MORPH_SECONDS;
    let previous = 0;
    let center: { x: number; y: number } | undefined;

    function draw(delta = 0) {
      if (!context || !canvas || !anchor) return;
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      const bounds = anchor.getBoundingClientRect();
      const destination = { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 };
      if (!center || motion.matches) center = destination;
      const follow = 1 - Math.exp(-delta * 8);
      center.x += (destination.x - center.x) * follow;
      center.y += (destination.y - center.y) * follow;
      const scale = Math.max(1, Math.min(bounds.width / 4.9, bounds.height / 2.9));
      const time = elapsed + 6;
      const progress = Math.max(0, Math.min(1, (elapsed - stageStarted) / MORPH_SECONDS));
      const blend = motion.matches ? 1 : (1 - Math.cos(progress * Math.PI)) / 2;
      const departure = Math.max(0, (elapsed - stageStarted) / (DISPERSAL_MS / 1000));
      const reach = Math.hypot(width, height) / scale;

      for (const particle of particles) {
        const { bird, source, point: old } = particle;
        let point: FlockPoint;
        let next: FlockPoint;
        if (activePhase === 'departing') {
          point = disperse(source, bird, motion.matches ? 1 : departure, reach);
          next = disperse(source, bird, departure + 0.01, reach);
        } else {
          const target = flockTarget(bird, time, activePhase);
          next = flockTarget(bird, time + 0.045, activePhase);
          point = {
            x: source.x + (target.x - source.x) * blend,
            y: source.y + (target.y - source.y) * blend,
            depth: source.depth + (target.depth - source.depth) * blend,
            perspective: source.perspective + (target.perspective - source.perspective) * blend,
          };
        }
        const dx = delta > 0 ? point.x - old.x : next.x - point.x;
        const dy = delta > 0 ? point.y - old.y : next.y - point.y;
        if (Math.abs(dx) + Math.abs(dy) > 0.00001) particle.angle = Math.atan2(dy, dx);
        particle.point = point;
      }

      const sorted = [...particles].sort((a, b) => b.point.depth - a.point.depth);
      context.lineCap = 'round';
      for (const { bird, point, angle } of sorted) {
        const stretch = activePhase === 'departing' ? 1 + Math.min(departure, 1) * 2.5 : 1;
        const length = bird.size * point.perspective * Math.max(1.7, scale * 0.025) * stretch;
        const x = center.x + point.x * scale;
        const y = center.y + point.y * scale;
        const opacity = Math.max(0.2, Math.min(0.72, 0.46 - point.depth * 0.2));
        context.strokeStyle = ink;
        context.globalAlpha = opacity;
        context.lineWidth = Math.max(0.7, bird.size * point.perspective * 0.9);
        context.beginPath();
        context.moveTo(x, y);
        context.lineTo(x + Math.cos(angle) * length, y + Math.sin(angle) * length);
        context.stroke();
      }
      context.globalAlpha = 1;
    }

    function syncColors() {
      if (canvas) ink = getComputedStyle(canvas).color;
      draw();
    }

    function animate(now: number) {
      const delta = previous ? Math.min((now - previous) / 1000, 0.05) : 0;
      elapsed += delta;
      previous = now;
      draw(delta);
      frame = window.requestAnimationFrame(animate);
    }

    function syncMotion() {
      window.cancelAnimationFrame(frame);
      previous = 0;
      if (!motion.matches && !document.hidden) frame = window.requestAnimationFrame(animate);
      else draw();
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
      if (next === activePhase) return;
      for (const particle of particles) particle.source = { ...particle.point };
      activePhase = next;
      stageStarted = elapsed;
      draw();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    observer.observe(anchor);
    resize();
    syncMotion();
    motion.addEventListener('change', syncMotion);
    document.addEventListener('visibilitychange', syncMotion);
    document.addEventListener('tetonic:palettechange', syncColors);
    return () => {
      changePhase.current = null;
      window.cancelAnimationFrame(frame);
      observer.disconnect();
      motion.removeEventListener('change', syncMotion);
      document.removeEventListener('visibilitychange', syncMotion);
      document.removeEventListener('tetonic:palettechange', syncColors);
    };
  }, []);

  useEffect(() => {
    changePhase.current?.(phase);
  }, [phase]);

  return (
    <div ref={anchorRef} className="preload-flock" aria-hidden="true">
      <canvas ref={canvasRef} className="preload-vectors" data-phase={phase} />
    </div>
  );
}
