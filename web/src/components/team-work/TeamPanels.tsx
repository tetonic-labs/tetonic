import { useRef, useState } from 'react';
import { Plus } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { EngineAgent, LocalApproval } from '../../lib/localEngine';
import { needsHelp, stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { workJourneys, journeyStatus, journeyPriority } from '../../lib/workJourneys';
import { HumanQuestion } from './HumanQuestion';
import { Portrait } from '../ui/Portrait';
import { LocalAgentSetup } from '../work/LocalAgentSetup';
import { EngineAgentDetail } from './EngineAgentDetail';
import { agentSetup } from '../../lib/agentCapabilities';

export type TeamPanel = 'work' | 'agents' | 'teams' | 'attention' | 'settings';

function Decision({ approval, onWork }: { approval: LocalApproval; onWork: (id: string) => void }) {
  const { resolveApproval, isConnected, readErrors } = useLocalEngine();
  const locked = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [receipt, setReceipt] = useState<string | null>(null);
  const proposal = approval.proposal;
  async function decide(allow: boolean) {
    if (locked.current) return;
    locked.current = true;
    setBusy(true);
    setError('');
    try {
      const result = await resolveApproval(approval.approval_id, allow, approval.proposal_digest);
      if (
        result.approval_id !== approval.approval_id ||
        result.proposal_digest !== approval.proposal_digest ||
        result.status !== (allow ? 'approved' : 'rejected')
      )
        throw new Error('Your decision was not confirmed. Refresh to check this request.');
      setReceipt(allow ? 'Command approved once' : 'Request declined');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Your decision could not be confirmed.');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  return (
    <article className="tw-decision">
      <h3>{receipt || (proposal ? 'Run this command?' : 'An action needs your permission')}</h3>
      {receipt ? (
        <p>The engine confirmed your decision.</p>
      ) : (
        <>
          {proposal ? (
            <>
              <p>This permission is for this command, on this attempt only.</p>
              <pre style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
                <code>{proposal.command}</code>
              </pre>
              <p>
                In <strong>{proposal.working_directory}</strong> · {proposal.shell}
              </p>
              {!!proposal.confinement_warnings.length && (
                <div role="note">
                  <strong>Limits of this computer’s isolation</strong>
                  <p>
                    The working folder is not a security boundary for this command. Review these
                    limits before allowing it.
                  </p>
                  <ul>
                    {proposal.confinement_warnings.map((warning) => (
                      <li key={warning}>{warning}</li>
                    ))}
                  </ul>
                </div>
              )}
            </>
          ) : (
            <p>
              The proposed action hasn’t been included with this request. You can’t approve it until
              its effects are available to review.
            </p>
          )}
          {approval.work_id && (
            <button onClick={() => onWork(approval.work_id!)}>Open related work</button>
          )}
          <details>
            <summary>Request details</summary>
            <p>Reference: {approval.approval_id}</p>
            <p>Proposal: {approval.proposal_digest}</p>
            <p>Expires: {new Date(approval.expires_at * 1000).toLocaleString()}</p>
          </details>
          <div className="tw-decision-actions">
            <button
              disabled={
                !proposal ||
                !isConnected ||
                !!readErrors.Decisions ||
                busy ||
                approval.expires_at * 1000 <= Date.now()
              }
              title={
                proposal
                  ? 'Allow this exact command once'
                  : 'The exact action must be available to review'
              }
              onClick={() => void decide(true)}
            >
              {proposal ? 'Allow once' : 'Approve unavailable'}
            </button>
            <button
              disabled={
                !isConnected ||
                !!readErrors.Decisions ||
                busy ||
                approval.expires_at * 1000 <= Date.now()
              }
              onClick={() => void decide(false)}
            >
              {busy ? 'Confirming…' : 'Decline request'}
            </button>
          </div>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </article>
  );
}

export function TeamPanels({
  panel,
  records,
  onWork,
  onAgent,
  dark,
  setDark,
  initialAgentId,
  editInitially = false,
}: {
  panel: TeamPanel;
  records: WorkRecord[];
  onWork: (id: string) => void;
  onAgent: (key: string) => void;
  dark: boolean;
  setDark: (value: boolean) => void;
  initialAgentId?: string;
  editInitially?: boolean;
}) {
  const engine = useLocalEngine();
  const { workspace, client, uiAgents, teams, approvals, readErrors } = engine;
  const [creating, setCreating] = useState(false);
  const [agentId, setAgentId] = useState<string | null>(initialAgentId || null);
  const [createdAgent, setCreatedAgent] = useState<EngineAgent | null>(null);
  const [editingAgent, setEditingAgent] = useState<EngineAgent | null>(() =>
    editInitially
      ? workspace?.agents.find((a) => a.key === initialAgentId && a.editable !== false) || null
      : null,
  );
  const [updatedAgentKey, setUpdatedAgentKey] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  if (panel === 'work')
    return (
      <>
        <label className="tw-search">
          Find work
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="A title or something you asked…"
          />
        </label>
        <div className="tw-record-list">
          {workJourneys(records)
            .reverse()
            .sort((a, b) => journeyPriority(a, records) - journeyPriority(b, records))
            .filter((work) =>
              `${work.title} ${work.turns.map((turn) => turn.input).join(' ')}`
                .toLowerCase()
                .includes(query.toLowerCase()),
            )
            .map((work) => (
              <button key={work.id} onClick={() => onWork(work.id)}>
                <strong>{work.title}</strong>
                <span>
                  {journeyStatus(work, records)}
                  {work.latest ? ` · ${work.latest.agent_name}` : ''}
                </span>
              </button>
            ))}
        </div>
        {!records.length && <p>Your work will be here once you send your first request.</p>}
      </>
    );
  if (panel === 'attention')
    return (
      <>
        {readErrors.Decisions && (
          <p className="tw-notice">
            We couldn’t refresh decisions. Any requests shown below may have changed.
          </p>
        )}
        {!readErrors.Decisions &&
          !approvals?.pending_approvals.length &&
          !records.some(needsHelp) && <p>Nothing is waiting for your input.</p>}
        {(approvals?.pending_approvals || []).map((approval) => (
          <Decision key={approval.approval_id} approval={approval} onWork={onWork} />
        ))}
        <div className="tw-record-list">
          {records.filter(needsHelp).map((work) =>
            work.latest?.state === 'waiting_human' &&
            work.latest.human_questions?.some((q) => !q.answer) ? (
              <div key={work.id}>
                <p>{work.title}</p>
                {work.latest.human_questions
                  .filter((q) => !q.answer)
                  .map((question) => (
                    <HumanQuestion
                      key={question.id}
                      task={work.latest!}
                      question={question}
                      refresh={engine.refresh}
                    />
                  ))}
                <button onClick={() => onWork(work.id)}>Open related work →</button>
              </div>
            ) : (
              <button key={work.id} onClick={() => onWork(work.id)}>
                <strong>{work.title}</strong>
                <span>{stateLabel(work)} · Open to see what happened</span>
              </button>
            ),
          )}
        </div>
      </>
    );
  if (panel === 'agents') {
    if (editingAgent && workspace)
      return (
        <LocalAgentSetup
          key={editingAgent.definition_digest}
          client={client}
          workspace={workspace}
          agent={editingAgent}
          onBack={() => setEditingAgent(null)}
          onCreated={async (saved) => {
            await engine.refresh();
            setUpdatedAgentKey(saved.key);
            setEditingAgent(null);
          }}
        />
      );
    if (creating && workspace)
      return (
        <LocalAgentSetup
          client={client}
          workspace={workspace}
          onBack={() => setCreating(false)}
          onCreated={(created) => {
            setCreatedAgent(created);
            setAgentId(created.key);
            setCreating(false);
            void engine.refresh();
          }}
        />
      );
    const profile =
      workspace?.agents.find((entry) => entry.key === agentId) ||
      (createdAgent?.key === agentId ? createdAgent : null);
    if (profile)
      return (
        <EngineAgentDetail
          profile={profile}
          created={createdAgent?.key === profile.key}
          updated={updatedAgentKey === profile.key}
          onEdit={() => {
            setCreatedAgent(null);
            setUpdatedAgentKey(null);
            setEditingAgent(profile);
          }}
          records={records}
          onBack={() => {
            setAgentId(null);
            setCreatedAgent(null);
          }}
          onWork={onWork}
          onAgent={onAgent}
        />
      );
    return (
      <>
        <div className="tw-agent-toolbar">
          <p>Choose someone to work with, or create a new teammate.</p>
          <button
            className="tw-primary"
            disabled={!workspace || !engine.isConnected}
            onClick={() => setCreating(true)}
          >
            <Plus size={16} /> Create agent
          </button>
        </div>
        <div className="tw-agent-list">
          {uiAgents.map((entry) => (
            <button key={entry.id} onClick={() => setAgentId(entry.id)}>
              <Portrait agent={entry} size={52} square={false} />
              <strong>{entry.name}</strong>
              <span>
                {entry.status === 'executing' && engine.isConnected
                  ? 'Working'
                  : !engine.isConnected
                    ? 'Connection lost'
                    : (() => {
                        const profile = workspace?.agents.find((agent) => agent.key === entry.id);
                        const setup =
                          profile &&
                          agentSetup(profile, engine.catalog, !readErrors['Agent setup']);
                        return setup?.state === 'needs_setup'
                          ? 'Needs setup'
                          : setup?.state === 'configured'
                            ? 'Idle'
                            : 'Setup unchecked';
                      })()}
              </span>
            </button>
          ))}
        </div>
      </>
    );
  }
  if (panel === 'teams')
    return (
      <>
        <p>Your connected workspace is {workspace?.team_name || 'not available yet'}.</p>
        {readErrors.Teams && <p className="tw-notice">Team information couldn’t be refreshed.</p>}
        {teams.map((team) => (
          <article className="tw-team" key={team.id}>
            <h3>{team.name}</h3>
            {team.id === workspace?.team_id && (
              <p>{uiAgents.map((agent) => agent.name).join(', ') || 'No assistants yet'}</p>
            )}
          </article>
        ))}
        <p>
          Team creation and membership changes aren’t available in this local connection yet. You
          can start work directly with an assistant.
        </p>
      </>
    );
  return (
    <div className="tw-settings">
      <label>
        <input type="checkbox" checked={dark} onChange={(event) => setDark(event.target.checked)} />{' '}
        Dark appearance
      </label>
      <h3>Connection</h3>
      <p>
        {engine.isConnected
          ? `Connected to ${workspace?.organization}.`
          : 'Open the connection link printed by your local Tetonic engine.'}
      </p>
      <button onClick={engine.reconnect}>Reconnect</button>
      <details>
        <summary>Connection details</summary>
        <p>{engine.error || 'The workspace is receiving engine state.'}</p>
        {Object.entries(readErrors).map(([name, error]) => (
          <p key={name}>
            {name}: {error}
          </p>
        ))}
      </details>
      <h3>Who can see this work?</h3>
      <p>
        This is the local owner workspace. Requests and replies are saved in its engine storage. A
        configured hosted model may receive the conversation; inspect the assistant’s settings for
        its provider. This is not a separate private chat boundary.
      </p>
      <h3>Stopping work</h3>
      <p>
        Open a running request and choose Stop this request. That asks the engine to stop that
        execution; it does not undo completed actions. Workspace-wide emergency stop is not exposed
        by this connection yet.
      </p>
      {!!approvals?.active_stops.length && (
        <div className="tw-notice">
          <h3>Active restrictions</h3>
          {approvals.active_stops.map((stop) => (
            <p key={`${stop.scope_kind}:${stop.scope_id}`}>
              {stop.mode}: {stop.reason} ({stop.scope_kind})
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
