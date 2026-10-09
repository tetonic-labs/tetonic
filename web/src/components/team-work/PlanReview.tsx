import { PlanExecution } from './PlanExecution';
import { PlanHandoff } from './PlanHandoff';
import { GuidePlanCard } from './GuidePlanCard';
import { ProposalHistory } from './ProposalHistory';
import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../engine/connection';
import { EngineRequestError } from '../../engine/failure';
import { taskIsActive } from '../../engine/projections/taskState';
import { type PlanCommand, type PlanContent, type PlanView } from '../../engine/contracts';
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
  onAgentSettings,
  onDiscuss,
  onTools,
  suggestion,
  conversationActive = false,
  conversationTurnId,
  inConversation = false,
  onTeamMap,
  initialDetailsOpen = false,
}: {
  workId: string;
  onView?: (view: PlanView) => void;
  onWork?: (id: string, inspect?: boolean) => void;
  onBrief?: () => void;
  onAgentSettings?: (key: string) => void;
  onDiscuss?: (text: string) => void;
  onTools?: () => void;
  suggestion?: string;
  conversationActive?: boolean;
  conversationTurnId?: string;
  inConversation?: boolean;
  onTeamMap?: (id: string) => void;
  initialDetailsOpen?: boolean;
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
  const [readError, setReadError] = useState('');
  const readPlan = useRef<() => Promise<void>>(async () => {});
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const mounted = useRef(true);
  const current = view?.plans[0];
  const [preparing, setPreparing] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(initialDetailsOpen);
  const detailSurface = useRef<HTMLDivElement>(null);
  const summarySurface = useRef<HTMLDivElement>(null);
  const wasOpen = useRef(false);
  useEffect(() => {
    if (detailsOpen) detailSurface.current?.focus({ preventScroll: true });
    else if (wasOpen.current)
      summarySurface.current?.querySelector('button')?.focus({ preventScroll: true });
    wasOpen.current = detailsOpen;
  }, [detailsOpen]);
  function discuss(text: string) {
    wasOpen.current = false;
    setDetailsOpen(false);
    onDiscuss?.(text);
  }
  const compact =
    inConversation &&
    !!view?.execution &&
    !detailsOpen &&
    !edit &&
    !pending &&
    !error &&
    !reworking;
  useEffect(() => {
    if (view) onView?.(view);
  }, [view, onView]);
  const agents = (workspace?.agents || []).filter(
    (a) => a.key !== workspace?.shaping_agent_key && !a.plan_coordinator,
  );
  const running = !!view?.generation && taskIsActive(view.generation);
  const stale = !!current && current.brief_revision !== view?.brief_revision;
  const unresolved =
    view?.readiness.filter(
      (reason) => !view.setup_issues?.some((issue) => issue.message === reason),
    ) || [];
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
    let active = true;
    let request: AbortController | undefined;
    let timer: ReturnType<typeof setTimeout>;
    let retryDelay = 2000;
    async function read() {
      if (!isConnected || !active) return;
      clearTimeout(timer);
      request?.abort();
      const controller = new AbortController();
      request = controller;
      try {
        const result = await client.plan(workId, controller.signal);
        if (controller.signal.aborted) return;
        setView(result);
        setReadError('');
        retryDelay = 2000;
        if (
          conversationActive ||
          (result.recovery &&
            !result.recovery.available &&
            result.execution?.state !== 'recovery_required') ||
          result.plans[0]?.status === 'drafting' ||
          (result.execution &&
            ['starting', 'running', 'waiting_human', 'canceling'].includes(result.execution.state))
        )
          timer = setTimeout(read, 2000);
      } catch (e) {
        if (!controller.signal.aborted) {
          setReadError(e instanceof Error ? e.message : 'Could not load the plan.');
          // Retry reads only. Uncertain mutations still require an explicit retry.
          timer = setTimeout(read, retryDelay);
          retryDelay = Math.min(retryDelay * 2, 15000);
        }
      }
    }
    readPlan.current = read;
    void read();
    return () => {
      active = false;
      mounted.current = false;
      request?.abort();
      clearTimeout(timer);
    };
  }, [
    client,
    workId,
    isConnected,
    conversationActive,
    conversationTurnId,
    current?.revision,
    current?.status,
    view?.execution?.receipt.root_work_id,
  ]);
  async function refresh() {
    await readPlan.current();
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
      !readError &&
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
    readError,
    busy,
    pending,
  ]);
  const disabled =
    !isConnected || !!readError || busy || !!pending || !!view?.execution || conversationActive;
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
  const executionPanel =
    view && !edit && !reworking ? (
      <PlanExecution
        workId={workId}
        view={view}
        refresh={refresh}
        onWork={onWork}
        onAgentSettings={onAgentSettings}
        onDiscuss={onDiscuss ? discuss : undefined}
        onTools={onTools}
        disabled={disabled || stale}
      />
    ) : null;
  if (conversationActive && !current && !readError) return null;
  return (
    <section className="tw-plan" aria-label="Work plan">
      {readError && (
        <div>
          <p role="status" title={readError}>
            {view ? 'Showing the last saved plan. ' : 'The plan could not be loaded. '}
            {isConnected ? 'Retrying automatically…' : 'Reconnect to resume updates.'}
          </p>
          <button
            type="button"
            className="px-text-button"
            disabled={!isConnected || busy}
            onClick={() => void refresh()}
          >
            Retry plan refresh
          </button>
        </div>
      )}
      {compact && view && (
        <div ref={summarySurface}>
          <GuidePlanCard view={view} onReview={() => setDetailsOpen(true)} onTeamMap={onTeamMap} />
        </div>
      )}
      <div className="tw-plan-detail-content" hidden={compact} ref={detailSurface} tabIndex={-1}>
        {inConversation && view?.execution && (
          <button
            type="button"
            className="px-text-button"
            onClick={() => setDetailsOpen(false)}
            disabled={!!edit || !!pending || !!error || reworking}
          >
            Back to conversation
          </button>
        )}
        {!view ? (
          <button disabled={!isConnected || busy} onClick={() => void refresh()}>
            Load work plan
          </button>
        ) : (
          <>
            {!current && !preparing && (
              <button
                className={onDiscuss ? 'cw-primary' : 'px-text-button'}
                disabled={disabled}
                onClick={() =>
                  onDiscuss
                    ? discuss(
                        'Help me turn this discussion into a proposal for the team. Check the available agents, access and allowance, and ask if anything important is still unclear. Do not start work yet.',
                      )
                    : setPreparing(true)
                }
              >
                {onDiscuss ? 'Plan this with the Guide' : 'Plan work from this discussion'}
              </button>
            )}
            {((!current && preparing) || reworking) && (
              <div className="tw-plan-empty">
                <h3>{current ? 'Refine the approach' : 'What should the team accomplish?'}</h3>
                <p>
                  We’ll suggest who can help and what they should do. You review it before anyone
                  starts.
                </p>
                <div className="tw-direction-preview">
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
                </div>
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
                {!current && (
                  <button type="button" onClick={() => setPreparing(false)}>
                    Keep discussing
                  </button>
                )}
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
                <div className="tw-plan-state" aria-label="Current proposal">
                  <strong>
                    {edit
                      ? 'Editing the current proposal'
                      : current.status === 'drafting'
                        ? 'Preparing the current proposal'
                        : 'Current proposal'}
                  </strong>
                  <span>Version {current.revision} · not started</span>
                </div>
                {conversationActive && content && (
                  <p role="status">
                    The Guide is replying. Review the current proposal when the reply finishes.
                  </p>
                )}
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
                    Your direction has changed. Ask the Guide to update this proposal before
                    starting.
                  </p>
                )}
                {content &&
                  (edit ? (
                    <fieldset className="tw-plan-edit" disabled={disabled}>
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
                                  .map(
                                    (k) => content.assignments.find((a) => a.key === k)?.title || k,
                                  )
                                  .join(' · ')}
                              </small>
                            )}
                            <details open>
                              <summary>Adjust assignment</summary>
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
                                                        : node.depends_on.filter(
                                                            (k) => k !== a.key,
                                                          ),
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
                                                tools: e.target.value
                                                  .split(',')
                                                  .map((s) => s.trim()),
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
                      {!!unresolved.length && (
                        <details>
                          <summary>Things to resolve · {unresolved.length}</summary>
                          <ul>
                            {unresolved.map((r, i) => (
                              <li key={i}>{r}</li>
                            ))}
                          </ul>
                        </details>
                      )}
                      {editStale && (
                        <p role="alert">
                          A newer plan or brief is saved. Your edits are preserved. Reload and
                          review before applying them.
                        </p>
                      )}
                      <div className="tw-plan-actions">
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
                          Save changes
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
                  ) : (
                    <>
                      <PlanHandoff content={content}>
                        <div className="tw-plan-actions">
                          {!running && onDiscuss && (
                            <button
                              disabled={disabled}
                              onClick={() =>
                                discuss(
                                  `Update the current proposal “${content.title}” (version ${current.revision}). Inspect the saved proposal and apply these changes to it; do not start work. My changes: `,
                                )
                              }
                            >
                              Ask the Guide for changes
                            </button>
                          )}
                          <button
                            className="px-text-button"
                            disabled={disabled}
                            onClick={() =>
                              setEdit({
                                base: current.revision,
                                brief: view.brief_revision,
                                content: structuredClone(content),
                              })
                            }
                          >
                            Edit details yourself
                          </button>
                          {!running && !onDiscuss && (
                            <button disabled={disabled} onClick={() => setReworking(true)}>
                              Refine this proposal
                            </button>
                          )}
                        </div>
                      </PlanHandoff>
                      {executionPanel}
                    </>
                  ))}
              </>
            )}
          </>
        )}
        {(!content || view?.execution) && executionPanel}
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
          <button
            disabled={!isConnected || busy}
            onClick={() => {
              setError('');
              void refresh();
            }}
          >
            Reload saved plan
          </button>
        )}
        {view && (
          <ProposalHistory
            key={current?.revision}
            plans={view.plans}
            currentRevision={current?.revision}
          />
        )}
        {onBrief && !!view?.brief_revision && (
          <button type="button" className="px-text-button" onClick={onBrief}>
            Saved direction & history
          </button>
        )}
      </div>
    </section>
  );
}
