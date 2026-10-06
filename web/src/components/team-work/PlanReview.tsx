import { PlanExecution } from './PlanExecution';
import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import {
  connectionDraftScope,
  EngineRequestError,
  taskIsActive,
  type PlanCommand,
  type PlanContent,
  type PlanView,
} from '../../lib/localEngine';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

type Edit = { base: number; brief: number; content: PlanContent };
const canonical = (value: unknown): string =>
  JSON.stringify(value, (_key, item) =>
    item && typeof item === 'object' && !Array.isArray(item)
      ? Object.fromEntries(Object.entries(item).sort(([a], [b]) => a.localeCompare(b)))
      : item,
  );
export function PlanReview({
  workId,
  onView,
  onWork,
  onBrief,
  suggestion,
  conversationActive = false,
}: {
  workId: string;
  onView?: (view: PlanView) => void;
  onWork?: (id: string, inspect?: boolean) => void;
  onBrief?: () => void;
  suggestion?: string;
  conversationActive?: boolean;
}) {
  const { client, isConnected, workspace, cancelTask } = useLocalEngine();
  const key = `tetonic_plan:${connectionDraftScope()}:${workId}`;
  const [retained] = useState(() => {
    try {
      return JSON.parse(sessionStorage.getItem(key) || '{}');
    } catch {
      return {};
    }
  });
  const [pending, setPending] = useState<PlanCommand | undefined>(retained.pending);
  const [edit, setEdit] = useState<Edit | undefined>(retained.edit);
  const [direction, setDirection] = useState<string | undefined>(retained.direction);
  const [reworking, setReworking] = useState(false);
  const captureAttempt = useRef<string>('');
  const [view, setView] = useState<PlanView>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const mounted = useRef(true);
  const current = view?.plans[0];
  useEffect(() => {
    if (view) onView?.(view);
  }, [view, onView]);
  const agents = (workspace?.agents || []).filter(
    (a) => a.key !== workspace?.shaping_agent_key && !a.plan_coordinator,
  );
  const running = !!view?.generation && taskIsActive(view.generation);
  const stale = !!current && current.brief_revision !== view?.brief_revision;
  const editStale =
    !!edit && (edit.base !== current?.revision || edit.brief !== view?.brief_revision);
  useEffect(() => {
    try {
      sessionStorage.setItem(key, JSON.stringify({ pending, edit, direction }));
    } catch {
      setError(
        'This tab cannot retain edits or an uncertain request. Keep a copy until it is saved.',
      );
    }
  }, [key, pending, edit, direction]);
  useEffect(() => {
    mounted.current = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function read() {
      if (!isConnected) return;
      try {
        const result = await client.plan(workId, controller.signal);
        if (controller.signal.aborted) return;
        setView(result);
        if (
          result.plans[0]?.status === 'drafting' ||
          (result.execution &&
            ['starting', 'running', 'waiting_human', 'canceling'].includes(result.execution.state))
        )
          timer = setTimeout(read, 2000);
      } catch (e) {
        if (!controller.signal.aborted)
          setError(e instanceof Error ? e.message : 'Could not load the plan.');
      }
    }
    void read();
    return () => {
      mounted.current = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [
    client,
    workId,
    isConnected,
    current?.revision,
    current?.status,
    view?.execution?.receipt.root_work_id,
  ]);
  async function refresh() {
    try {
      const result = await client.plan(workId);
      if (mounted.current) {
        setView(result);
        setError('');
      }
    } catch (e) {
      if (mounted.current) setError(e instanceof Error ? e.message : 'Could not reload the plan.');
    }
  }
  async function perform(command: PlanCommand) {
    if (!isConnected || gate.current) return;
    gate.current = true;
    setBusy(true);
    setError('');
    // Persist synchronously before dispatch, including when navigation follows immediately.
    try {
      sessionStorage.setItem(key, JSON.stringify({ pending: command, edit, direction }));
    } catch {
      setError('The request cannot be retained in this tab. Nothing was sent.');
      gate.current = false;
      setBusy(false);
      return;
    }
    setPending(command);
    try {
      const result = await client.updatePlan(workId, command);
      const expected =
        command.action === 'generate' || command.action === 'revise' || command.action === 'prepare'
          ? command.expected_revision + 1
          : command.revision;
      if (
        result.work_id !== workId ||
        result.revision !== expected ||
        (command.action === 'agree' &&
          (result.agreement_id !== command.request_id || result.status !== 'agreed')) ||
        ((command.action === 'generate' || command.action === 'revise') &&
          (result.request_id !== command.request_id ||
            result.brief_revision !== command.brief_revision)) ||
        (command.action === 'prepare' &&
          (result.request_id !== command.request_id ||
            result.brief_revision !== command.expected_brief_revision + 1)) ||
        (command.action === 'capture' && !result.content) ||
        (command.action === 'revise' && canonical(result.content) !== canonical(command.content))
      )
        throw new Error('The engine has not confirmed this operation. Retry the same request.');
      // Clear durable pending state even if the overlay closed during the request.
      sessionStorage.setItem(
        key,
        JSON.stringify({ edit: command.action === 'revise' ? undefined : edit, direction }),
      );
      if (!mounted.current) return;
      setPending(undefined);
      if (command.action === 'revise') setEdit(undefined);
      if (command.action === 'prepare') setReworking(false);
      setView((old) =>
        old
          ? {
              ...old,
              plans: [result, ...old.plans.filter((p) => p.revision !== result.revision)].sort(
                (a, b) => b.revision - a.revision,
              ),
            }
          : old,
      );
      await refresh();
    } catch (e) {
      if (!mounted.current) return;
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        setPending(undefined);
        await refresh();
      }
      setError(e instanceof Error ? e.message : 'Could not confirm this request.');
    } finally {
      gate.current = false;
      if (mounted.current) setBusy(false);
    }
  }
  const generate = () =>
    view &&
    perform({
      action: 'generate',
      request_id: crypto.randomUUID(),
      expected_revision: current?.revision || 0,
      brief_revision: view.brief_revision,
    });
  // Capturing a structured reply validates and stores a proposal, never launches it.
  // Do it once automatically; malformed replies remain inspectable and retryable.
  useEffect(() => {
    const identity = `${current?.revision}:${view?.generation?.id}`;
    if (
      isConnected &&
      !busy &&
      !pending &&
      current?.status === 'drafting' &&
      view?.generation?.state === 'completed' &&
      captureAttempt.current !== identity
    ) {
      captureAttempt.current = identity;
      void perform({ action: 'capture', revision: current.revision });
    }
  }, [
    current?.revision,
    current?.status,
    view?.generation?.id,
    view?.generation?.state,
    isConnected,
    busy,
    pending,
  ]);
  const disabled = !isConnected || busy || !!pending || !!view?.execution || conversationActive;
  const proposedDirection = direction ?? suggestion ?? '';
  const prepare = () =>
    view &&
    perform({
      action: 'prepare',
      request_id: crypto.randomUUID(),
      expected_revision: current?.revision || 0,
      expected_brief_revision: view.brief_revision,
      body: proposedDirection,
    });
  function change(value: Partial<PlanContent>) {
    if (edit) setEdit({ ...edit, content: { ...edit.content, ...value } });
  }
  const content = edit?.content || current?.content;
  if (conversationActive && !current) return null;
  return (
    <section className="tw-plan" aria-label="Work plan">
      {!view ? (
        <button disabled={!isConnected || busy} onClick={() => void refresh()}>
          Load work plan
        </button>
      ) : (
        <>
          {(!current || reworking) && (
            <div className="tw-plan-empty">
              <h3>{current ? 'Refine the approach' : 'Ready to put a team on it?'}</h3>
              <p>
                We’ll suggest who can help and what they should do. You review it before anyone
                starts.
              </p>
              {(suggestion || direction !== undefined) && (
                <details className="tw-direction-preview" open={reworking || undefined}>
                  <summary>Direction to share with the team</summary>
                  <label>
                    Direction for the team
                    <textarea
                      rows={5}
                      value={proposedDirection}
                      readOnly={disabled}
                      onChange={(event) => setDirection(event.target.value)}
                    />
                  </label>
                  <small>
                    Only this direction is passed into planning. Your full conversation stays here.
                  </small>
                  {direction !== undefined && suggestion && direction !== suggestion && (
                    <button
                      type="button"
                      disabled={disabled}
                      onClick={() => setDirection(undefined)}
                    >
                      Use the latest discussion
                    </button>
                  )}
                </details>
              )}
              <button
                className="cw-primary"
                disabled={
                  disabled ||
                  (!proposedDirection.trim() && !view.brief_revision) ||
                  new TextEncoder().encode(proposedDirection).length > 12000
                }
                onClick={() => void (proposedDirection.trim() ? prepare() : generate())}
              >
                {current ? 'Update proposal' : 'Prepare a plan'}
              </button>
              {!view.brief_revision && !proposedDirection && !conversationActive && (
                <>
                  <small>Continue the conversation to settle on a direction.</small>
                  {onBrief && <button onClick={onBrief}>Write the working brief →</button>}
                </>
              )}
            </div>
          )}
          {current && !view.execution && !reworking && (
            <>
              <div className="tw-plan-state">
                <strong>
                  {current.status === 'agreed'
                    ? view.execution
                      ? 'Direction agreed · execution recorded'
                      : 'Direction agreed · not started'
                    : current.status === 'drafting'
                      ? running
                        ? 'Preparing a proposal'
                        : view.generation?.state === 'completed'
                          ? 'Reply ready to review'
                          : 'Proposal not ready'
                      : 'Proposed · not started'}
                </strong>
              </div>
              {current.status === 'drafting' && (
                <div>
                  <p>
                    {running
                      ? 'The Guide is putting the proposal together…'
                      : view.generation?.state === 'completed'
                        ? 'The reply is ready to review.'
                        : view.generation
                          ? `Planning reply: ${view.generation.state.replaceAll('_', ' ')}`
                          : 'The request is saved; its reply has not started.'}
                  </p>
                  {view.generation?.error && <p role="alert">{view.generation.error}</p>}
                  {view.generation?.state === 'completed' && (
                    <button
                      className="cw-primary"
                      disabled={disabled}
                      onClick={() =>
                        void perform({ action: 'capture', revision: current.revision })
                      }
                    >
                      Review proposed plan
                    </button>
                  )}
                  {running && (
                    <button
                      disabled={disabled}
                      onClick={async () => {
                        try {
                          await cancelTask(current.generation_id);
                          await refresh();
                        } catch (e) {
                          setError(e instanceof Error ? e.message : 'Stop was not confirmed.');
                        }
                      }}
                    >
                      Stop planning reply
                    </button>
                  )}
                  {(!view.generation || view.generation.state === 'not_started') && (
                    <button
                      disabled={disabled}
                      onClick={() =>
                        void perform({
                          action: 'generate',
                          request_id: current.request_id,
                          expected_revision: current.revision - 1,
                          brief_revision: current.brief_revision,
                        })
                      }
                    >
                      Retry planning request
                    </button>
                  )}
                  {!!view.generation?.messages.length && (
                    <details>
                      <summary>Original planning reply</summary>
                      {view.generation.messages
                        .filter((m) => m.role === 'assistant')
                        .map((m) => (
                          <FormattedMarkdown key={m.id} text={m.content} />
                        ))}
                    </details>
                  )}
                </div>
              )}
              {stale && (
                <p role="alert">
                  Your brief has changed. Review and update this plan against brief{' '}
                  {view.brief_revision}.
                </p>
              )}
              {content && (
                <fieldset className="tw-plan-edit" disabled={disabled}>
                  {edit ? (
                    <>
                      <label>
                        Outcome
                        <input
                          value={content.title}
                          onChange={(e) => change({ title: e.target.value })}
                        />
                      </label>
                      <label>
                        Approach
                        <textarea
                          value={content.summary}
                          onChange={(e) => change({ summary: e.target.value })}
                        />
                      </label>
                    </>
                  ) : (
                    <>
                      <h3>{content.title}</h3>
                      <div className="tw-plan-approach">
                        <FormattedMarkdown text={content.summary} />
                      </div>
                    </>
                  )}
                  <ol className="tw-plan-assignments">
                    {content.assignments.map((assignment, index) => (
                      <li key={assignment.key}>
                        <div className="tw-plan-task-heading">
                          <span>{index + 1}</span>
                          <div>
                            <h4>{assignment.title}</h4>
                            <small>
                              {agents.find((a) => a.key === assignment.agent_key)?.name ||
                                assignment.agent_key}{' '}
                              · {assignment.token_budget.toLocaleString()} proposed tokens
                            </small>
                          </div>
                        </div>
                        <p>{assignment.deliverable}</p>
                        {!!assignment.depends_on.length && (
                          <small>
                            After:{' '}
                            {assignment.depends_on
                              .map((k) => content.assignments.find((a) => a.key === k)?.title || k)
                              .join(' · ')}
                          </small>
                        )}
                        <details open={!!edit}>
                          <summary>{edit ? 'Adjust assignment' : 'Assignment details'}</summary>
                          {edit ? (
                            <div className="tw-plan-fields">
                              <label>
                                Task
                                <input
                                  value={assignment.title}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index ? { ...a, title: e.target.value } : a,
                                      ),
                                    })
                                  }
                                />
                              </label>
                              <label>
                                Instructions
                                <textarea
                                  rows={3}
                                  value={assignment.instructions}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index ? { ...a, instructions: e.target.value } : a,
                                      ),
                                    })
                                  }
                                />
                              </label>
                              <label>
                                Agent
                                <select
                                  value={assignment.agent_key}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index ? { ...a, agent_key: e.target.value } : a,
                                      ),
                                    })
                                  }
                                >
                                  {agents.map((a) => (
                                    <option key={a.key} value={a.key}>
                                      {a.name}
                                    </option>
                                  ))}
                                </select>
                              </label>
                              <label>
                                Deliverable
                                <input
                                  value={assignment.deliverable}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index ? { ...a, deliverable: e.target.value } : a,
                                      ),
                                    })
                                  }
                                />
                              </label>
                              <label>
                                Proposed tokens
                                <input
                                  type="number"
                                  min={1}
                                  max={1000000}
                                  value={assignment.token_budget}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index
                                          ? { ...a, token_budget: Number(e.target.value) }
                                          : a,
                                      ),
                                    })
                                  }
                                />
                              </label>
                              <fieldset>
                                <legend>Depends on</legend>
                                {content.assignments
                                  .filter((a) => a.key !== assignment.key)
                                  .map((a) => (
                                    <label key={a.key}>
                                      <input
                                        type="checkbox"
                                        checked={assignment.depends_on.includes(a.key)}
                                        onChange={(e) =>
                                          change({
                                            assignments: content.assignments.map((node, i) =>
                                              i === index
                                                ? {
                                                    ...node,
                                                    depends_on: e.target.checked
                                                      ? [...node.depends_on, a.key]
                                                      : node.depends_on.filter((k) => k !== a.key),
                                                  }
                                                : node,
                                            ),
                                          })
                                        }
                                      />
                                      {a.title}
                                    </label>
                                  ))}
                              </fieldset>
                              <label>
                                Requested tools (comma separated)
                                <input
                                  value={assignment.tools.join(', ')}
                                  onChange={(e) =>
                                    change({
                                      assignments: content.assignments.map((a, i) =>
                                        i === index
                                          ? {
                                              ...a,
                                              tools: e.target.value.split(',').map((s) => s.trim()),
                                            }
                                          : a,
                                      ),
                                    })
                                  }
                                />
                              </label>
                              <button
                                disabled={content.assignments.length <= 1}
                                onClick={() =>
                                  change({
                                    assignments: content.assignments
                                      .filter((a) => a.key !== assignment.key)
                                      .map((a) => ({
                                        ...a,
                                        depends_on: a.depends_on.filter(
                                          (k) => k !== assignment.key,
                                        ),
                                      })),
                                  })
                                }
                              >
                                Remove assignment
                              </button>
                            </div>
                          ) : (
                            <>
                              <FormattedMarkdown text={assignment.instructions} />
                              <small>
                                {assignment.tools.length
                                  ? `Requested tools: ${assignment.tools.join(', ')}`
                                  : 'No tools requested'}
                              </small>
                            </>
                          )}
                        </details>
                      </li>
                    ))}
                  </ol>
                  {edit && (
                    <button
                      disabled={content.assignments.length >= 12 || !agents.length}
                      onClick={() =>
                        change({
                          assignments: [
                            ...content.assignments,
                            {
                              key: `task-${crypto.randomUUID().slice(0, 8)}`,
                              title: 'New assignment',
                              instructions: '',
                              agent_key: agents[0].key,
                              deliverable: '',
                              depends_on: [],
                              tools: [],
                              token_budget: 1000,
                            },
                          ],
                        })
                      }
                    >
                      Add assignment
                    </button>
                  )}
                  {edit ? (
                    <>
                      <label>
                        Proposed total token budget
                        <input
                          type="number"
                          min={1}
                          max={1000000}
                          value={content.token_budget}
                          onChange={(e) => change({ token_budget: Number(e.target.value) })}
                        />
                      </label>
                      <label>
                        Open questions (one per line)
                        <textarea
                          value={content.open_questions.join('\n')}
                          onChange={(e) =>
                            change({
                              open_questions: e.target.value.split('\n'),
                            })
                          }
                        />
                      </label>
                    </>
                  ) : (
                    <>
                      <p>
                        {content.assignments.length} proposed assignments ·{' '}
                        {content.token_budget.toLocaleString()} proposed tokens
                      </p>
                      {!!content.open_questions.length && (
                        <details>
                          <summary>Questions to resolve · {content.open_questions.length}</summary>
                          <ul>
                            {content.open_questions.map((q, i) => (
                              <li key={i}>{q}</li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </>
                  )}
                  {!!view.readiness.length && (
                    <details>
                      <summary>Things to resolve · {view.readiness.length}</summary>
                      <ul>
                        {view.readiness.map((r, i) => (
                          <li key={i}>{r}</li>
                        ))}
                      </ul>
                    </details>
                  )}
                  {editStale && (
                    <p role="alert">
                      A newer plan or brief is saved. Your edits are preserved. Reload and review
                      before applying them.
                    </p>
                  )}
                  <div className="tw-plan-actions">
                    {edit ? (
                      <>
                        <button
                          className="cw-primary"
                          disabled={disabled || editStale}
                          onClick={() =>
                            void perform({
                              action: 'revise',
                              request_id: crypto.randomUUID(),
                              expected_revision: edit.base,
                              brief_revision: edit.brief,
                              content: {
                                ...content,
                                open_questions: content.open_questions
                                  .map((q) => q.trim())
                                  .filter(Boolean),
                                assignments: content.assignments.map((a) => ({
                                  ...a,
                                  tools: a.tools.map((t) => t.trim()).filter(Boolean),
                                })),
                              },
                            })
                          }
                        >
                          Save plan revision
                        </button>
                        <button disabled={disabled} onClick={() => setEdit(undefined)}>
                          Discard edits
                        </button>
                        {editStale && (
                          <button
                            disabled={disabled}
                            onClick={() =>
                              setEdit({
                                ...edit,
                                base: current.revision,
                                brief: view.brief_revision,
                              })
                            }
                          >
                            Apply edits against current revisions
                          </button>
                        )}
                      </>
                    ) : (
                      <>
                        <button
                          disabled={disabled}
                          onClick={() =>
                            setEdit({
                              base: current.revision,
                              brief: view.brief_revision,
                              content: structuredClone(content),
                            })
                          }
                        >
                          Adjust plan
                        </button>
                      </>
                    )}
                  </div>
                  {view.generation && (
                    <details>
                      <summary>Planning reply and run</summary>
                      <small>
                        Request {view.generation.id} · run{' '}
                        {view.generation.run_id || 'not admitted'}
                      </small>
                      {view.generation.messages
                        .filter((m) => m.role === 'assistant')
                        .map((m) => (
                          <FormattedMarkdown key={m.id} text={m.content} />
                        ))}
                    </details>
                  )}
                </fieldset>
              )}
              {!running && !edit && (
                <button disabled={disabled} onClick={() => setReworking(true)}>
                  Refine this proposal
                </button>
              )}
            </>
          )}
        </>
      )}
      {view && !edit && !reworking && (
        <PlanExecution
          workId={workId}
          view={view}
          refresh={refresh}
          onWork={onWork}
          disabled={disabled || stale}
        />
      )}
      {pending && (
        <div role="status">
          <p>
            {busy
              ? 'Waiting for the engine…'
              : 'This operation was not confirmed. Retry checks the same request.'}
          </p>
          <button disabled={busy || !isConnected} onClick={() => void perform(pending)}>
            Retry plan operation
          </button>
        </div>
      )}
      {error && <p role="alert">{error}</p>}
      {!isConnected && <p>Reconnect to load or change the plan. Your local edits are kept.</p>}
      {error && view && !view.execution && (
        <button disabled={!isConnected || busy} onClick={() => void refresh()}>
          Reload saved plan
        </button>
      )}
      {!!view?.plans.length && (
        <details>
          <summary>Plan history · {view.plans.length}</summary>
          {view.plans.map((p) => (
            <article key={p.revision}>
              <strong>
                Plan {p.revision} · brief {p.brief_revision}
              </strong>
              <small>{p.status}</small>
              {p.content && (
                <>
                  <h4>{p.content.title}</h4>
                  <FormattedMarkdown text={p.content.summary} />
                  <ol>
                    {p.content.assignments.map((a) => (
                      <li key={a.key}>
                        {a.title} · {a.agent_key}
                      </li>
                    ))}
                  </ol>
                </>
              )}
            </article>
          ))}
        </details>
      )}
    </section>
  );
}
