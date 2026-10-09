import { waitingAfterAnswer } from './taskState';
import { type EngineTask, type EngineWorkspace, type LocalWorkItem } from '../contracts';

export interface WorkRecord {
  id: string;
  title: string;
  turns: EngineTask[];
  latest?: EngineTask;
  item?: LocalWorkItem;
}

// A compact presentation label, not a rewritten instruction. Full text stays
// in the recorded turns and is displayed in the work view.
function displayTitle(text: string) {
  return text.split(/(?<=[.!?])\s/)[0].split('\n')[0];
}

// A reply belongs to its recorded parent, never the last task with the same
// agent name. The local engine permits one successor per turn.
export function conversation(tasks: EngineTask[], last: EngineTask) {
  const turns: EngineTask[] = [];
  let current: EngineTask | undefined = last;
  const seen = new Set<string>();
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    turns.unshift(current);
    const parent: string | null | undefined = current.parent_id;
    current = tasks.find((task) => task.id === parent);
  }
  return turns;
}

export function mergeTasks(previous: EngineTask[], incoming: EngineTask[]) {
  const merged = new Map(previous.map((task) => [task.id, task]));
  for (const task of incoming) {
    const old = merged.get(task.id);
    if (!old || task.sequence >= old.sequence) merged.set(task.id, task);
  }
  return [...merged.values()];
}

export function mergeWorkspace(previous: EngineWorkspace | null, next: EngineWorkspace) {
  if (!previous || previous.organization !== next.organization || previous.team_id !== next.team_id)
    return next;
  return { ...next, tasks: mergeTasks(previous.tasks, next.tasks) };
}

export function workRecords(tasks: EngineTask[], items: LocalWorkItem[]): WorkRecord[] {
  const parents = new Set(tasks.map((task) => task.parent_id).filter(Boolean));
  const records: WorkRecord[] = tasks
    .filter((task) => !parents.has(task.id))
    .map((latest) => {
      const turns = conversation(tasks, latest);
      const root = turns[0];
      const item = items.find((entry) => entry.id === root.id);
      return {
        id: root.id,
        title: displayTitle(root.plan?.title || item?.title || root.input),
        turns,
        latest,
        item,
      };
    });
  for (const item of items) {
    if (!tasks.some((task) => task.id === item.id))
      records.push({ id: item.id, title: item.title, turns: [], item });
  }
  return records;
}

export const stateLabels: Record<string, string> = {
  waiting_human: 'Needs your input',
  not_started: 'Waiting to start',
  starting: 'Starting',
  running: 'Working',
  canceling: 'Stopping',
  canceled: 'Stopped',
  failed: 'Couldn’t finish',
  completed: 'Result ready',
  recovery_required: 'Interrupted',
};
export const stateLabel = (work: WorkRecord) => {
  if (work.latest && waitingAfterAnswer(work.latest)) return 'Waiting to continue';
  if (work.latest?.purpose === 'explore') {
    if (work.latest.state === 'completed') return 'Discussion ready';
    if (work.latest.state === 'running') return 'Thinking it through';
  }
  return work.latest
    ? stateLabels[work.latest.state] || 'Status unavailable'
    : 'Saved · no run recorded';
};
export const needsHelp = (work: WorkRecord) =>
  !!work.latest &&
  !waitingAfterAnswer(work.latest) &&
  (['failed', 'recovery_required', 'waiting_human'].includes(work.latest.state) ||
    (work.latest.state === 'not_started' && !work.latest.plan));
export const canReply = (work?: WorkRecord) =>
  !!work?.latest &&
  !work.latest.plan &&
  ['completed', 'failed', 'canceled'].includes(work.latest.state);
