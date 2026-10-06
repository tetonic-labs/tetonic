import { useEffect, useState } from 'react';
import type { ProviderModelCatalog } from '../../lib/localEngine';

export function AgentModelSelect({
  provider,
  keySaved,
  connectionRevision,
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
  const [remote, setRemote] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    setRemote([]);
    setError('');
    setLoaded(false);
    if (!hosted || !keySaved || !discover) {
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
        setRemote(catalog.models);
        setLoaded(true);
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
  const missing = model && model !== 'custom' && !models.includes(model);
  return (
    <>
      <label>
        Model
        <select value={model} onChange={(event) => onModel(event.target.value)}>
          <option value="">
            {hosted
              ? 'Choose a model'
              : defaultModel
                ? `Workspace default · ${defaultModel}`
                : 'No local models available'}
          </option>
          {missing && <option value={model}>{model} · selected</option>}
          {models.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
          {(!connected || hosted) && <option value="custom">Specify a model…</option>}
        </select>
      </label>
      {hosted && (
        <div className="agent-field-note">
          <p role="status">
            {loading
              ? 'Checking models available to your account…'
              : error ||
                (!keySaved
                  ? 'Save a provider key to see available models.'
                  : loaded && !remote.length
                    ? 'This account returned no models. Check its access or enter a model ID.'
                    : loaded
                      ? 'Models available to your account. Choose one that supports text and tool calling.'
                      : 'Enter the provider’s model ID, or connect model discovery.')}
          </p>
          {loaded && missing && (
            <p>
              The selected model was not returned by this account. Check access before starting
              work.
            </p>
          )}
          {discover && keySaved && (
            <button
              type="button"
              disabled={loading}
              onClick={() => setRefresh((value) => value + 1)}
            >
              Refresh models
            </button>
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
