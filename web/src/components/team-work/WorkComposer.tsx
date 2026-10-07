import { useRef, useState } from 'react';
import { ArrowUp } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../lib/localEngine';
import { canReply, type WorkRecord } from '../../lib/workspaceRecords';
import { useWorkspaceDraft } from '../workspace/useWorkspaceDraft';
import { agentSetup } from '../../lib/agentCapabilities';

export function WorkComposer({
  work,
  recipient,
  onAccepted,
  onShape,
  onGuideSettings,
}: {
  work?: WorkRecord;
  recipient?: string;
  onAccepted: (id: string) => void;
  onShape?: (id?: string) => void;
  onGuideSettings?: () => void;
}) {
  const engine = useLocalEngine();
  const [choice, setChoice] = useState(recipient || '');
  const agents = (engine.workspace?.agents || []).filter(
    (agent) => agent.key !== engine.workspace?.shaping_agent_key && !agent.plan_coordinator,
  );
  const key = work ? `work:${work.id}` : 'new';
  const writer = useWorkspaceDraft(`team-work:${connectionDraftScope()}`);
  const draft = writer.drafts[key] || { text: '' };
  const agentKey =
    draft.pending?.agent ||
    work?.latest?.agent_key ||
    choice ||
    engine.workspace?.shaping_agent_key ||
    agents[0]?.key ||
    '';
  const directing = !work && agentKey === engine.workspace?.shaping_agent_key;
  const selectedAgent = engine.workspace?.agents.find((agent) => agent.key === agentKey);
  const setup =
    selectedAgent &&
    agentSetup(
      selectedAgent,
      engine.catalog,
      engine.isConnected && !engine.readErrors['Agent setup'],
    );
  const setupIssue =
    !selectedAgent && agentKey
      ? 'The selected agent is not in this workspace. Refresh or choose another agent.'
      : setup?.state === 'needs_setup'
        ? setup.message
        : '';
  const current = useRef(key);
  current.current = key;
  const withinLimit =
    new TextEncoder().encode(draft.text.trim()).length <= (engine.workspace?.input_limit || 12000);
  const enabled =
    engine.isConnected &&
    !!agentKey &&
    (!setupIssue || !!draft.pending) &&
    !writer.busyKey &&
    !!draft.text.trim() &&
    withinLimit &&
    (!work || canReply(work) || !!draft.pending);
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        if (!enabled) return;
        const sentKey = key;
        void writer.send(
          key,
          agentKey,
          work?.latest?.id,
          engine.submitTask,
          (task) => {
            if (current.current !== sentKey) return;
            if (task.purpose === 'explore' && onShape) onShape(task.id);
            else onAccepted(work?.id || task.id);
          },
          directing ? 'explore' : undefined,
        );
      }}
    >
      <div className="tw-recipient">
        <label htmlFor={`request-${key}`}>
          {work ? 'Follow up on this work' : 'What would you like to work on?'}
        </label>
        {!work && (
          <label>
            With{' '}
            <select
              aria-label="Assign to agent"
              value={agentKey}
              disabled={!!draft.pending || !!writer.busyKey}
              onChange={(event) => setChoice(event.target.value)}
            >
              {engine.workspace?.shaping_agent_key ? (
                <option value={engine.workspace.shaping_agent_key}>
                  The Guide · coordinate with me
                </option>
              ) : (
                <option value="">Connect your engine</option>
              )}
              {agentKey && !selectedAgent && (
                <option value={agentKey}>Selected agent unavailable</option>
              )}
              {agents.map((agent) => (
                <option key={agent.key} value={agent.key}>
                  {agent.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <div>
        <textarea
          id={`request-${key}`}
          value={draft.text}
          rows={2}
          placeholder={
            work
              ? 'Share guidance or ask a follow-up…'
              : directing
                ? 'Ask a question, explore an idea, or put your team to work…'
                : 'Give this agent something worth working on…'
          }
          readOnly={!!writer.busyKey || (!!draft.pending && !draft.editable)}
          onChange={(event) => writer.edit(key, event.target.value)}
        />
        <button
          disabled={!enabled}
          aria-label={
            draft.pending
              ? 'Retry work request'
              : work
                ? 'Send follow-up'
                : directing
                  ? 'Send to the Guide'
                  : 'Start work'
          }
        >
          <ArrowUp size={16} />
          {!work && <span>{directing ? 'Send' : 'Start work'}</span>}
        </button>
      </div>
      {directing && selectedAgent && onGuideSettings && (
        <button
          type="button"
          className="px-text-button"
          onClick={onGuideSettings}
          disabled={!engine.isConnected}
        >
          Guide model · {selectedAgent.model}
        </button>
      )}
      {onShape && !directing && (
        <div className="tw-composer-alternatives">
          <span>Still figuring it out?</span>
          <button type="button" onClick={() => onShape()}>
            Shape work together →
          </button>
        </div>
      )}
      <small>
        {work && !canReply(work)
          ? 'Draft here; send after this request finishes.'
          : engine.isConnected
            ? ''
            : 'Disconnected · your text stays here.'}
      </small>
      {!withinLimit && <p role="alert">Shorten this request before sending.</p>}
      {setupIssue && <p role="status">{setupIssue} Open Agents to check their setup.</p>}
      {draft.error && (
        <p role="alert">
          {draft.error}{' '}
          {draft.pending &&
            !draft.editable &&
            'Retry checks the same request without starting it twice.'}
        </p>
      )}
      {draft.pending && !draft.error && !writer.busyKey && (
        <p role="status">An earlier send was not confirmed. Retry to check it.</p>
      )}
      {writer.storageWarning && (
        <p role="alert">This tab cannot retain unsent text. Keep a copy until sent.</p>
      )}
    </form>
  );
}
