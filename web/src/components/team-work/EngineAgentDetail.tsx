import { useLocalEngine } from '../../context/LocalEngineContext';
import { engineAgentToUI } from '../../lib/engineAdapters';
import { agentSetup, toolDescription } from '../../lib/agentCapabilities';
import type { EngineAgent } from '../../lib/localEngine';
import { stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { parentEffortTitle, workSignal } from '../../lib/workSignals';
import { WorkStatus } from './WorkStatus';
import { Portrait } from '../ui/Portrait';
import { AgentProviderKey } from '../views/AgentProviderKey';

export function EngineAgentDetail({
  profile,
  created,
  records,
  onBack,
  onWork,
  onAgent,
  onEdit,
  updated,
}: {
  profile: EngineAgent;
  created: boolean;
  records: WorkRecord[];
  onBack: () => void;
  onWork: (id: string) => void;
  onAgent: (key: string) => void;
  onEdit?: () => void;
  updated?: boolean;
}) {
  const engine = useLocalEngine();
  const guide = profile.key === engine.workspace?.shaping_agent_key;
  const fresh = engine.isConnected && !engine.readErrors['Agent setup'];
  const setup = agentSetup(profile, engine.catalog, fresh);
  const listed = engine.workspace?.agents.some((agent) => agent.key === profile.key);
  const provider = engine.catalog?.providers?.find((p) => p.id === profile.provider);
  const connection =
    provider && fresh ? (
      <AgentProviderKey
        key={provider.id}
        provider={provider}
        onSave={async (id, key) => {
          await engine.client.saveProviderKey(id, key);
          await engine.refresh();
        }}
        onRemove={async (id) => {
          await engine.client.removeProviderKey(id);
          await engine.refresh();
        }}
      />
    ) : null;
  const owned = records.filter((work) => work.latest?.agent_key === profile.key);
  const active = owned.filter((work) =>
    ['starting', 'running', 'canceling', 'waiting_human'].includes(work.latest!.state),
  );
  const recent = owned
    .filter((work) => !active.includes(work))
    .slice()
    .reverse();
  const waiting = active.some((work) => work.latest?.state === 'waiting_human');
  const planning =
    engine.workspace?.planning_tasks?.filter(
      (task) =>
        task.agent_key === profile.key && ['starting', 'running', 'canceling'].includes(task.state),
    ) || [];
  const activeCount = active.length + planning.length;
  const tools = (profile.tools || []).filter(
    (id) => !['finish', 'dispatch_assignment', 'ask_human'].includes(id),
  );
  const groups = [
    {
      label: 'Tools',
      ids: tools.filter((id) => !id.startsWith('mcp_') && !id.startsWith('skill_')),
    },
    { label: 'Connections', ids: tools.filter((id) => id.startsWith('mcp_')) },
    { label: 'Skills', ids: tools.filter((id) => id.startsWith('skill_')) },
  ];
  const workRow = (work: WorkRecord) => (
    <button className="agent-work-row" key={work.id} onClick={() => onWork(work.id)}>
      <span>
        <strong>{work.title}</strong>
        {parentEffortTitle(work, records) && <small>{parentEffortTitle(work, records)}</small>}
      </span>
      <WorkStatus
        signal={workSignal(work, engine.approvals?.pending_approvals)}
        label={stateLabel(work)}
      />
    </button>
  );
  return (
    <div className="tw-agent-detail agent-profile">
      <button className="operator-back" onClick={onBack}>
        Back to agents
      </button>
      <header className="agent-profile-hero">
        <Portrait agent={engineAgentToUI(profile)} size={70} square={false} />
        <div>
          <h3>{profile.name}</h3>
          <WorkStatus
            signal={
              !fresh
                ? 'unknown'
                : setup.state === 'needs_setup'
                  ? 'blocked'
                  : waiting
                    ? 'needs_you'
                    : activeCount
                      ? 'working'
                      : setup.state === 'configured'
                        ? 'done'
                        : 'unknown'
            }
            label={
              !fresh
                ? 'Last recorded state'
                : setup.state === 'needs_setup'
                  ? 'Needs setup'
                  : waiting
                    ? 'Needs you'
                    : activeCount
                      ? `${activeCount} active requests`
                      : setup.state === 'configured'
                        ? 'Ready for work'
                        : 'Setup unchecked'
            }
          />
        </div>
      </header>
      <p className="agent-profile-purpose">
        {guide
          ? 'Helps you think through ideas, shape team plans, and understand ongoing work.'
          : profile.purpose || 'No role has been added yet.'}
      </p>
      {updated && (
        <p className="operator-receipt" role="status">
          Changes saved. New work will use these settings.
        </p>
      )}
      {created && (
        <p className="operator-receipt" role="status">
          {listed
            ? 'Agent saved. Give them their first assignment.'
            : 'Agent saved. Refreshing your workspace…'}
        </p>
      )}
      {setup.state !== 'configured' && (
        <p className="operator-notice" role="status">
          {setup.message}
        </p>
      )}
      <div className="operator-actions">
        {!profile.plan_coordinator && (
          <button
            className="tw-primary"
            disabled={!engine.isConnected || !listed || setup.state === 'needs_setup'}
            onClick={() => onAgent(profile.key)}
          >
            Give {profile.name} work
          </button>
        )}
        {onEdit && profile.editable !== false && (
          <button
            className="operator-secondary"
            disabled={!fresh || !profile.definition_digest}
            onClick={onEdit}
          >
            {guide ? 'Guide model & limits' : 'Edit agent'}
          </button>
        )}
        {created && !listed && (
          <button onClick={() => void engine.refresh()}>Refresh workspace</button>
        )}
      </div>
      {profile.editable === false && (
        <p className="operator-footnote">
          Engine-managed planning agent. Create a teammate to choose your own settings and tools.
        </p>
      )}
      {provider && !provider.key_saved && connection}
      <section className="agent-profile-section">
        <h4>
          Current work <span>{activeCount}</span>
        </h4>
        {active.map(workRow)}
        {planning.map((task) => (
          <button
            className="agent-work-row"
            key={task.id}
            onClick={() => onWork(task.planning_for!)}
          >
            <strong>Shaping: {task.input}</strong>
            <WorkStatus signal="working" label="Planning" />
          </button>
        ))}
        {!activeCount && <p className="operator-footnote">No active requests recorded.</p>}
      </section>
      <section className="agent-profile-section">
        <h4>What they can use</h4>
        {guide ? (
          <p>Can inspect work and save proposals. You choose when a plan starts.</p>
        ) : tools.length ? (
          <dl className="agent-access-list">
            {groups
              .filter((g) => g.ids.length)
              .map((g) => (
                <div key={g.label}>
                  <dt>{g.label}</dt>
                  <dd>{toolDescription(g.ids, engine.catalog)}</dd>
                </div>
              ))}
          </dl>
        ) : (
          <p>Works with the information you supply. No tools or skills selected.</p>
        )}
      </section>
      <section className="agent-profile-section">
        <h4>Setup</h4>
        <dl className="agent-access-list">
          <div>
            <dt>Model</dt>
            <dd>
              {profile.model}
              <small>
                {provider?.name ||
                  (profile.provider === 'ollama' || !profile.provider
                    ? 'On this machine · Ollama'
                    : profile.provider)}
              </small>
            </dd>
          </div>
          <div>
            <dt>Runtime</dt>
            <dd>Tetonic · {profile.harness}</dd>
          </div>
          {!profile.plan_coordinator && (
            <div>
              <dt>Per run</dt>
              <dd>
                {profile.max_steps} steps · {profile.max_seconds}s ·{' '}
                {profile.max_tokens.toLocaleString()} tokens
              </dd>
            </div>
          )}
        </dl>
        <details>
          <summary>Access and limits</summary>
          <p>{setup.message}</p>
          {provider?.key_saved && connection}
          {!!groups[0].ids.length && (
            <p>
              Working folder:{' '}
              {profile.hosted_workspace ||
                engine.catalog?.workspace_root ||
                'Configured by the engine host'}
              {profile.hosted_workspace &&
                '. Selected file results may be sent to the model provider.'}
            </p>
          )}
          {profile.plan_coordinator ? (
            <p>
              The agreed plan sets the shared allowance and whole-plan time limit. Review them in
              Shape work.
            </p>
          ) : (
            <p>
              Limits apply to each run. Tool use remains subject to engine policy and the
              permissions you granted.
            </p>
          )}
        </details>
      </section>
      {!!recent.length && (
        <section className="agent-profile-section">
          <h4>Recent work</h4>
          {recent.slice(0, 4).map(workRow)}
          {recent.length > 4 && (
            <details>
              <summary>{recent.length - 4} earlier requests</summary>
              {recent.slice(4).map(workRow)}
            </details>
          )}
        </section>
      )}
    </div>
  );
}
