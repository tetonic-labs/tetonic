import { useEffect, useMemo, useRef, useState } from 'react';
import { Entity } from '../../lib/graphMotion';
import { MotionExample, MotionPlayback } from '../../lib/motionPlayback';

export function useGraphMotion(
  entities: Entity[],
  example: MotionExample | undefined,
  scope: string,
  paused: boolean,
  held: boolean,
  reduced: boolean,
) {
  const signature = entities.map((e) => `${e.id}:${e.home.x}:${e.home.y}`).join('|');
  const exampleSignature = JSON.stringify(example);
  // Parent chat/approval renders must not restart an otherwise identical scene.
  const player = useMemo(
    () => new MotionPlayback(entities, example),
    [signature, exampleSignature, scope],
  );
  const [playing, setPlaying] = useState(false),
    [started, setStarted] = useState(false);
  const [view, setView] = useState(() => ({ frames: player.world.snapshot(), elapsed: 0 }));
  const controls = useRef({ playing, paused, held, reduced, started });
  controls.current = { playing, paused, held, reduced, started };
  useEffect(() => {
    setPlaying(false);
    setStarted(false);
    setView({ frames: player.world.snapshot(), elapsed: 0 });
  }, [player]);
  useEffect(() => {
    if (paused) setPlaying(false);
  }, [paused]);
  useEffect(() => {
    let frame = 0,
      last = 0;
    const tick = (now: number) => {
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      last = now;
      const c = controls.current;
      const ended = !!player.example && player.elapsed >= player.example.duration;
      // Freeze the entire local scene while an entity is being targeted or a
      // modal is open. Both event time and physics pause, preventing lost events.
      if (!document.hidden && !c.paused && !c.held && (!c.started || c.playing || ended)) {
        player.advance(dt, c.playing, c.reduced);
        setView({ frames: player.world.snapshot(), elapsed: player.elapsed });
        if (c.playing && player.example && player.elapsed >= player.example.duration)
          setPlaying(false);
      }
      frame = requestAnimationFrame(tick);
    };
    const hide = () => {
      last = 0;
      if (document.hidden) setPlaying(false);
    };
    document.addEventListener('visibilitychange', hide);
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener('visibilitychange', hide);
    };
  }, [player]);
  function seek(time: number) {
    player.seek(time);
    setStarted(true);
    setPlaying(false);
    setView({ frames: player.world.snapshot(), elapsed: player.elapsed });
  }
  function play() {
    if (example && player.elapsed >= example.duration) {
      player.reset();
      setView({ frames: player.world.snapshot(), elapsed: 0 });
    }
    setStarted(true);
    setPlaying(!playing);
  }
  return { ...view, playing, started, play, seek };
}
