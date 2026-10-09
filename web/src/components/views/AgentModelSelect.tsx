import { useEffect, useId, useState } from 'react';
import { type ProviderModelCatalog } from '../../engine/contracts';

const providerCatalogs: Record<string, { name: string; url: string }> = {
  openai: { name: 'OpenAI', url: 'https://developers.openai.com/api/docs/models' },
  anthropic: {
    name: 'Anthropic',
    url: 'https://platform.claude.com/docs/en/about-claude/models/overview',
  },
  google: { name: 'Google', url: 'https://ai.google.dev/gemini-api/docs/models' },
};

export function AgentModelSelect({
  provider,
  keySaved,
  connectionRevision = 0,
  discover,
  choices,
  defaultModel,
  model,
  customModel,
  onModel,
  onCustomModel,
  connected,
}: {
  provider: string;
  keySaved: boolean;
  connectionRevision?: number;
  discover?: (provider: string, signal?: AbortSignal) => Promise<ProviderModelCatalog>;
  choices: string[];
  defaultModel: string;
  model: string;
  customModel: string;
  onModel: (value: string) => void;
  onCustomModel: (value: string) => void;
  connected: boolean;
}) {
  const hosted = provider !== 'ollama';
  const lab = providerCatalogs[provider];
  const statusId = useId();
  const [result, setResult] = useState<{
    catalog: ProviderModelCatalog;
    revision: number;
    checkedAt: string;
  } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [refresh, setRefresh] = useState(0);
  const [query, setQuery] = useState('');
  const current =
    keySaved && result?.catalog.provider === provider && result.revision === connectionRevision
      ? result
      : null;
  const remote = current?.catalog.models || [];

  useEffect(() => {
    setQuery('');
  }, [provider, keySaved, connectionRevision]);
  useEffect(() => {
    setError('');
    if (!hosted || !keySaved || !discover) {
      setResult(null);
      setLoading(false);
      return;
    }
    const abort = new AbortController();
    setLoading(true);
    discover(provider, abort.signal)
      .then((catalog) => {
        if (abort.signal.aborted) return;
        if (catalog.provider !== provider)
          throw new Error('The model catalog did not match this provider. Refresh to try again.');
        setResult({
          catalog,
          revision: connectionRevision,
          checkedAt: catalog.fetched_at || new Date().toISOString(),
        });
      })
      .catch((problem: unknown) => {
        if (!abort.signal.aborted)
          setError(
            problem instanceof Error
              ? problem.message
              : 'Could not load models. Retry or enter a model ID.',
          );
      })
      .finally(() => {
        if (!abort.signal.aborted) setLoading(false);
      });
    return () => abort.abort();
  }, [hosted, provider, keySaved, connectionRevision, discover, refresh]);

  const models = hosted ? remote : choices;
  const entries = new Map(current?.catalog.entries?.map((entry) => [entry.id, entry]));
  const search = query.trim().toLowerCase();
  const matching = hosted
    ? models.filter((id) =>
        `${id} ${entries.get(id)?.display_name || ''}`.toLowerCase().includes(search),
      )
    : models;
  const selected = model && model !== 'custom';
  const missing = selected && !models.includes(model);
  const hiddenSelection = selected && !matching.includes(model);
  const checkedDate = current ? new Date(current.checkedAt) : null;
  const checkedTime =
    checkedDate && !Number.isNaN(checkedDate.getTime())
      ? checkedDate.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
      : '';

  return (
    <>
      {hosted && models.length > 0 && (
        <label className="agent-model-search">
          Search models
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search by name or model ID"
            autoComplete="off"
          />
        </label>
      )}
      <label>
        Model
        <select
          value={model}
          onChange={(event) => onModel(event.target.value)}
          aria-describedby={hosted ? statusId : undefined}
          size={
            hosted && models.length > 0 ? Math.min(6, Math.max(3, matching.length + 2)) : undefined
          }
          className={hosted && models.length > 0 ? 'agent-model-list' : undefined}
        >
          <option value="">
            {hosted
              ? 'Choose a model'
              : defaultModel
                ? `Workspace default · ${defaultModel}`
                : 'No local models available'}
          </option>
          {hiddenSelection && <option value={model}>{model} · selected</option>}
          {matching.map((id) => {
            const name = entries.get(id)?.display_name;
            return (
              <option key={id} value={id} title={id}>
                {name && name !== id ? `${name} · ${id}` : id}
              </option>
            );
          })}
          {(!connected || hosted) && <option value="custom">Specify a model…</option>}
        </select>
      </label>
      {hosted && (
        <div className="agent-field-note agent-model-note" id={statusId}>
          {selected && <p className="agent-selected-model">Selected: {model}</p>}
          <p role="status">
            {loading
              ? 'Checking models available to your account…'
              : !keySaved
                ? `Connect ${lab?.name || 'your provider'} below to load your available models.`
                : current
                  ? `${remote.length} ${remote.length === 1 ? 'model' : 'models'} returned by ${lab?.name || provider}${checkedTime ? ` · Last checked ${checkedTime}` : ''}.`
                  : error
                    ? 'Your model list is unavailable.'
                    : 'Model discovery is not connected. You can enter a model ID.'}
          </p>
          {error && (
            <p role="alert">
              {error}
              {current ? ' Showing the last successful list; availability may have changed.' : ''}
            </p>
          )}
          {current && !remote.length && (
            <p>This account returned no models. Check its access or enter a model ID.</p>
          )}
          {search && !matching.length && (
            <p>No models match “{query}”. Clear the search to see all models.</p>
          )}
          {current && missing && (
            <p>
              The selected model was not returned by this account. Check access before starting
              work.
            </p>
          )}
          <div className="agent-model-actions">
            {discover && keySaved && (
              <button
                type="button"
                disabled={loading}
                onClick={() => setRefresh((value) => value + 1)}
              >
                Refresh models
              </button>
            )}
            {lab && (
              <a href={lab.url} target="_blank" rel="noreferrer">
                Browse {lab.name}’s catalog ↗
              </a>
            )}
          </div>
          {current && remote.length > 0 && (
            <p>
              Choose a text model with tool calling. Being listed doesn’t verify those capabilities.
            </p>
          )}
          {!keySaved && (
            <p>
              The public catalog shows the lab’s lineup. Your API key determines account access.
            </p>
          )}
        </div>
      )}
      {model === 'custom' && (
        <label>
          Model identifier
          <input
            required
            maxLength={256}
            value={customModel}
            onChange={(event) => onCustomModel(event.target.value)}
            placeholder="Provider’s exact model ID"
          />
        </label>
      )}
    </>
  );
}
