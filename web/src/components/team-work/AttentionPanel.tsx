import { useState } from 'react';
import { CircleCheck } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { attentionItems } from '../../lib/attentionItems';
import { stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { Decision } from './Decision';
import { HumanQuestion } from './HumanQuestion';
import { WorkStatus } from './WorkStatus';
import { parentEffortTitle } from '../../lib/workSignals';

export function AttentionPanel({
  records,
  onWork,
}: {
  records: WorkRecord[];
  onWork: (id: string) => void;
}) {
  const engine = useLocalEngine();
  const current = engine.isConnected && !!engine.approvals && !engine.readErrors.Decisions;
  const [filter, setFilter] = useState('all');
  const { permissions, questions, problems, total } = attentionItems(
    records,
    engine.approvals?.pending_approvals || [],
  );
  const filters = [
    ['all', 'All', total],
    ['input', 'Decisions & questions', permissions.length + questions.length],
    ['problems', 'Needs a look', problems.length],
  ] as const;
  return (
    <section className="attention-panel" aria-label="Attention inbox">
      <p className="operator-intro">
        {total
          ? `${total} ${total === 1 ? 'request' : 'requests'} for your attention. Other work can keep moving.`
          : current
            ? 'You’re caught up.'
            : 'Checking what needs your attention.'}
      </p>
      {engine.readErrors.Decisions && (
        <p className="operator-notice">
          We couldn’t refresh decisions. These requests may have changed; approvals are paused until
          the connection recovers.
        </p>
      )}
      {!engine.isConnected && (
        <p className="operator-notice">
          {engine.isConnecting
            ? 'Connecting to your engine…'
            : 'Connection lost. These are the last known requests.'}
        </p>
      )}
      {total > 0 && (
        <div className="operator-filters" aria-label="Filter attention">
          {filters.map(([id, label, count]) => (
            <button key={id} aria-pressed={filter === id} onClick={() => setFilter(id)}>
              {label}
              <span>{count}</span>
            </button>
          ))}
        </div>
      )}
      {!total && current && (
        <div className="operator-empty">
          <CircleCheck size={27} />
          <p>Nothing is waiting for your input.</p>
        </div>
      )}
      {filter !== 'problems' &&
        permissions.map((approval) => {
          const work = records.find(
            (r) => r.id === approval.work_id || r.turns.some((t) => t.id === approval.work_id),
          );
          return (
            <details className="attention-item" key={approval.approval_id} data-signal="needs_you">
              <summary>
                <span>
                  <span className="attention-kind">
                    Permission · {work?.latest?.agent_name || 'Your team'}
                  </span>
                  <strong>{work?.title || 'An action needs your permission'}</strong>
                  <small>
                    {approval.proposal
                      ? 'Review a command before it runs'
                      : 'Action details unavailable'}
                  </small>
                </span>
              </summary>
              <Decision approval={approval} onWork={onWork} />
            </details>
          );
        })}
      {filter !== 'problems' &&
        questions.map(({ work, question }) => (
          <details className="attention-item" key={question.id} data-signal="needs_you">
            <summary>
              <span>
                <span className="attention-kind">Question · {work.latest!.agent_name}</span>
                <strong>{question.content.question}</strong>
                <small>{parentEffortTitle(work, records) || work.title}</small>
              </span>
            </summary>
            <HumanQuestion
              task={work.latest!}
              question={question}
              refresh={engine.refresh}
              showHeading={false}
            />
            <button className="operator-secondary" onClick={() => onWork(work.id)}>
              Open related work
            </button>
          </details>
        ))}
      {filter !== 'input' &&
        problems.map((work) => (
          <article className="attention-problem" key={work.id} data-signal="blocked">
            <WorkStatus signal="blocked" label={stateLabel(work)} />
            <h3>{work.title}</h3>
            {parentEffortTitle(work, records) && <small>{parentEffortTitle(work, records)}</small>}
            <p>{work.latest?.agent_name} · Review what happened before deciding the next step.</p>
            <button className="operator-secondary" onClick={() => onWork(work.id)}>
              Review work
            </button>
          </article>
        ))}
      {total > 0 &&
        ((filter === 'input' && !permissions.length && !questions.length) ||
          (filter === 'problems' && !problems.length)) && (
          <p className="operator-empty">Nothing in this view.</p>
        )}
    </section>
  );
}
