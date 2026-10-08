import { waitingAfterAnswer, type EngineTask, type LocalApproval } from './localEngine';
import { journeyMembers } from './workJourneys';
import type { WorkRecord } from './workspaceRecords';

export type WorkSignal =
  | 'working'
  | 'needs_you'
  | 'blocked'
  | 'done'
  | 'waiting'
  | 'stopped'
  | 'unknown';
export const signalLabels: Record<WorkSignal, string> = {
  working: 'In progress',
  needs_you: 'Needs you',
  blocked: 'Blocked',
  done: 'Done',
  waiting: 'Waiting',
  stopped: 'Stopped',
  unknown: 'Status unavailable',
};
export function approvalFor(work: WorkRecord, approvals: LocalApproval[]) {
  return approvals.some(
    (a) =>
      a.status === 'pending' &&
      (a.work_id === work.id || work.turns.some((t) => t.id === a.work_id)),
  );
}
export function parentEffortTitle(work: WorkRecord, records: WorkRecord[]) {
  return records.find(
    (record) => record.id !== work.id && record.id === work.latest?.plan?.root_work_id,
  )?.title;
}
export function workSignal(
  work: WorkRecord,
  approvals: LocalApproval[] = [],
  planning: EngineTask[] = [],
): WorkSignal {
  if (approvalFor(work, approvals)) return 'needs_you';
  if (work.latest?.state === 'waiting_human')
    return waitingAfterAnswer(work.latest) ? 'waiting' : 'needs_you';
  if (
    planning.some(
      (t) => t.planning_for === work.id && ['starting', 'running', 'canceling'].includes(t.state),
    )
  )
    return 'working';
  switch (work.latest?.state) {
    case 'starting':
    case 'running':
    case 'canceling':
      return 'working';
    case 'failed':
    case 'recovery_required':
      return 'blocked';
    case 'completed':
      return 'done';
    case 'canceled':
      return 'stopped';
    case 'not_started':
      return work.latest.plan ? 'waiting' : 'blocked';
    case undefined:
      return 'waiting';
    default:
      return 'unknown';
  }
}

// A completed coordinator must not hide a child that is still working or blocked.
// Counts are recorded contributions, never inferred percentage completion.
export function combinedSignal(signals: WorkSignal[]): WorkSignal {
  if (!signals.length) return 'waiting';
  return (
    ['needs_you', 'blocked', 'working', 'unknown', 'waiting', 'stopped', 'done'] as const
  ).find((signal) => signals.includes(signal))!;
}
export function journeySummary(
  work: WorkRecord,
  records: WorkRecord[],
  approvals: LocalApproval[] = [],
  planning: EngineTask[] = [],
) {
  const members = journeyMembers(work, records);
  const signals = members.map((member) => workSignal(member, approvals, planning));
  const agents = [
    ...new Map(
      members.flatMap((m) =>
        m.latest ? [[m.latest.agent_key, m.latest.agent_name] as const] : [],
      ),
    ).values(),
  ];
  return {
    signal: combinedSignal(signals),
    members,
    agents,
    active: signals.filter((s) => s === 'working').length,
    done: signals.filter((s) => s === 'done').length,
    attention: signals.filter((s) => s === 'needs_you' || s === 'blocked').length,
  };
}
