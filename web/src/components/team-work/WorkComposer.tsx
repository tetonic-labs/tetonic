import { useRef, useState } from 'react';
import { ArrowUp } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../lib/localEngine';
import { canReply, type WorkRecord } from '../../lib/workspaceRecords';
import { useWorkspaceDraft } from '../workspace/useWorkspaceDraft';

export function WorkComposer({
  work,
  recipient,
  onAccepted,
  onShape,
}: {
  work?: WorkRecord;
  recipient?: string;
  onAccepted: (id: string) => void;
  onShape?: () => void;
}) {
  const engine = useLocalEngine();
  const [choice, setChoice] = useState(recipient || '');
  const agents = (engine.workspace?.agents || []).filter(
    (agent) => agent.key !== engine.workspace?.shaping_agent_key && !agent.plan_coordinator,
  );
  const agentKey =
    work?.latest?.agent_key ||
    (agents.some((agent) => agent.key === choice) ? choice : agents[0]?.key || '');
  const key = work ? `work:${work.id}` : 'new';
  const writer = useWorkspaceDraft(`team-work:${connectionDraftScope()}`);
  const draft = writer.drafts[key] || { text: '' };
  const current = useRef(key);
  current.current = key;
  const withinLimit =
    new TextEncoder().encode(draft.text.trim()).length <= (engine.workspace?.input_limit || 12000);
  const enabled =
    engine.isConnected &&
    !!agentKey &&
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
        void writer.send(key, agentKey, work?.latest?.id, engine.submitTask, (task) => {
          if (current.current === sentKey) onAccepted(work?.id || task.id);
        });
      }}
    >
      <div className="tw-recipient">
        <label htmlFor={`request-${key}`}>
          {work ? 'Follow up on this work' : 'What would you like done?'}
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
              {!agents.length && <option value="">No agent available</option>}
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
              : 'Give an agent something worth working on…'
          }
          readOnly={!!writer.busyKey || (!!draft.pending && !draft.editable)}
          onChange={(event) => writer.edit(key, event.target.value)}
        />
        <button
          disabled={!enabled}
          aria-label={draft.pending ? 'Retry work request' : work ? 'Send follow-up' : 'Start work'}
        >
          <ArrowUp size={16} />
          {!work && <span>Start work</span>}
        </button>
      </div>
      {onShape && (
        <div className="tw-composer-alternatives">
          <span>Still figuring it out?</span>
          <button type="button" onClick={onShape}>
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
