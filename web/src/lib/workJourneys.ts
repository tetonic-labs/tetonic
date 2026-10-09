import { type WorkRecord } from '../engine/projections/records';

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
