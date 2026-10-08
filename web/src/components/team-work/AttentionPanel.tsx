import { useRef, useState } from 'react';
import { CircleCheck } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { attentionItems } from '../../lib/attentionItems';
import { stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { ApprovalRequests } from './ApprovalRequests';
import type { EngineTask, WorkHumanQuestion } from '../../lib/localEngine';
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
  const [answers, setAnswers] = useState<
    { task: EngineTask; question: WorkHumanQuestion; title: string }[]
  >([]);
  const questionOrder = useRef<string[]>([]);
  const { permissions, questions, problems, total } = attentionItems(
    records,
    engine.approvals?.pending_approvals || [],
  );
  const filters = [
    ['all', 'All', total],
    ['input', 'Decisions & questions', permissions.length + questions.length],
    ['problems', 'Needs a look', problems.length],
  ] as const;
  const displayed = new Map(
    questions.map(({ work, question }) => [
      question.id,
      { task: work.latest!, question, title: parentEffortTitle(work, records) || work.title },
    ]),
  );
  for (const answer of answers) {
    const task =
      records.flatMap((work) => work.turns).find((t) => t.id === answer.task.id) || answer.task;
    displayed.set(answer.question.id, { ...answer, task });
  }
  for (const id of displayed.keys())
    if (!questionOrder.current.includes(id)) questionOrder.current.push(id);
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
      <div hidden={filter === 'problems'}>
        <ApprovalRequests tasks={records.flatMap((work) => work.turns)} onWork={onWork} />
        {questionOrder.current
          .filter((id) => displayed.has(id))
          .map((id) => {
            const { task, question, title } = displayed.get(id)!;
            return (
              <HumanQuestion
                key={id}
                task={task}
                question={question}
                workTitle={title}
                refresh={engine.refresh}
                onWork={() => onWork(task.id)}
                onAnswered={(receipt) =>
                  setAnswers((old) => [
                    ...old.filter((a) => a.question.id !== id),
                    { task, question: receipt, title },
                  ])
                }
              />
            );
          })}
      </div>
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
