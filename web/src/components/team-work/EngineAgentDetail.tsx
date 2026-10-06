import { useLocalEngine } from '../../context/LocalEngineContext';
import { engineAgentToUI } from '../../lib/engineAdapters';
import { agentSetup, toolDescription } from '../../lib/agentCapabilities';
import type { EngineAgent } from '../../lib/localEngine';
import { stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { Portrait } from '../ui/Portrait';
import { AgentProviderKey } from '../views/AgentProviderKey';

export function EngineAgentDetail({
  profile,
  created,
  records,
  onBack,
  onWork,
  onAgent,
}: {
  profile: EngineAgent;
  created: boolean;
  records: WorkRecord[];
  onBack: () => void;
  onWork: (id: string) => void;
  onAgent: (key: string) => void;
}) {
  const engine = useLocalEngine();
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
  return (
    <div className="tw-agent-detail">
      <button onClick={onBack}>Back to agents</button>
      <Portrait agent={engineAgentToUI(profile)} size={76} square={false} />
      <h3>{profile.name}</h3>
      {created && (
        <p role="status">
          {listed
            ? 'Agent saved. Give them their first assignment.'
            : 'Agent saved. Refreshing your workspace…'}
        </p>
      )}
      <p>{profile.purpose}</p>
      <p>
        {toolDescription(profile.tools || [], engine.catalog) ||
          'Works with the information you supply. No file or external tools selected.'}
      </p>
      {setup.state !== 'configured' && <p role="status">{setup.message}</p>}
      {provider && !provider.key_saved && connection}
      {!profile.plan_coordinator && (
        <button
          className="tw-primary"
          disabled={!engine.isConnected || !listed || setup.state === 'needs_setup'}
          onClick={() => onAgent(profile.key)}
        >
          Give {profile.name} work
        </button>
      )}
      {created && !listed && (
        <button onClick={() => void engine.refresh()}>Refresh workspace</button>
      )}
      <div className="tw-record-list">
        {records
          .filter((work) => work.latest?.agent_key === profile.key)
          .map((work) => (
            <button key={work.id} onClick={() => onWork(work.id)}>
              <strong>{work.title}</strong>
              <span>{stateLabel(work)}</span>
            </button>
          ))}
      </div>
      <details>
        <summary>Access and limits</summary>
        <p>
          Model: {profile.model} · {profile.provider || 'Local provider'}
        </p>
        <p>Tetonic runtime · {setup.message}</p>
        {provider?.key_saved && connection}
        {!!profile.tools?.some(
          (tool) =>
            !tool.startsWith('mcp_') &&
            !['finish', 'dispatch_assignment', 'ask_human'].includes(tool),
        ) && (
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
            Up to {profile.max_steps} steps, {profile.max_seconds} seconds and{' '}
            {profile.max_tokens.toLocaleString()} reported tokens per run.
          </p>
        )}
      </details>
    </div>
  );
}
