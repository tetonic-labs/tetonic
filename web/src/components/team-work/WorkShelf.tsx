import { ArrowUpRight } from 'lucide-react';
import { journeyPriority, journeyStatus, workJourneys } from '../../lib/workJourneys';
import type { WorkRecord } from '../../lib/workspaceRecords';

export function WorkShelf({
  records,
  onWork,
}: {
  records: WorkRecord[];
  onWork: (id: string) => void;
}) {
  const work = workJourneys(records)
    .reverse()
    .sort((a, b) => journeyPriority(a, records) - journeyPriority(b, records))
    .slice(0, 3);
  if (!work.length) return null;
  return (
    <div className="tw-work-shelf" aria-label="Pick up your work">
      {work.map((record) => (
        <button
          key={record.id}
          onClick={() => onWork(record.id)}
          data-attention={journeyPriority(record, records) === 0}
        >
          <span>{journeyStatus(record, records)}</span>
          <strong>{record.title}</strong>
          <ArrowUpRight size={16} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
}
