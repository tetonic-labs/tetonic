import type { EngineTask, EngineTaskState } from '../contracts';

export const engineStates: Record<EngineTaskState, string> = {
  waiting_human: 'Needs your input',
  not_started: 'Not started',
  starting: 'Starting',
  running: 'Working',
  canceling: 'Stopping',
  canceled: 'Stopped',
  failed: 'Failed',
  completed: 'Completed',
  recovery_required: 'Interrupted · needs review',
};
export const taskIsActive = (task: EngineTask) =>
  ['starting', 'running', 'waiting_human', 'canceling'].includes(task.state);

// A saved answer and resumed execution can arrive in separate snapshots.
// This is a presentation distinction, never permission to restart the work.
export const waitingAfterAnswer = (task: EngineTask) =>
  task.state === 'waiting_human' &&
  !!task.human_questions?.length &&
  task.human_questions.every((question) => question.answer !== null);
