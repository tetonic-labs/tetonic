import { useCallback, useEffect, useRef, useState } from 'react';
import { AgentCreateForm } from '../views/AgentCreateForm';
import type { AgentDraft } from '../../lib/agentConfiguration';
import type {
  AgentCatalog,
  CreateEngineAgent,
  EngineAgent,
  EngineWorkspace,
  LocalEngine,
} from '../../lib/localEngine';

export function LocalAgentSetup({
  client,
  workspace,
  onCreated,
  onBack,
}: {
  client: LocalEngine;
  workspace: EngineWorkspace;
  onCreated: (agent: EngineAgent) => void;
  onBack: () => void;
}) {
  const [catalog, setCatalog] = useState<AgentCatalog | null>(null);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [retry, setRetry] = useState(0);
  const [connectionRevision, setConnectionRevision] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const discoverModels = useCallback(
    (provider: string, signal?: AbortSignal) => client.providerModels(provider, signal),
    [client],
  );
  const pending = useRef<CreateEngineAgent | null>(null);
  const active = useRef(true);
  useEffect(() => {
    active.current = true;
    const abort = new AbortController();
    setError('');
    setRefreshing(true);
    client
      .agentCatalog(abort.signal)
      .then((value) => {
        if (active.current && !abort.signal.aborted) setCatalog(value);
      })
      .catch((error: unknown) => {
        if (active.current && !abort.signal.aborted)
          setError(error instanceof Error ? error.message : 'Model discovery failed.');
      })
      .finally(() => {
        if (active.current && !abort.signal.aborted) setRefreshing(false);
      });
    return () => {
      active.current = false;
      abort.abort();
    };
  }, [client, retry]);
  async function create(draft: AgentDraft) {
    if (saving) return;
    const value = {
      provider: draft.provider || 'ollama',
      hosted_consent: !!draft.hostedConsent,
      hosted_tools_consent: !!draft.hostedToolsConsent,
      expected_workspace_root: draft.expectedWorkspaceRoot,
      tools: draft.tools || [],
      name: draft.name,
      purpose: draft.purpose,
      model: draft.model,
      harness: draft.configuration.harness,
      max_steps: draft.configuration.limits.maxSteps,
      max_seconds: draft.configuration.limits.maxSeconds,
      max_tokens: draft.configuration.limits.maxTokens,
    };
    const candidate = { ...value, request_id: pending.current?.request_id || crypto.randomUUID() };
    if (pending.current && JSON.stringify(candidate) !== JSON.stringify(pending.current))
      candidate.request_id = crypto.randomUUID();
    pending.current = candidate;
    setSaving(true);
    setError('');
    try {
      const agent = await client.createAgent(candidate);
      if (active.current) onCreated(agent);
    } catch (error) {
      if (active.current)
        setError(error instanceof Error ? error.message : 'Could not save agent.');
    } finally {
      if (active.current) setSaving(false);
    }
  }
  if (!catalog)
    return (
      <div className="local-agent-loading">
        <button className="local-back" onClick={onBack}>
          Back to work
        </button>
        <p role={error ? 'alert' : 'status'}>
          {error ||
            (catalog
              ? 'No compatible local models found. Install a tool-capable model in Ollama, then refresh.'
              : 'Checking installed models…')}
        </p>
        {(error || catalog) && (
          <button className="canvas-primary" onClick={() => setRetry((value) => value + 1)}>
            Refresh models
          </button>
        )}
      </div>
    );
  return (
    <AgentCreateForm
      teams={[
        {
          id: workspace.team_id,
          name: workspace.team_name,
          tagline: '',
          isPersonal: true,
          members: [],
          pledgedAgentIds: [],
          createdAt: '',
        },
      ]}
      currentTeamId={workspace.team_id}
      resources={[]}
      models={catalog.models}
      defaultModel={
        catalog.models.includes(workspace.model) ? workspace.model : catalog.models[0] || ''
      }
      onCreate={(draft) => void create(draft)}
      onBack={saving ? undefined : onBack}
      connected={{
        catalog,
        saving,
        error,
        onDiscoverModels: discoverModels,
        connectionRevision,
        refreshing,
        onRefresh: () => setRetry((value) => value + 1),
        onSaveKey: async (provider, key) => {
          const saved = await client.saveProviderKey(provider, key);
          if (active.current) setConnectionRevision((value) => value + 1);
          if (active.current)
            setCatalog((old) =>
              old
                ? {
                    ...old,
                    providers: old.providers?.map((item) => (item.id === saved.id ? saved : item)),
                  }
                : old,
            );
        },
        onRemoveKey: async (provider) => {
          const removed = await client.removeProviderKey(provider);
          if (active.current)
            setCatalog((old) =>
              old
                ? {
                    ...old,
                    providers: old.providers?.map((item) =>
                      item.id === removed.id ? removed : item,
                    ),
                  }
                : old,
            );
        },
      }}
    />
  );
}
