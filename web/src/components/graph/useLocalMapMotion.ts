import { useEffect, useRef, useState } from 'react';
import type { Entity, MotionFrame } from '../../lib/graphMotion';
import { LocalMapMotion } from '../../lib/localMapMotion';
import type { OrganizationActivity } from './useOrganizationActivity';

export function useLocalMapMotion(
  entities: Entity[],
  activity: OrganizationActivity,
  reduced: boolean,
  visible: boolean,
) {
  const driver = useRef(new LocalMapMotion());
  const [frames, setFrames] = useState<MotionFrame[]>([]);
  const controls = useRef({ activity, reduced });
  const previousTime = useRef(activity.elapsed);
  controls.current = { activity, reduced };
  useEffect(() => {
    driver.current.sync(entities, activity.states, activity.elapsed, activity.example?.id || '');
    const sought = !activity.playing && activity.elapsed !== previousTime.current;
    previousTime.current = activity.elapsed;
    if (reduced || sought) setFrames(driver.current.advance(0, true));
    else if (!activity.playing) setFrames(driver.current.world.snapshot());
  }, [entities, activity.states, activity.example?.id, reduced, activity.playing]);
  useEffect(() => {
    if (!visible) return;
    let frame = 0,
      previous = 0;
    const tick = (now: number) => {
      const dt = previous ? Math.min(0.05, (now - previous) / 1000) : 0;
      previous = now;
      const { activity: a, reduced: r } = controls.current;
      if (!document.hidden && a.playing && !a.reading && !r && driver.current.world.bodies.size)
        setFrames(driver.current.advance(dt, false));
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [visible]);
  return frames;
}
