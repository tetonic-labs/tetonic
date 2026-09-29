import { useEffect, useRef, useState } from 'react';
import type { Dispatch } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUpRight,
  Check,
  ChevronDown,
  Circle,
  Flag,
  GitBranch,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Sparkles,
} from 'lucide-react';
import type { Agent, Team } from '../../types';
import type { WorkItem, WorkKind } from '../../types/workroom';
import {
  kindLabels,
  needsJudgment,
  newWork,
  statusLabels,
  type WorkAction,
} from '../../lib/workroom';
import { Portrait } from '../ui/Portrait';
import { teammate } from '../../lib/teammates';

interface Props {
  items: WorkItem[];
  dispatch: Dispatch<WorkAction>;
  agents: Agent[];
  teams: Team[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onAgent: (id: string) => void;
  onMap: (teamId: string) => void;
  onTeam?: (teamId: string, workId: string) => void;
}
type DecisionDraft = { optionId: string; note: string };
const kindIcons = { assignment: Flag, responsibility: RefreshCw, response: GitBranch };

export function Workroom({
  items,
  dispatch,
  agents,
  teams,
  selectedId,
  onSelect,
  onAgent,
  onMap,
  onTeam,
}: Props) {
  const [context, setContext] = useState('all');
  const [lens, setLens] = useState<'all' | 'review'>('all');
  const [idea, setIdea] = useState('');
  const [decisionDrafts, setDecisionDrafts] = useState<Record<string, DecisionDraft>>({});
  const scroll = useRef<HTMLDivElement>(null),
    homeScroll = useRef(0),
    origin = useRef<HTMLElement | null>(null);
  const selected = items.find((i) => i.id === selectedId);
  const contexts = [...new Set(items.map((i) => i.context))];
  const scoped = items.filter((i) => context === 'all' || i.context === context);
  const forYou = scoped.filter(needsJudgment);
  const others = scoped.filter((i) => !needsJudgment(i));
  const outside = items.filter(
    (i) => needsJudgment(i) && context !== 'all' && i.context !== context,
  ).length;
  useEffect(() => {
    if (!scroll.current) return;
    scroll.current.scrollTop = selectedId ? 0 : homeScroll.current;
    if (selectedId)
      scroll.current.querySelector<HTMLElement>('.wr-detail h1')?.focus({ preventScroll: true });
    else if (origin.current?.isConnected) origin.current.focus({ preventScroll: true });
  }, [selectedId]);
  function select(id: string) {
    homeScroll.current = scroll.current?.scrollTop || 0;
    origin.current = document.activeElement as HTMLElement;
    onSelect(id);
  }
  function start() {
    if (!idea.trim()) return;
    const agentIds = agents.slice(0, 3).map((a) => a.id);
    const item = {
      ...newWork('work-' + crypto.randomUUID(), idea, context),
      teamId: '',
      agentIds,
      leadId: agentIds[0] || '',
    };
    dispatch({ type: 'add', item });
    setIdea('');
    select(item.id);
  }
  return (
    <div className="workroom" ref={scroll}>
      <div className="wr-page" hidden={!!selected}>
        <div className="wr-opening">
          <div className="wr-topline">
            <h1>
              Your <em>work.</em>
            </h1>
            <label className="wr-context">
              <span className="sr-only">Work context</span>
              <select
                aria-label="Work context"
                value={context}
                onChange={(e) => setContext(e.target.value)}
              >
                <option value="all">All contexts</option>
                {contexts.map((name) => (
                  <option key={name}>{name}</option>
                ))}
              </select>
              <ChevronDown size={14} />
            </label>
          </div>
          <form
            className="wr-idea"
            onSubmit={(e) => {
              e.preventDefault();
              start();
            }}
          >
            <Sparkles size={21} aria-hidden="true" />
            <label className="sr-only" htmlFor="work-idea">
              What would you like help with?
            </label>
            <textarea
              id="work-idea"
              value={idea}
              onChange={(e) => setIdea(e.target.value)}
              placeholder="What would you like help with? Start with a thought…"
              rows={1}
            />
            <button className="wr-primary" disabled={!idea.trim()}>
              Shape this work <ArrowRight size={16} />
            </button>
          </form>
        </div>
        <div className="wr-reading-note">
          <span>Design preview · illustrative work · nothing is running</span>
          <span>Changes stay in this tab</span>
        </div>
        <div className="wr-section-nav" aria-label="Work filters">
          <button aria-pressed={lens === 'all'} onClick={() => setLens('all')}>
            All work <span>{scoped.length}</span>
          </button>
          <button aria-pressed={lens === 'review'} onClick={() => setLens('review')}>
            For you <span>{forYou.length}</span>
          </button>
          {outside > 0 && (
            <button
              className="wr-outside"
              onClick={() => {
                setContext('all');
                setLens('review');
              }}
            >
              {outside} more in other contexts <ArrowUpRight size={14} />
            </button>
          )}
        </div>
        <section className="wr-section" aria-label="Work needing your judgment">
          <div className="wr-section-heading">
            <h2>{forYou.length ? 'Your judgment' : 'No judgment requested here.'}</h2>
            <span>
              {forYou.length ? 'Prepared for your review' : 'Based on the loaded examples'}
            </span>
          </div>
          {forYou.length ? (
            forYou.map((item) => (
              <WorkRow
                key={item.id}
                item={item}
                agents={agents}
                onOpen={() => select(item.id)}
                attention
              />
            ))
          ) : (
            <p className="wr-empty">
              You can review the ongoing work below. This preview does not establish live health.
            </p>
          )}
        </section>
        {lens === 'all' && (
          <section className="wr-section" aria-label="Other work">
            <div className="wr-section-heading">
              <h2>Other work</h2>
              <span>Assignments, responsibilities & responses</span>
            </div>
            {others.map((item) => (
              <WorkRow key={item.id} item={item} agents={agents} onOpen={() => select(item.id)} />
            ))}
            {!others.length && (
              <p className="wr-empty">
                Nothing else in this context yet. Start with an idea above.
              </p>
            )}
          </section>
        )}
        <footer className="wr-footer">
          <span>You set the direction. Each team has boundaries.</span>
          <button onClick={() => onMap('all')}>
            Explore the organization <ArrowUpRight size={15} />
          </button>
        </footer>
      </div>
      {selected && (
        <WorkDetail
          key={selected.id}
          item={selected}
          dispatch={dispatch}
          agents={agents}
          teams={teams}
          onBack={() => onSelect(null)}
          onAgent={onAgent}
          onMap={onMap}
          onTeam={onTeam}
          draft={decisionDrafts[selected.id] || { optionId: '', note: '' }}
          onDraft={(draft) => setDecisionDrafts((old) => ({ ...old, [selected.id]: draft }))}
        />
      )}
    </div>
  );
}

function WorkRow({
  item,
  agents,
  onOpen,
  attention = false,
}: {
  item: WorkItem;
  agents: Agent[];
  onOpen: () => void;
  attention?: boolean;
}) {
  const Icon = kindIcons[item.kind],
    lead = agents.find((a) => a.id === item.leadId);
  return (
    <button
      className={'wr-row' + (attention ? ' wr-row-attention' : '')}
      data-status={item.status}
      onClick={onOpen}
      aria-label={`Open ${item.title}`}
    >
      <span className="wr-row-symbol">
        <Icon size={20} strokeWidth={1.5} />
      </span>
      <span className="wr-row-main">
        <span className="wr-meta">
          {item.context} <span> / </span> {kindLabels[item.kind]}
        </span>
        <strong>{item.title || 'Untitled work'}</strong>
        <span className="wr-row-summary">{item.summary}</span>
        {attention && (
          <span className="wr-row-question">
            {item.status === 'review' ? 'Inspect the reported result' : item.decision?.question}{' '}
            <ArrowUpRight size={14} />
          </span>
        )}
      </span>
      <span className="wr-row-end">
        <span className={'wr-status status-' + item.status}>
          <span aria-hidden="true" />
          {statusLabels[item.status]}
        </span>
        {lead && (
          <span className="wr-owner">
            <Portrait agent={lead} size={24} />
            {teammate(lead).name}
            <span className="sr-only">, lead</span>
          </span>
        )}
      </span>
      <ArrowRight size={18} className="wr-row-arrow" />
    </button>
  );
}

function WorkDetail({
  item,
  dispatch,
  agents,
  teams,
  onBack,
  onAgent,
  onMap,
  onTeam,
  draft,
  onDraft,
}: {
  item: WorkItem;
  dispatch: Dispatch<WorkAction>;
  agents: Agent[];
  teams: Team[];
  onBack: () => void;
  onAgent: (id: string) => void;
  onMap: (teamId: string) => void;
  onTeam?: (teamId: string, workId: string) => void;
  draft: DecisionDraft;
  onDraft: (draft: DecisionDraft) => void;
}) {
  const [editing, setEditing] = useState(item.status === 'draft');
  const [thought, setThought] = useState('');
  const [thoughtType, setThoughtType] = useState('Open question');
  const [investigation, setInvestigation] = useState('');
  const [question, setQuestion] = useState('');
  const lead = agents.find((a) => a.id === item.leadId);
  const team = teams.find((t) => t.id === item.teamId);
  const patch = (patch: Partial<WorkItem>) => dispatch({ type: 'edit', id: item.id, patch });
  const decision = item.decision;
  return (
    <article className="wr-page wr-detail">
      <div className="wr-detail-nav">
        <button onClick={onBack}>
          <ArrowLeft size={16} /> Back to work
        </button>
        <span>
          {item.fixture ? 'Illustrative scenario' : 'Your work'} ·{' '}
          {item.status === 'draft' ? 'draft saved in this tab' : 'preview only'}
        </span>
      </div>
      <div className="wr-detail-heading">
        <div>
          <span className="wr-eyebrow">
            {item.context} / {kindLabels[item.kind]}
          </span>
          <h1 tabIndex={-1}>{item.title || 'Untitled work'}</h1>
          {item.intent !== item.title && (
            <p>{item.intent || 'Start by describing what you want to make possible.'}</p>
          )}
        </div>
        <span className={'wr-status status-' + item.status}>
          <span aria-hidden="true" />
          {statusLabels[item.status]}
        </span>
      </div>
      <div className="wr-detail-grid">
        <div className="wr-main-column">
          {editing ? (
            <ConversationStart
              item={item}
              agents={agents}
              teams={teams}
              onChange={patch}
              onDone={() => setEditing(false)}
              onDispatch={() => {
                dispatch({ type: 'dispatch', id: item.id });
                setEditing(false);
              }}
            />
          ) : (
            <>
              {decision && item.status === 'needs_input' && (
                <section className="wr-decision" aria-label="Decision brief">
                  <span className="wr-eyebrow">A decision for you</span>
                  <h2>{decision.question}</h2>
                  <p>{decision.whyYou}</p>
                  <div className="wr-known">
                    <div>
                      <h3>What we know</h3>
                      <p>{decision.known}</p>
                    </div>
                    <div>
                      <h3>What’s uncertain</h3>
                      <p>{decision.unknown}</p>
                    </div>
                  </div>
                  <details className="wr-disclosure">
                    <summary>
                      Review the supporting evidence · {item.evidence.length} records
                    </summary>
                    {item.evidence.map((record) => (
                      <div key={record.title}>
                        <h3>{record.title}</h3>
                        <p>{record.text}</p>
                      </div>
                    ))}
                  </details>
                  <fieldset className="wr-options">
                    <legend>Choose a direction</legend>
                    {decision.options.map((option) => (
                      <label
                        key={option.id}
                        className={draft.optionId === option.id ? 'is-chosen' : ''}
                      >
                        <input
                          type="radio"
                          name={`decision-${item.id}`}
                          checked={draft.optionId === option.id}
                          onChange={() => onDraft({ ...draft, optionId: option.id })}
                        />
                        <span>
                          <strong>{option.title}</strong>
                          {option.recommended && <small>Team recommendation</small>}
                          <span>{option.consequence}</span>
                        </span>
                      </label>
                    ))}
                  </fieldset>
                  <label className="wr-field">
                    Add a condition or context <span className="wr-optional">optional</span>
                    <textarea
                      rows={2}
                      placeholder="Anything the team should preserve or take into account…"
                      value={draft.note}
                      onChange={(e) => onDraft({ ...draft, note: e.target.value })}
                    />
                  </label>
                  <div className="wr-decision-submit">
                    <button
                      className="wr-primary"
                      disabled={!draft.optionId}
                      onClick={() =>
                        dispatch({
                          type: 'decide',
                          id: item.id,
                          optionId: draft.optionId,
                          note: draft.note,
                        })
                      }
                    >
                      Record direction <ArrowRight size={16} />
                    </button>
                    <span>Saved here. No live action is sent.</span>
                  </div>
                  <details className="wr-disclosure">
                    <summary>What happens if I wait?</summary>
                    <p>{decision.fallback}</p>
                  </details>
                  <details className="wr-disclosure">
                    <summary>I need more context first</summary>
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        if (question.trim()) {
                          setInvestigation(question.trim());
                          patch({
                            notes: [...item.notes, `Investigation requested: ${question.trim()}`],
                          });
                          setQuestion('');
                        }
                      }}
                    >
                      <label className="wr-field">
                        What should the team investigate?
                        <textarea
                          rows={2}
                          value={question}
                          onChange={(e) => setQuestion(e.target.value)}
                        />
                      </label>
                      <button className="wr-secondary" disabled={!question.trim()}>
                        Save investigation request
                      </button>
                      <p>
                        No response will be generated in this preview. The original decision remains
                        open.
                      </p>
                    </form>
                  </details>
                  {investigation && (
                    <p className="wr-inline-receipt" role="status">
                      Request saved: {investigation}. Awaiting a response.
                    </p>
                  )}
                </section>
              )}
              {item.receipt && (
                <section className="wr-receipt" aria-label="Direction receipt">
                  <span className="wr-eyebrow">
                    {item.receipt.phase === 'recorded'
                      ? 'Direction recorded'
                      : 'Simulated follow-through'}
                  </span>
                  <h2>{item.receipt.choice}</h2>
                  {item.receipt.note && (
                    <>
                      <blockquote>{item.receipt.note}</blockquote>
                      <small>
                        Your added conditions are preserved here. Scripted sample updates do not
                        evaluate them.
                      </small>
                    </>
                  )}
                  <ol className="wr-receipt-steps">
                    <li className="reached">
                      <Check size={15} /> Recorded
                    </li>
                    <li className={item.receipt.phase !== 'recorded' ? 'reached' : ''}>
                      {item.receipt.phase !== 'recorded' ? (
                        <Check size={15} />
                      ) : (
                        <Circle size={13} />
                      )}{' '}
                      Acknowledged
                    </li>
                    <li className={item.receipt.phase === 'reported' ? 'reached' : ''}>
                      {item.receipt.phase === 'reported' ? (
                        <Check size={15} />
                      ) : (
                        <Circle size={13} />
                      )}{' '}
                      Result reported
                    </li>
                  </ol>
                  <p>{item.summary}</p>
                  <small>
                    {item.receipt.phase === 'reported'
                      ? 'A sample report is ready for inspection. It has not been independently verified.'
                      : 'Nothing has run in a connected system. Recorded direction is not completed work.'}
                  </small>
                  {item.fixture && item.receipt.phase !== 'reported' && (
                    <button
                      className="wr-secondary"
                      onClick={() => dispatch({ type: 'advance', id: item.id })}
                    >
                      Load{' '}
                      {item.receipt.phase === 'recorded'
                        ? 'sample acknowledgment'
                        : 'sample result'}{' '}
                      <ArrowRight size={15} />
                    </button>
                  )}
                </section>
              )}
              {!item.receipt && item.status !== 'needs_input' && (
                <section className="wr-current">
                  <span className="wr-eyebrow">Where things stand</span>
                  <h2>
                    {item.status === 'paused'
                      ? 'This responsibility is paused in the preview.'
                      : item.summary}
                  </h2>
                  <p>{item.next}</p>
                  {item.kind === 'responsibility' && (
                    <div className="wr-loop">
                      <RefreshCw size={21} />
                      <div>
                        <strong>{item.trigger}</strong>
                        <span>{item.cadence}</span>
                      </div>
                      <span className="wr-loop-return">Receive → work → check → repeat</span>
                    </div>
                  )}
                </section>
              )}
              {item.milestones.length > 0 && (
                <section className="wr-progress" aria-label="Work checkpoints">
                  <div className="wr-section-heading">
                    <h2>The thread of the work</h2>
                    <span>
                      {item.receipt?.phase === 'reported'
                        ? 'Original plan · latest result above'
                        : 'Checkpoints, not activity counts'}
                    </span>
                  </div>
                  <ol>
                    {item.milestones.map((step, i) => (
                      <li key={step.title} className={'step-' + step.state}>
                        <span className="wr-step-icon">
                          {step.state === 'done' ? (
                            <Check size={15} />
                          ) : (
                            String(i + 1).padStart(2, '0')
                          )}
                        </span>
                        <div>
                          <strong>{step.title}</strong>
                          <p>{step.detail}</p>
                          <small>
                            {step.state === 'done'
                              ? 'Recorded in the sample'
                              : step.state === 'current'
                                ? 'Current checkpoint in the original plan'
                                : 'Next in the original plan'}
                          </small>
                        </div>
                      </li>
                    ))}
                  </ol>
                </section>
              )}
              <section className="wr-brief-read">
                <div className="wr-section-heading">
                  <h2>What useful looks like</h2>
                  <button onClick={() => setEditing(true)}>
                    Refine brief <ArrowUpRight size={14} />
                  </button>
                </div>
                <p>{item.success || 'No success criteria recorded yet.'}</p>
                {item.clarifications.length > 0 && (
                  <details className="wr-disclosure">
                    <summary>Additional direction · {item.clarifications.length}</summary>
                    {item.clarifications.map((text, i) => (
                      <p key={i}>{text}</p>
                    ))}
                  </details>
                )}
              </section>
            </>
          )}
          <section className="wr-thinking" aria-label="Thinking space">
            <div className="wr-section-heading">
              <h2>Room to think</h2>
              <span>Ideas don’t change instructions</span>
            </div>
            <p>
              Keep alternatives and questions here. Move an idea into the brief when you decide to
              pursue it.
            </p>
            {item.notes.length > 0 && (
              <ul>
                {item.notes.map((note, i) => (
                  <li key={i}>{note}</li>
                ))}
              </ul>
            )}
            <form
              onSubmit={(e) => {
                e.preventDefault();
                if (!thought.trim()) return;
                patch({ notes: [...item.notes, `${thoughtType}: ${thought.trim()}`] });
                setThought('');
              }}
            >
              <label className="sr-only" htmlFor="thought-kind">
                Thought type
              </label>
              <select
                id="thought-kind"
                value={thoughtType}
                onChange={(e) => setThoughtType(e.target.value)}
              >
                <option>Open question</option>
                <option>Possible approach</option>
                <option>Constraint to consider</option>
              </select>
              <label className="sr-only" htmlFor="thought-text">
                Add a thought
              </label>
              <textarea
                id="thought-text"
                rows={2}
                value={thought}
                onChange={(e) => setThought(e.target.value)}
                placeholder="What else could we try? What are we missing?"
              />
              <button className="wr-secondary" disabled={!thought.trim()}>
                Keep this thought <Plus size={15} />
              </button>
            </form>
          </section>
        </div>
        <aside className="wr-sidebar" aria-label="Responsibility and evidence">
          <section>
            <span className="wr-eyebrow">People behind the work</span>
            <h2>{item.teamId ? team?.name || 'Assigned team' : 'Task force'}</h2>
            {team && onTeam && (
              <button className="wr-link" onClick={() => onTeam(team.id, item.id)}>
                Continue in the team conversation <ArrowUpRight size={14} />
              </button>
            )}
            {item.agentIds.length ? (
              <ul className="wr-people">
                {item.agentIds.map((id) => {
                  const agent = agents.find((a) => a.id === id);
                  return agent ? (
                    <li key={id}>
                      <button onClick={() => onAgent(id)}>
                        <Portrait agent={agent} size={34} />
                        <span>
                          <strong>{teammate(agent).name}</strong>
                          <small>{id === item.leadId ? 'Accountable lead' : 'Contributor'}</small>
                        </span>
                        <ArrowUpRight size={14} />
                      </button>
                    </li>
                  ) : null;
                })}
              </ul>
            ) : (
              <p>Choose a lead and contributors in the brief.</p>
            )}
            {!!item.agentIds.length && (
              <button className="wr-link" onClick={() => onMap(item.teamId || 'all')}>
                Explore relationships <ArrowUpRight size={14} />
              </button>
            )}
            {lead && (
              <p className="wr-small">
                Shared people keep one identity. Availability and competing commitments are not
                modeled in this preview.
              </p>
            )}
          </section>
          <section>
            <span className="wr-eyebrow">Operating agreement</span>
            <h3>When to involve you</h3>
            <p>{item.boundary}</p>
            <h3>Check in</h3>
            <p>{item.cadence}</p>
            {item.kind === 'responsibility' && ['watching', 'paused'].includes(item.status) && (
              <>
                <button
                  className="wr-secondary"
                  onClick={() => dispatch({ type: 'pause', id: item.id })}
                >
                  {item.status === 'paused' ? <Play size={14} /> : <Pause size={14} />}{' '}
                  {item.status === 'paused' ? 'Resume sample loop' : 'Pause sample loop'}
                </button>
                <small className="wr-small">
                  Changes this preview only. No connected process is affected.
                </small>
              </>
            )}
          </section>
          {item.evidence.length > 0 && (
            <section>
              <span className="wr-eyebrow">Evidence & context</span>
              <p className="wr-small">Illustrative records, available when you need the detail.</p>
              {item.evidence.map((record) => (
                <details className="wr-evidence" key={record.title}>
                  <summary>
                    {record.title}
                    <Plus size={14} />
                  </summary>
                  <p>{record.text}</p>
                </details>
              ))}
            </section>
          )}
        </aside>
      </div>
    </article>
  );
}

function ConversationStart({
  item,
  agents,
  teams,
  onChange,
  onDone,
  onDispatch,
}: {
  item: WorkItem;
  agents: Agent[];
  teams: Team[];
  onChange: (patch: Partial<WorkItem>) => void;
  onDone: () => void;
  onDispatch: () => void;
}) {
  const [message, setMessage] = useState('');
  const [agreementOpen, setAgreementOpen] = useState(false);
  const ready = !!(
    item.title.trim() &&
    item.intent.trim() &&
    item.success.trim() &&
    item.boundary.trim() &&
    item.leadId &&
    item.agentIds.includes(item.leadId) &&
    (item.kind === 'assignment' || item.trigger.trim())
  );
  const draft = item.status === 'draft';
  return (
    <section className="wr-start" aria-label="Shape together">
      <span className="wr-eyebrow">Start rough. Find the shape together.</span>
      <h2>{draft ? 'An idea is enough to begin.' : 'Where should we take this?'}</h2>
      <p>
        {draft
          ? 'The team can help turn this into a plan. Add what you know, or let them explore a starting point.'
          : 'Add context or a change in direction. You can refine the agreement when you need more control.'}
      </p>
      <div className="wr-start-agreement">
        <span className="wr-eyebrow">The first step</span>
        <p>{item.success}</p>
        <span>{item.boundary}</span>
      </div>
      {item.clarifications.length > 0 && (
        <div className="wr-clarifications" aria-label="Additional direction">
          {item.clarifications.map((text, i) => (
            <div key={i}>
              <small>You</small>
              <p>{text}</p>
            </div>
          ))}
        </div>
      )}
      <div className="wr-start-team">
        <label>
          Start with{' '}
          <select
            aria-label="Starting team"
            value={item.teamId}
            onChange={(e) => {
              const ids =
                teams
                  .find((t) => t.id === e.target.value)
                  ?.pledgedAgentIds.filter((id) => agents.some((a) => a.id === id)) ||
                item.agentIds.slice(0, 3);
              onChange({ teamId: e.target.value, agentIds: ids, leadId: ids[0] || '' });
            }}
          >
            <option value="">A task force</option>
            {teams.map((team) => (
              <option key={team.id} value={team.id}>
                {team.name}
              </option>
            ))}
          </select>
        </label>
        <span>You can change this.</span>
      </div>
      <div className="wr-editor-footer">
        <button
          className="wr-primary"
          disabled={draft && !ready}
          onClick={() => {
            if (message.trim()) {
              onChange({ clarifications: [...item.clarifications, message.trim()] });
              setMessage('');
            }
            if (draft) onDispatch();
            else onDone();
          }}
        >
          {draft ? 'Send to team in preview' : 'Keep this direction'} <ArrowRight size={16} />
        </button>
        <span>
          {!ready && draft
            ? 'Choose a team or lead; check the optional agreement if you changed its starting conditions.'
            : 'A local handoff only. No engine is connected.'}
        </span>
      </div>
      <form
        className="wr-conversation-input"
        onSubmit={(e) => {
          e.preventDefault();
          if (message.trim()) {
            onChange({ clarifications: [...item.clarifications, message.trim()] });
            setMessage('');
          }
        }}
      >
        <label className="sr-only" htmlFor="work-context-message">
          Add context to the work
        </label>
        <textarea
          id="work-context-message"
          rows={2}
          placeholder="A constraint, a question, a direction you’re considering…"
          value={message}
          onChange={(e) => setMessage(e.target.value)}
        />
        <button className="wr-secondary" disabled={!message.trim()}>
          Add context <Plus size={15} />
        </button>
      </form>
      <details
        className="wr-disclosure wr-optional-agreement"
        onToggle={(e) => setAgreementOpen(e.currentTarget.open)}
      >
        <summary>
          Adjust the starting agreement <span>optional</span>
        </summary>
        {agreementOpen && (
          <BriefEditor
            item={item}
            agents={agents}
            teams={teams}
            onChange={onChange}
            onDone={onDone}
            onDispatch={onDispatch}
            hideActions
          />
        )}
      </details>
    </section>
  );
}

function BriefEditor({
  item,
  agents,
  teams,
  onChange,
  onDone,
  onDispatch,
  hideActions = false,
}: {
  item: WorkItem;
  agents: Agent[];
  teams: Team[];
  onChange: (patch: Partial<WorkItem>) => void;
  onDone: () => void;
  onDispatch: () => void;
  hideActions?: boolean;
}) {
  const draft = item.status === 'draft';
  const ready =
    item.title.trim() &&
    item.intent.trim() &&
    item.success.trim() &&
    item.boundary.trim() &&
    item.leadId &&
    item.agentIds.includes(item.leadId) &&
    (item.kind === 'assignment' || item.trigger.trim());
  function chooseTeam(id: string) {
    const ids =
      teams
        .find((t) => t.id === id)
        ?.pledgedAgentIds.filter((id) => agents.some((a) => a.id === id)) || [];
    onChange({ teamId: id, agentIds: ids, leadId: ids[0] || '' });
  }
  return (
    <section className="wr-editor" aria-label="Work brief">
      <span className="wr-eyebrow">Shape the work</span>
      <h2>A clear starting point.</h2>
      <p>Enough direction to move independently. Enough boundaries to keep you in control.</p>
      <label className="wr-field">
        Give it a name
        <input value={item.title} onChange={(e) => onChange({ title: e.target.value })} />
      </label>
      <div className="wr-form-pair">
        <label className="wr-field">
          Context
          <input
            value={item.context}
            placeholder="A product, business, classroom, or part of life"
            onChange={(e) => onChange({ context: e.target.value })}
          />
        </label>
        <label className="wr-field">
          How should it work?
          <select
            value={item.kind}
            onChange={(e) => onChange({ kind: e.target.value as WorkKind })}
          >
            <option value="assignment">An assignment with a result</option>
            <option value="responsibility">An ongoing responsibility</option>
            <option value="response">A response to an event</option>
          </select>
        </label>
      </div>
      <label className="wr-field">
        What are we trying to make possible?
        <textarea
          rows={3}
          value={item.intent}
          onChange={(e) => onChange({ intent: e.target.value })}
        />
      </label>
      {item.kind !== 'assignment' && (
        <label className="wr-field">
          What starts the work?
          <input
            value={item.trigger}
            placeholder="A new request, a scheduled check-in, or a threshold…"
            onChange={(e) => onChange({ trigger: e.target.value })}
          />
        </label>
      )}
      <label className="wr-field">
        How will we know it’s useful?
        <textarea
          rows={2}
          value={item.success}
          placeholder="The result or condition you want to verify…"
          onChange={(e) => onChange({ success: e.target.value })}
        />
      </label>
      <label className="wr-field">
        What can the team do, and when should it ask?
        <textarea
          rows={3}
          value={item.boundary}
          onChange={(e) => onChange({ boundary: e.target.value })}
        />
      </label>
      <label className="wr-field">
        When would you like a check-in?
        <input value={item.cadence} onChange={(e) => onChange({ cadence: e.target.value })} />
      </label>
      <div className="wr-form-pair">
        <label className="wr-field">
          Who takes this on?
          <select value={item.teamId} onChange={(e) => chooseTeam(e.target.value)}>
            <option value="">Build a task force</option>
            {teams.map((team) => (
              <option key={team.id} value={team.id}>
                {team.name}
              </option>
            ))}
          </select>
        </label>
        <label className="wr-field">
          Accountable lead
          <select
            value={item.leadId}
            onChange={(e) =>
              onChange({
                leadId: e.target.value,
                agentIds: [...new Set([...item.agentIds, e.target.value])].filter(Boolean),
              })
            }
          >
            <option value="">Choose a lead</option>
            {agents.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {teammate(agent).name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <details className="wr-disclosure">
        <summary>Choose contributors · {item.agentIds.length} selected</summary>
        <div className="wr-contributors">
          {agents.map((agent) => (
            <label key={agent.id}>
              <input
                type="checkbox"
                checked={item.agentIds.includes(agent.id)}
                disabled={item.leadId === agent.id}
                onChange={(e) =>
                  onChange({
                    agentIds: e.target.checked
                      ? [...item.agentIds, agent.id]
                      : item.agentIds.filter((id) => id !== agent.id),
                  })
                }
              />
              {teammate(agent).name}
              {item.leadId === agent.id ? ' · lead' : ''}
            </label>
          ))}
        </div>
      </details>
      {!hideActions && (
        <div className="wr-editor-footer">
          <button
            className="wr-primary"
            disabled={draft && !ready}
            onClick={draft ? onDispatch : onDone}
          >
            {draft ? 'Dispatch in preview' : 'Done shaping'} <ArrowRight size={16} />
          </button>
          <span>
            {draft
              ? ready
                ? 'Ready for a local handoff. No team will run yet.'
                : 'Add a useful result, operating boundaries, a trigger if needed, and a lead.'
              : 'Changes are saved locally; they have not been sent to a team.'}
          </span>
        </div>
      )}
    </section>
  );
}
