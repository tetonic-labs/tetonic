import type { MotionExample } from './motionPlayback';
import type { Interaction, Outcome } from './graphMotion';
import type { Point } from './mapActivity';

export interface WorkState {
  id: string;
  interaction?: Interaction;
  started: number;
  waiting: boolean;
  previous?: { interaction: Interaction; at: number; outcome: Outcome };
  failure?: WorkFailure;
  failures: WorkFailure[];
}
export interface WorkFailure {
  interaction: Interaction;
  at: number;
  retryId?: string;
}
export function workScene(example: MotionExample | undefined, elapsed: number) {
  const states = new Map<string, WorkState>();
  for (const event of example?.events || []) {
    if (event.at > elapsed) break;
    const id = event.type === 'start' ? event.interaction.agentId : event.agentId;
    const state = states.get(id) || { id, started: 0, waiting: false, failures: [] };
    if (event.type === 'start') {
      const interruptedRetry = state.failures.find((f) => f.retryId === state.interaction?.id);
      if (interruptedRetry) interruptedRetry.retryId = undefined;
      if (state.interaction)
        state.previous = { interaction: state.interaction, at: event.at, outcome: 'cancelled' };
      state.interaction = event.interaction;
      state.started = event.at;
      state.waiting = false;
      const retry = state.failures.find((f) => f.interaction.id === event.interaction.retryOf);
      if (retry) retry.retryId = event.interaction.id;
    } else if (state.interaction?.id === event.interactionId) {
      if (event.type === 'end') {
        state.previous = { interaction: state.interaction, at: event.at, outcome: event.outcome };
        const retryOf = state.interaction.retryOf;
        const original = state.failures.find((f) => f.interaction.id === retryOf);
        if (original) original.retryId = undefined;
        if (event.outcome === 'failed' && !original)
          state.failures.push({ interaction: state.interaction, at: event.at });
        // Only an explicitly correlated, successful retry resolves the original failure.
        if (event.outcome === 'completed' && retryOf)
          state.failures = state.failures.filter((f) => f.interaction.id !== retryOf);
        state.interaction = undefined;
        state.waiting = false;
      } else state.waiting = event.type === 'wait';
    }
    state.failure = state.failures[0];
    states.set(id, state);
  }
  return states;
}

export function markerPoint(from: Point, to: Point, progress: number): Point {
  const t = Math.min(1, Math.max(0, progress));
  const eased = t * t * (3 - 2 * t);
  return {
    x: from.x + (to.x - from.x) * eased,
    y: from.y + (to.y - from.y) * eased - Math.sin(Math.PI * eased) * 34,
  };
}

// Continuous semantic zoom: neither layout nor identity changes at a threshold.
export const detailLevel = (scale: number) => Math.max(0, Math.min(1, (scale - 0.28) / 0.4));
