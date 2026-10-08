import { ArrowUpRight } from 'lucide-react';
import { workJourneys } from '../../lib/workJourneys';
import { journeySummary, type WorkSignal } from '../../lib/workSignals';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { WorkStatus } from './WorkStatus';
import type { WorkRecord } from '../../lib/workspaceRecords';

export function WorkShelf({
  records,
  onWork,
  onAll,
}: {
  records: WorkRecord[];
  onWork: (id: string) => void;
  onAll: () => void;
}) {
  const engine = useLocalEngine();
  const priority: WorkSignal[] = [
    'needs_you',
    'blocked',
    'working',
    'waiting',
    'unknown',
    'done',
    'stopped',
  ];
  const all = workJourneys(records)
    .reverse()
    .map((record) => ({
      record,
      ...journeySummary(
        record,
        records,
        engine.approvals?.pending_approvals,
        engine.workspace?.planning_tasks,
      ),
    }))
    .sort((a, b) => priority.indexOf(a.signal) - priority.indexOf(b.signal));
  const work = all.slice(0, 3);
  if (!work.length) return null;
  return (
    <div className="tw-work-shelf" aria-label="Pick up your work">
      {work.map(({ record, signal }) => (
        <button key={record.id} onClick={() => onWork(record.id)} data-signal={signal}>
          <WorkStatus signal={signal} />
          <strong>{record.title}</strong>
          <ArrowUpRight size={16} aria-hidden="true" />
        </button>
      ))}
      {all.length > 3 && (
        <button className="tw-work-shelf-all" onClick={onAll}>
          All work <strong>{all.length} efforts</strong>
          <ArrowUpRight size={16} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
