import { useMemo, useState } from 'react';
import { Search } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { workJourneys } from '../../lib/workJourneys';
import { journeySummary, type WorkSignal } from '../../lib/workSignals';
import { type WorkRecord } from '../../engine/projections/records';
import { WorkStatus } from './WorkStatus';

const groups = [
  { id: 'attention', title: 'Needs your attention', signals: ['needs_you', 'blocked'] },
  { id: 'active', title: 'In progress', signals: ['working'] },
  { id: 'ready', title: 'Ready to review', signals: ['done'] },
  { id: 'later', title: 'On hold & saved', signals: ['waiting', 'stopped', 'unknown'] },
];
export function WorkOverview({
  records,
  onWork,
}: {
  records: WorkRecord[];
  onWork: (id: string) => void;
}) {
  const engine = useLocalEngine();
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [limit, setLimit] = useState(30);
  const rows = useMemo(
    () =>
      workJourneys(records)
        .slice()
        .reverse()
        .map((work) => ({
          work,
          ...journeySummary(
            work,
            records,
            engine.approvals?.pending_approvals,
            engine.workspace?.planning_tasks,
          ),
        })),
    [records, engine.approvals, engine.workspace?.planning_tasks],
  );
  const matching = rows.filter((row) =>
    `${row.work.title} ${row.agents.join(' ')} ${row.members.flatMap((m) => m.turns.map((t) => t.input)).join(' ')}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  const visible = groups
    .filter((g) => filter === 'all' || filter === g.id)
    .flatMap((g) => matching.filter((r) => g.signals.includes(r.signal)))
    .slice(0, limit);
  const total = matching.filter(
    (r) => filter === 'all' || groups.find((g) => g.id === filter)?.signals.includes(r.signal),
  ).length;
  return (
    <section className="work-overview" aria-label="Work overview">
      <p className="operator-intro">
        {rows.length
          ? `${rows.length} efforts. Keep your attention where it matters.`
          : 'Give your team a direction. Their work will appear here.'}
      </p>
      <label className="operator-search">
        <Search size={16} aria-hidden="true" />
        <input
          aria-label="Find work"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setLimit(30);
          }}
          placeholder="Find an effort, agent or request…"
        />
      </label>
      <div className="operator-filters" aria-label="Filter work">
        {[{ id: 'all', title: 'All', signals: [] as string[] }, ...groups].map((g) => (
          <button
            key={g.id}
            aria-pressed={filter === g.id}
            onClick={() => {
              setFilter(g.id);
              setLimit(30);
            }}
          >
            {g.id === 'attention' ? 'Needs attention' : g.title}
            <span>
              {g.id === 'all'
                ? rows.length
                : rows.filter((r) => g.signals.includes(r.signal)).length}
            </span>
          </button>
        ))}
      </div>
      {!engine.isConnected && (
        <p className="operator-notice">Last recorded state. Reconnect to check what has changed.</p>
      )}
      {groups.map((group) => {
        const entries = visible.filter((r) => group.signals.includes(r.signal));
        if (!entries.length) return null;
        return (
          <section className="work-group" key={group.id} aria-label={group.title}>
            <h3>
              {group.title}
              <span>{matching.filter((r) => group.signals.includes(r.signal)).length}</span>
            </h3>
            {entries.map(({ work, signal, members, agents, active, done, attention }) => (
              <button
                className="work-overview-row"
                data-signal={signal}
                key={work.id}
                onClick={() => onWork(work.id)}
              >
                <span className="work-row-heading">
                  <strong>{work.title}</strong>
                  <WorkStatus signal={signal as WorkSignal} />
                </span>
                <span className="work-row-context">
                  {agents.slice(0, 2).join(', ') || 'No agent assigned'}
                  {agents.length > 2 && ` +${agents.length - 2}`}
                  <span>
                    {members.length > 1
                      ? `${done}/${members.length} contributions done`
                      : 'Assignment'}
                  </span>
                </span>
                {(active > 0 || attention > 0) && (
                  <span className="work-row-update">
                    {[
                      active ? `${active} running` : '',
                      attention
                        ? `${attention} ${attention === 1 ? 'needs' : 'need'} attention`
                        : '',
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </span>
                )}
              </button>
            ))}
          </section>
        );
      })}
      {!visible.length && rows.length > 0 && (
        <p className="operator-empty">No work matches this view. Try another filter or search.</p>
      )}
      {total > visible.length && (
        <button className="operator-more" onClick={() => setLimit(limit + 30)}>
          Show more · {total - visible.length} remaining
        </button>
      )}
    </section>
  );
}
