import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../engine/connection';
import { taskIsActive } from '../../engine/projections/taskState';
import { type EngineTask, type PlanView } from '../../engine/contracts';
import { workRecords, stateLabel, stateLabels } from '../../engine/projections/records';
import { useWorkspaceDraft } from '../workspace/useWorkspaceDraft';
import { WorkingBrief } from '../workspace/WorkingBrief';
import { PlanReview } from './PlanReview';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { Portrait } from '../ui/Portrait';
import { agentSetup } from '../../lib/agentCapabilities';
import './shaping.css';

type ShapingProps = {
  workId?: string;
  onWork?: (id: string, inspect?: boolean) => void;
  onSelected?: (id: string) => void;
  onGuideSettings?: () => void;
  onAgentSettings?: (key: string) => void;
  onTools?: () => void;
};
export function LiveShaping(props: ShapingProps) {
  useLocalEngine();
  return <ConnectedShaping key={connectionDraftScope()} {...props} />;
}

function ConnectedShaping({
  workId,
  onWork,
  onSelected,
  onGuideSettings,
  onAgentSettings,
  onTools,
}: ShapingProps) {
  const engine = useLocalEngine();
  const { workspace, uiAgents, isConnected, isConnecting, submitTask, cancelTask } = engine;
  const records = workRecords(workspace?.tasks || [], engine.workItems).filter(
    (work) => work.latest?.purpose === 'explore',
  );
  const [selectedId, setSelectedId] = useState(
    () => workId || new URLSearchParams(location.hash.slice(1)).get('shape') || '',
  );
  const [planView, setPlanView] = useState<PlanView>();
  const viewChanged = useCallback((value: PlanView) => setPlanView(value), []);
  const [conversationOpen, setConversationOpen] = useState(false);
  const [briefOpen, setBriefOpen] = useState(false);
  const [briefVersion, setBriefVersion] = useState(0);
  const [stopError, setStopError] = useState('');
  const [stopping, setStopping] = useState(false);
  const stopGate = useRef(false);
  const selected = records.find((work) => work.id === selectedId);
  const missing = !!selectedId && !selected && !!workspace;
  const latest = selected?.latest;
  const active = !!latest && taskIsActive(latest);
  const guideKey = latest?.agent_key || workspace?.shaping_agent_key || '';
  const guide = uiAgents.find((agent) => agent.id === guideKey);
  const guideProfile = workspace?.agents.find((agent) => agent.key === guideKey);
  const setup =
    guideProfile &&
    agentSetup(guideProfile, engine.catalog, isConnected && !engine.readErrors['Agent setup']);
  const setupIssue = setup?.state === 'needs_setup' ? setup.message : '';
  const writer = useWorkspaceDraft(`team-shaping:${connectionDraftScope()}`);
  const draftKey = selectedId || 'new';
  const draft = writer.drafts[draftKey] || { text: '' };
  const composer = useRef<HTMLTextAreaElement>(null);
  const hasHandoff = !!planView?.plans[0] || !!planView?.execution;
  const currentKey = useRef(draftKey);
  currentKey.current = draftKey;
  const thread = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const bytes = new TextEncoder().encode(draft.text.trim()).length;
  const canSend =
    isConnected &&
    !!guideKey &&
    (!setupIssue || !!draft.pending) &&
    !missing &&
    !writer.busyKey &&
    (!active || !!draft.pending) &&
    latest?.state !== 'recovery_required' &&
    !!draft.text.trim() &&
    bytes <= (workspace?.input_limit || 12000);
  const suggestion =
    latest?.state === 'completed'
      ? `Goal: ${selected?.turns[0]?.input}\n\n${latest.input !== selected?.turns[0]?.input ? `Latest guidance: ${latest.input}\n\n` : ''}${latest.messages.filter((message) => message.role === 'assistant').at(-1)?.content || ''}`
      : undefined;

  useEffect(() => {
    if (thread.current && following.current && (!hasHandoff || conversationOpen))
      thread.current.scrollTop = thread.current.scrollHeight;
  }, [latest?.sequence, latest?.id, selectedId, hasHandoff, conversationOpen]);
  useEffect(() => {
    if (hasHandoff && thread.current && !conversationOpen) thread.current.scrollTop = 0;
  }, [hasHandoff, conversationOpen]);
  useEffect(() => {
    if (active) setConversationOpen(true);
  }, [active]);

  function discuss(text: string) {
    if (writer.busyKey || (draft.pending && !draft.editable)) return;
    writer.edit(draftKey, draft.text.trim() ? `${draft.text}\n\n${text}` : text);
    setConversationOpen(true);
    composer.current?.focus();
  }
  async function send() {
    if (!canSend) return;
    setConversationOpen(true);
    const submittedKey = draftKey;
    await writer.send(
      draftKey,
      guideKey,
      latest?.id,
      submitTask,
      (task) => {
        if (currentKey.current !== submittedKey) return;
        if (!selectedId) {
          setSelectedId(task.id);
          history.replaceState(
            null,
            '',
            `${location.pathname}${location.search}#shape=${encodeURIComponent(task.id)}`,
          );
          onSelected?.(task.id);
        }
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

  const renderTurn = (turn: EngineTask) => (
    <article className="px-shaping-turn" key={turn.id}>
      <div className="px-shaping-human">
        <strong>You</strong>
        <p>{turn.input}</p>
      </div>
      {turn.messages
        .filter((message) => message.role === 'assistant')
        .slice(-1)
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
          This reply was interrupted. Earlier discussion is saved; start a new conversation to
          continue.
        </p>
      )}
    </article>
  );

  if (missing && workspace?.tasks.some((task) => task.plan?.source_work_id === selectedId))
    return (
      <section className="px-shaping" aria-label="Team work">
        <div className="px-shaping-body">
          <PlanReview
            workId={selectedId}
            onWork={onWork}
            onAgentSettings={onAgentSettings}
            onTools={onTools}
          />
        </div>
      </section>
    );

  return (
    <section className="px-shaping" aria-label="Work with the Guide">
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
                ? active
                  ? 'Thinking it through…'
                  : planView?.execution
                    ? `Team · ${planView.execution.state === 'completed' && !planView.execution.root?.messages.some((message) => message.role === 'assistant' && message.content.trim()) ? 'Execution finished' : stateLabels[planView.execution.state] || planView.execution.state}`
                    : planView?.plans[0]?.content
                      ? planView.readiness.length
                        ? 'A few things to resolve'
                        : 'Proposal ready for review'
                      : stateLabel(selected)
                : 'Think it through. Put your team to work.'}
          </small>
        </span>
        {onGuideSettings && guideProfile && (
          <button
            type="button"
            onClick={onGuideSettings}
            disabled={!isConnected}
            title={`Used for new replies: ${guideProfile.model}`}
          >
            Guide model
          </button>
        )}
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
      {setupIssue && <p role="status">{setupIssue} Open Guide model to review its settings.</p>}
      <div
        className="px-shaping-body"
        data-handoff={hasHandoff}
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
            <div className="tw-conversation-plan" data-handoff={hasHandoff}>
              <PlanReview
                key={`${selected.id}:${briefVersion}`}
                workId={selected.id}
                onView={viewChanged}
                onAgentSettings={onAgentSettings}
                onTools={onTools}
                onDiscuss={
                  writer.busyKey || (draft.pending && !draft.editable) ? undefined : discuss
                }
                onWork={onWork}
                suggestion={suggestion}
                conversationActive={active}
                onBrief={() => setBriefOpen(true)}
              />
              {!!planView?.brief_revision && (
                <details
                  className="tw-saved-direction"
                  open={briefOpen}
                  onToggle={(event) => setBriefOpen(event.currentTarget.open)}
                >
                  <summary>Saved direction & history</summary>
                  {briefOpen && (
                    <WorkingBrief
                      key={selected.id}
                      workId={selected.id}
                      suggestion={suggestion}
                      onContinue={() => {
                        setBriefOpen(false);
                        setBriefVersion((v) => v + 1);
                      }}
                    />
                  )}
                </details>
              )}
            </div>
            {hasHandoff ? (
              <details
                className="tw-handoff-conversation"
                open={conversationOpen}
                onToggle={(event) => setConversationOpen(event.currentTarget.open)}
              >
                <summary>
                  Conversation · {selected.turns.length}{' '}
                  {selected.turns.length === 1 ? 'exchange' : 'exchanges'}
                </summary>
                {selected.turns.map(renderTurn)}
              </details>
            ) : (
              selected.turns.map(renderTurn)
            )}
          </>
        ) : (
          <div className="px-shaping-empty">
            {missing && <h3>This discussion is unavailable.</h3>}
            <p>
              {missing
                ? 'Open Work to choose a saved discussion, or start something new.'
                : 'Bring a question, a rough idea, or a clear goal. We’ll work out the next step together.'}
            </p>
          </div>
        )}
      </div>
      <form
        className="px-shaping-composer"
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <label htmlFor="shaping-message">
          {selected ? 'Continue the conversation' : 'What are you working through?'}
        </label>
        <div>
          <textarea
            id="shaping-message"
            ref={composer}
            value={draft.text}
            onChange={(event) => writer.edit(draftKey, event.target.value)}
            readOnly={!!writer.busyKey || (!!draft.pending && !draft.editable)}
            rows={2}
            placeholder={
              planView?.execution
                ? 'Ask how work is going, or think through what comes next…'
                : 'Ask a question, share a thought, or refine the approach…'
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
            <span>Send</span>
          </button>
        </div>
        {bytes > (workspace?.input_limit || 12000) && (
          <p role="alert">This message is too long. Shorten it before sending.</p>
        )}
        {draft.error && (
          <p role="alert">
            {draft.error}{' '}
            {draft.pending &&
              !draft.editable &&
              'Retry checks the same message without sending it twice.'}
          </p>
        )}
        {draft.pending && !draft.error && !writer.busyKey && (
          <p>A previous send was not confirmed. Retry to check the same message.</p>
        )}
        {writer.storageWarning && (
          <p role="alert">This tab cannot retain the unsent text. Keep a copy until it is sent.</p>
        )}
        {stopError && <p role="alert">{stopError}</p>}
        {!isConnected && !isConnecting && (
          <p role="alert">Reconnect your engine to continue. Your draft stays here.</p>
        )}
      </form>
    </section>
  );
}
