import { useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope, EngineRequestError, type PlanView } from '../../lib/localEngine';
import { stateLabels } from '../../lib/workspaceRecords';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { HumanQuestion } from './HumanQuestion';
import { PlanDirectionEditor } from './PlanDirectionEditor';

export function PlanExecution({
  workId,
  view,
  refresh,
  onWork,
}: {
  workId: string;
  view: PlanView;
  refresh: () => Promise<void>;
  onWork?: (id: string, inspect?: boolean) => void;
}) {
  const { client, isConnected, cancelTask, workspace } = useLocalEngine();
  const key = `tetonic_plan_start:${connectionDraftScope()}:${workId}`;
  const [pending, setPending] = useState<{ request_id: string; revision: number } | null>(() => {
    try {
      return JSON.parse(sessionStorage.getItem(key) || 'null');
    } catch {
      return null;
    }
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const execution = view.execution;
  const plan = view.plans[0];
  if (!execution && plan?.status !== 'agreed') return null;
  const content = execution?.receipt.content || plan?.content;
  if (!content) return null;
  const coordination =
    content.token_budget - content.assignments.reduce((total, a) => total + a.token_budget, 0);
  async function start() {
    if (busy || !plan || !isConnected) return;
    setBusy(true);
    setError('');
    const request = pending || { request_id: crypto.randomUUID(), revision: plan.revision };
    try {
      sessionStorage.setItem(key, JSON.stringify(request));
      setPending(request);
      const result = await client.startPlan(workId, request);
      if (
        result.receipt.request_id !== request.request_id ||
        result.receipt.revision !== request.revision
      )
        throw new Error('The engine has not confirmed this start. Retry the same request.');
      sessionStorage.removeItem(key);
      setPending(null);
      await refresh();
    } catch (e) {
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        // An explicit rejection is safe to correct; an uncertain response retains
        // the same command so a retry cannot dispatch a second team.
        sessionStorage.removeItem(key);
        setPending(null);
      }
      setError(e instanceof Error ? e.message : 'Could not confirm plan start.');
      await refresh();
    } finally {
      setBusy(false);
    }
  }
  const active =
    execution && ['starting', 'running', 'waiting_human', 'canceling'].includes(execution.state);
  const result =
    execution?.state === 'completed'
      ? execution.root?.messages.filter((m) => m.role === 'assistant').at(-1)?.content
      : undefined;
  const tasks = [...(execution?.root ? [execution.root] : []), ...(execution?.assignments || [])];
  const questions = tasks.flatMap((task) =>
    (task.human_questions || []).map((question) => ({ task, question })),
  );
  const pendingQuestions = questions.filter(
    ({ task, question }) => !question.answer && task.state === 'waiting_human',
  );
  const pastQuestions = questions.filter((row) => !pendingQuestions.includes(row));
  const ids = new Set([
    execution?.receipt.root_work_id,
    ...(execution?.receipt.assignments.map((a) => a.work_id) || []),
  ]);
  const usage = (workspace?.usage || []).filter((row) => ids.has(row.work_id));
  const used = usage.reduce((sum, row) => sum + row.input_tokens + row.output_tokens, 0);
  const usageIncomplete =
    usage.length !== ids.size || usage.some((row) => row.unknown_calls || row.pending_calls);
  const rootUsage = usage.find((row) => row.work_id === execution?.receipt.root_work_id);
  const rootAllowance = rootUsage?.budget
    ? rootUsage.budget.token_limit - rootUsage.budget.delegated_tokens
    : null;
  const coordinationOverrun =
    rootUsage &&
    rootAllowance !== null &&
    rootUsage.input_tokens + rootUsage.output_tokens > rootAllowance;
  const openWork = (event: React.MouseEvent<HTMLAnchorElement>, id: string, inspect = false) => {
    if (
      onWork &&
      !event.metaKey &&
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.altKey &&
      event.button === 0
    ) {
      event.preventDefault();
      onWork(id, inspect);
    }
  };
  return (
    <section className="cw-plan-execution" data-started={!!execution} aria-label="Team execution">
      <h3>
        {execution
          ? result
            ? 'Your team’s result'
            : active
              ? 'Your team at work'
              : 'Your team’s work'
          : 'Ready for your team'}
      </h3>
      {!execution ? (
        <>
          <p>
            {content.assignments.length} assignments share a {content.token_budget.toLocaleString()}{' '}
            token allowance, including {coordination.toLocaleString()} for coordination and the
            combined result.
          </p>
          {view.execution_max_seconds && (
            <p>
              Up to {Math.ceil(view.execution_max_seconds / 60)} minutes for the whole plan,
              including time waiting for each agent.
            </p>
          )}
          <button
            className="cw-primary"
            disabled={busy || !isConnected || (!view.execution_available && !pending)}
            onClick={() => void start()}
          >
            {busy
              ? 'Starting your team…'
              : pending
                ? 'Check this start again'
                : 'Start agreed plan'}
          </button>
          <small>
            Starts the agreed assignments. Reported token usage is tracked; this is not a billing
            cap.
          </small>
        </>
      ) : (
        <>
          <p role="status">
            {!isConnected && 'Last seen: '}
            {pendingQuestions.length
              ? 'Needs your input'
              : stateLabels[execution.state] || 'Status unavailable'}{' '}
            · {execution.assignments.filter((t) => t.state === 'completed').length} of{' '}
            {execution.receipt.assignments.length} contributions ready
          </p>
          {execution.error && <p role="alert">{execution.error}</p>}
          {coordinationOverrun && (
            <p role="status">
              {!isConnected ? 'Last recorded: ' : ''}Coordination used{' '}
              {(rootUsage.input_tokens + rootUsage.output_tokens).toLocaleString()} tokens against
              its {rootAllowance.toLocaleString()} allowance. Unused contribution allowances are not
              automatically moved into coordination.
            </p>
          )}
          {pendingQuestions.map(({ task, question }) => (
            <HumanQuestion key={question.id} task={task} question={question} refresh={refresh} />
          ))}
          {result && (
            <div className="tw-team-result">
              <FormattedMarkdown text={result} />
            </div>
          )}
          <details className="tw-execution-brief">
            <summary>What the team was given</summary>
            {execution.receipt.brief ? (
              <>
                <p>
                  Brief
                  {execution.receipt.brief_revision ? ` ${execution.receipt.brief_revision}` : ''},
                  saved when this team started. Later edits are not included.
                </p>
                <FormattedMarkdown text={execution.receipt.brief} />
              </>
            ) : (
              <p>The original brief is unavailable for this saved run.</p>
            )}
          </details>
          <div className="tw-contributions-heading">
            <h4>Contributions</h4>
            <span>Open any contribution to read it here</span>
          </div>
          <div className="tw-contributions">
            {execution.receipt.assignments.map((pin) => {
              const task = execution.assignments.find((t) => t.id === pin.work_id);
              const assignment = execution.receipt.content.assignments.find(
                (a) => a.key === pin.assignment_key,
              )!;
              const state =
                !task || task.state === 'not_started'
                  ? active
                    ? 'Up next'
                    : 'Did not start'
                  : stateLabels[task.state];
              const response = task?.messages.filter((m) => m.role === 'assistant').at(-1)?.content;
              return (
                <details key={pin.work_id} className="tw-contribution">
                  <summary>
                    <span>
                      <strong>{assignment.title}</strong>
                      <small>{task?.agent_name || pin.agent_key}</small>
                    </span>
                    <span className="tw-contribution-state" data-state={task?.state}>
                      {state}
                    </span>
                  </summary>
                  {task?.error && <p role="alert">{task.error}</p>}
                  {response ? (
                    <FormattedMarkdown text={response} />
                  ) : (
                    <p>
                      {task?.state === 'not_started' || !task
                        ? assignment.deliverable
                        : 'No response recorded yet.'}
                    </p>
                  )}
                  <a
                    aria-label={`Inspect ${assignment.title}`}
                    href={`#work=${encodeURIComponent(pin.work_id)}`}
                    onClick={(e) => openWork(e, pin.work_id)}
                  >
                    Inspect this assignment →
                  </a>
                </details>
              );
            })}
          </div>
          {usage.length > 0 && (
            <details className="tw-plan-usage">
              <summary>
                Usage{' '}
                <span>
                  {!isConnected ? 'Last recorded · ' : ''}
                  {usageIncomplete ? '≥ ' : ''}
                  {used.toLocaleString()} / {content.token_budget.toLocaleString()} tokens reported
                </span>
              </summary>
              <p>
                {usageIncomplete
                  ? 'Some usage is still unconfirmed. This is the reported amount so far.'
                  : 'Reported usage across coordination and assignments.'}{' '}
                This is a token allowance, not a billing cap.
              </p>
              <ul>
                {usage.map((row) => (
                  <li key={row.work_id}>
                    <span>
                      {tasks.find((t) => t.id === row.work_id)?.agent_name || 'Assignment'}
                    </span>
                    <span>
                      {(row.input_tokens + row.output_tokens).toLocaleString()} reported
                      {row.budget &&
                        ` / ${(row.budget.token_limit - row.budget.delegated_tokens).toLocaleString()} allowed`}{' '}
                      · {row.held_tokens.toLocaleString()} held
                    </span>
                  </li>
                ))}
              </ul>
            </details>
          )}
          {!!pastQuestions.length && (
            <details className="tw-answered-questions">
              <summary>Earlier questions · {pastQuestions.length}</summary>
              {pastQuestions.map(({ task, question }) => (
                <HumanQuestion
                  key={question.id}
                  task={task}
                  question={question}
                  refresh={refresh}
                />
              ))}
            </details>
          )}
          <PlanDirectionEditor execution={execution} refresh={refresh} />
          {execution.root && (
            <a
              href={`#work=${encodeURIComponent(execution.root.id)}&inspect=1`}
              onClick={(e) => openWork(e, execution.root!.id, true)}
            >
              Inspect coordination activity →
            </a>
          )}
          {active && execution.root && (
            <button
              disabled={!isConnected || busy || execution.state === 'canceling'}
              onClick={async () => {
                setBusy(true);
                try {
                  await cancelTask(execution.root!.id);
                  await refresh();
                } catch (e) {
                  setError(e instanceof Error ? e.message : 'Could not confirm stop.');
                } finally {
                  setBusy(false);
                }
              }}
            >
              Stop this plan
            </button>
          )}
          {!active && !result && (
            <p>Recorded contributions are kept. This plan will not restart automatically.</p>
          )}
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
