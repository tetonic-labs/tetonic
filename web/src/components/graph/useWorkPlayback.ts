import { useEffect, useMemo, useRef, useState } from 'react';
import type { MotionExample } from '../../lib/motionPlayback';
import { workScene } from '../../lib/workScene';

export function useWorkPlayback(example: MotionExample | undefined, paused: boolean) {
  const [elapsed, setElapsed] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [started, setStarted] = useState(false);
  const duration = example?.duration || 0;
  const controls = useRef({ playing, paused, duration });
  controls.current = { playing, paused, duration };
  useEffect(() => {
    setElapsed(0);
    setPlaying(false);
    setStarted(false);
  }, [example?.id]);
  useEffect(() => {
    let frame = 0,
      previous = 0,
      pending = 0;
    const tick = (now: number) => {
      const dt = previous ? Math.min(0.1, (now - previous) / 1000) : 0;
      previous = now;
      const c = controls.current;
      if (c.playing && !c.paused && !document.hidden) {
        pending += dt;
        if (pending >= 1 / 30) {
          const step = pending;
          pending = 0;
          setElapsed((value) => Math.min(c.duration, value + step));
        }
      } else pending = 0;
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);
  useEffect(() => {
    if (started && elapsed >= duration) setPlaying(false);
  }, [elapsed, duration, started]);
  const states = useMemo(
    () => workScene(started ? example : undefined, elapsed),
    [example, elapsed, started],
  );
  return {
    elapsed,
    playing,
    started,
    states,
    play: () => {
      if (elapsed >= duration) setElapsed(0);
      setStarted(true);
      setPlaying((v) => !v);
    },
    seek: (time: number) => {
      setElapsed(Math.max(0, Math.min(duration, time)));
      setStarted(true);
      setPlaying(false);
    },
  };
}
