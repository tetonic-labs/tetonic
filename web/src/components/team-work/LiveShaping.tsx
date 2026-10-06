import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope, taskIsActive, type PlanView } from '../../lib/localEngine';
import { workRecords, stateLabel } from '../../lib/workspaceRecords';
import { useWorkspaceDraft } from '../workspace/useWorkspaceDraft';
import { WorkingBrief } from '../workspace/WorkingBrief';
import { PlanReview } from './PlanReview';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { Portrait } from '../ui/Portrait';
import './shaping.css';

// Shaping uses the same authorized engine and durable work as the map.
type ShapingProps = {
  workId?: string;
  onWork?: (id: string, inspect?: boolean) => void;
  onSelected?: (id: string) => void;
};
export function LiveShaping(props: ShapingProps) {
  useLocalEngine(); // Remount drafts if the authenticated destination changes.
  return <ConnectedShaping key={connectionDraftScope()} {...props} />;
}

function ConnectedShaping({ workId, onWork, onSelected }: ShapingProps) {
  const engine = useLocalEngine();
  const { workspace, uiAgents, isConnected, isConnecting, submitTask, cancelTask } = engine;
  const records = workRecords(workspace?.tasks || [], engine.workItems).filter(
    (work) => work.latest?.purpose === 'explore',
  );
  const [selectedId, setSelectedId] = useState(
    () => workId || new URLSearchParams(location.hash.slice(1)).get('shape') || '',
  );
  const [tab, setTab] = useState<'conversation' | 'brief' | 'plan'>(() =>
    workId && workspace?.tasks.some((t) => t.plan?.source_work_id === workId)
      ? 'plan'
      : 'conversation',
  );
  const [planView, setPlanView] = useState<PlanView>();
  const choseTab = useRef(false);
  const viewChanged = useCallback((value: PlanView) => {
    setPlanView(value);
    if (!choseTab.current && (value.plans.length || value.execution)) setTab('plan');
  }, []);
  const [stopError, setStopError] = useState('');
  const [stopping, setStopping] = useState(false);
  const stopGate = useRef(false);
  const selected = records.find((work) => work.id === selectedId);
  const missing = !!selectedId && !selected && !!workspace;
  const latest = selected?.latest;
  const active = !!latest && taskIsActive(latest);
  const guideKey = latest?.agent_key || workspace?.shaping_agent_key || '';
  const guide = uiAgents.find((agent) => agent.id === guideKey);
  const writer = useWorkspaceDraft(`team-shaping:${connectionDraftScope()}`);
  const draftKey = selectedId || 'new';
  const draft = writer.drafts[draftKey] || { text: '' };
  const currentKey = useRef(draftKey);
  currentKey.current = draftKey;
  const thread = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const bytes = new TextEncoder().encode(draft.text.trim()).length;
  const canSend =
    isConnected &&
    !!guideKey &&
    !missing &&
    !writer.busyKey &&
    (!active || !!draft.pending) &&
    latest?.state !== 'recovery_required' &&
    !!draft.text.trim() &&
    bytes <= (workspace?.input_limit || 12000);

  function select(id: string) {
    setSelectedId(id);
    setStopError('');
    setTab('conversation');
    choseTab.current = false;
    following.current = true;
    history.replaceState(
      null,
      '',
      `${location.pathname}${location.search}${id ? `#shape=${encodeURIComponent(id)}` : ''}`,
    );
    onSelected?.(id);
  }

  useEffect(() => {
    if (thread.current && tab === 'conversation' && following.current)
      thread.current.scrollTop = thread.current.scrollHeight;
  }, [latest?.sequence, latest?.id, selectedId, tab]);

  useEffect(() => {
    if (thread.current && tab === 'brief') thread.current.scrollTop = 0;
  }, [selectedId, tab]);

  async function send() {
    if (!canSend) return;
    const submittedKey = draftKey;
    await writer.send(
      draftKey,
      guideKey,
      latest?.id,
      submitTask,
      (task) => {
        if (currentKey.current !== submittedKey) return;
        if (!selectedId) select(task.id);
        else setTab('conversation');
        following.current = true;
      },
      'explore',
    );
  }
  async function stop() {
    if (!latest || !isConnected || stopGate.current) return;
    stopGate.current = true;
    setStopping(true);
    setStopError('');
    try {
      const receipt = await cancelTask(latest.id);
      if (!['canceling', 'canceled', 'completed', 'failed'].includes(receipt.state))
        throw new Error('The stop has not been confirmed. This reply may still be running.');
    } catch (error) {
      setStopError(error instanceof Error ? error.message : 'The stop has not been confirmed.');
    } finally {
      stopGate.current = false;
      setStopping(false);
    }
  }

  if (missing && workspace?.tasks.some((task) => task.plan?.source_work_id === selectedId))
    return (
      <section className="px-shaping" aria-label="Team work">
        <div className="px-shaping-body">
          <PlanReview workId={selectedId} onWork={onWork} />
        </div>
      </section>
    );

  return (
    <section className="px-shaping" aria-label="Shape work with the Guide">
      {tab === 'conversation' && (selected || !isConnected) && (
        <div className="px-shaping-guide">
          {guide && <Portrait agent={guide} size={32} square={false} />}
          <span>
            <strong>{guide?.name || 'The Guide'}</strong>
            <small>
              {!isConnected
                ? isConnecting
                  ? 'Connecting…'
                  : 'Engine disconnected'
                : selected
                  ? planView?.execution
                    ? 'Original discussion'
                    : stateLabel(selected)
                  : 'A place to think before putting work in motion.'}
            </small>
          </span>
          {active && (
            <button
              onClick={() => void stop()}
              disabled={!isConnected || stopping || latest?.state === 'canceling'}
              aria-label="Stop this exploration reply"
            >
              <Square size={12} />
              {stopping || latest?.state === 'canceling' ? 'Stopping…' : 'Stop reply'}
            </button>
          )}
        </div>
      )}
      {selected && (
        <div className="px-shaping-tabs" role="tablist" aria-label="Shaping details">
          {(['plan', 'conversation', 'brief'] as const).map((name, index) => (
            <button
              key={name}
              id={`shape-tab-${name}`}
              role="tab"
              aria-selected={tab === name}
              aria-controls={`shape-panel-${name}`}
              tabIndex={tab === name ? 0 : -1}
              onClick={() => {
                choseTab.current = true;
                following.current = true;
                setTab(name);
              }}
              onKeyDown={(event) => {
                if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
                  event.preventDefault();
                  const tabs = ['plan', 'conversation', 'brief'] as const;
                  const next =
                    tabs[
                      event.key === 'Home'
                        ? 0
                        : event.key === 'End'
                          ? 2
                          : (index + (event.key === 'ArrowLeft' ? 2 : 1)) % 3
                    ];
                  following.current = true;
                  choseTab.current = true;
                  setTab(next);
                  document.getElementById(`shape-tab-${next}`)?.focus();
                }
              }}
            >
              {name === 'conversation'
                ? 'Discussion'
                : name === 'brief'
                  ? 'Working brief'
                  : planView?.execution
                    ? 'Overview'
                    : 'Work plan'}
            </button>
          ))}
        </div>
      )}
      <div
        className="px-shaping-body"
        ref={thread}
        onScroll={() => {
          const element = thread.current;
          if (element)
            following.current =
              element.scrollHeight - element.scrollTop - element.clientHeight < 64;
        }}
      >
        {selected ? (
          <>
            <div
              id="shape-panel-conversation"
              role="tabpanel"
              aria-labelledby="shape-tab-conversation"
              hidden={tab !== 'conversation'}
            >
              {selected.turns.map((turn) => (
                <article className="px-shaping-turn" key={turn.id}>
                  <div className="px-shaping-human">
                    <strong>You</strong>
                    <p>{turn.input}</p>
                  </div>
                  {turn.messages
                    .filter((message) => message.role === 'assistant')
                    .map((message) => (
                      <div className="px-shaping-answer" key={message.id}>
                        <strong>{turn.agent_name}</strong>
                        <FormattedMarkdown text={message.content} />
                      </div>
                    ))}
                  {taskIsActive(turn) && (
                    <p className="px-shaping-status" role="status">
                      {isConnected
                        ? 'Thinking it through…'
                        : 'Connection lost. The reply may still be running.'}
                    </p>
                  )}
                  {turn.error && <p role="alert">{turn.error}</p>}
                  {turn.state === 'canceled' && <p>Reply stopped. Your discussion is saved.</p>}
                  {turn.state === 'recovery_required' && (
                    <p>
                      This reply was interrupted. Earlier discussion is saved; start a new
                      exploration to continue.
                    </p>
                  )}
                </article>
              ))}
              {!active && !planView?.execution && (
                <button
                  className="tw-next-step"
                  onClick={() => {
                    choseTab.current = true;
                    setTab('brief');
                  }}
                >
                  Keep the decisions in a brief <span>Then shape a plan for your team →</span>
                </button>
              )}
            </div>
            <div
              id="shape-panel-brief"
              role="tabpanel"
              aria-labelledby="shape-tab-brief"
              hidden={tab !== 'brief'}
            >
              {tab === 'brief' && planView?.execution && (
                <p className="tw-small">
                  This brief records the original direction. Changes to upcoming assignments are
                  handled in Overview.
                </p>
              )}
              {tab === 'brief' && (
                <WorkingBrief
                  key={selected.id}
                  workId={selected.id}
                  onContinue={() => {
                    choseTab.current = true;
                    setTab('plan');
                  }}
                  suggestion={
                    latest?.messages.filter((message) => message.role === 'assistant').at(-1)
                      ?.content
                  }
                />
              )}
            </div>
            <div
              id="shape-panel-plan"
              role="tabpanel"
              aria-labelledby="shape-tab-plan"
              hidden={tab !== 'plan'}
            >
              <PlanReview
                key={selected.id}
                workId={selected.id}
                onView={viewChanged}
                onWork={onWork}
                onBrief={() => {
                  choseTab.current = true;
                  setTab('brief');
                }}
              />
            </div>
          </>
        ) : (
          <div className="px-shaping-empty">
            {missing && <h3>This discussion is unavailable.</h3>}
            <p>
              {missing
                ? 'Open Work to choose a saved discussion, or start something new.'
                : 'You don’t need a finished plan. Explore the options with the Guide, decide what matters, then shape the work for your team.'}
            </p>
          </div>
        )}
      </div>
      {tab === 'conversation' && (
        <form
          className="px-shaping-composer"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <label htmlFor="shaping-message">
            {selected ? 'Continue shaping' : 'What are you working through?'}
          </label>
          <div>
            <textarea
              id="shaping-message"
              value={draft.text}
              onChange={(event) => writer.edit(draftKey, event.target.value)}
              readOnly={!!writer.busyKey || (!!draft.pending && !draft.editable)}
              rows={2}
              placeholder={
                selected
                  ? 'Ask a question, share a thought, or change direction…'
                  : 'I’m trying to figure out…'
              }
            />
            <button
              disabled={!canSend}
              aria-label={
                draft.pending
                  ? 'Retry exploration message'
                  : selected
                    ? 'Send exploration reply'
                    : 'Start exploring with the Guide'
              }
            >
              <ArrowUp size={19} />
              <span>{selected ? 'Send' : 'Explore'}</span>
            </button>
          </div>
          <small>
            Discussion only. The Guide cannot use tools or start agents here. Save a working brief,
            then open Work plan to prepare execution.
          </small>
          {bytes > (workspace?.input_limit || 12000) && (
            <p role="alert">This message is too long. Shorten it before sending.</p>
          )}
          {draft.error && (
            <p role="alert">
              {draft.error}{' '}
              {draft.pending && !draft.editable
                ? 'Retry checks the same message without sending it twice.'
                : ''}
            </p>
          )}
          {draft.pending && !draft.error && !writer.busyKey && (
            <p>A previous send was not confirmed. Retry to check the same message.</p>
          )}
          {writer.storageWarning && (
            <p role="alert">
              This tab cannot retain the unsent text. Keep a copy until it is sent.
            </p>
          )}
          {stopError && <p role="alert">{stopError}</p>}
          {!isConnected && !isConnecting && (
            <p role="alert">
              Connect using the local engine’s current connection link, then return here. Nothing is
              sent while disconnected.
            </p>
          )}
        </form>
      )}
    </section>
  );
}
