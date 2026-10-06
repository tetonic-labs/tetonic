import { needsHelp, stateLabel, type WorkRecord } from './workspaceRecords';
import { taskIsActive } from './localEngine';

// A launched plan is one human undertaking, with inspectable contributions.
// Keep the source discussion and children in the underlying records.
export function workJourneys(records: WorkRecord[]) {
  const sources = new Set(
    records.flatMap((r) => (r.latest?.plan ? [r.latest.plan.source_work_id] : [])),
  );
  return records.filter((r) => !r.latest?.plan?.assignment_key && !sources.has(r.id));
}

export function journeyMembers(work: WorkRecord, records: WorkRecord[]) {
  return records.filter((r) => r.id === work.id || r.latest?.plan?.root_work_id === work.id);
}

export function journeyStatus(work: WorkRecord, records: WorkRecord[]) {
  const members = journeyMembers(work, records);
  if (members.some((r) => r.latest?.state === 'waiting_human')) return 'Needs your input';
  return stateLabel(work);
}

export function journeyPriority(work: WorkRecord, records: WorkRecord[]) {
  const members = journeyMembers(work, records);
  if (members.some(needsHelp)) return 0;
  if (members.some((r) => r.latest && taskIsActive(r.latest))) return 1;
  return 2;
}
