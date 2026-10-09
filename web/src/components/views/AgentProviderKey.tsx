import { useState } from 'react';
import { type EngineProvider } from '../../engine/contracts';

export function AgentProviderKey({
  provider,
  onSave,
  onRemove,
}: {
  provider: EngineProvider;
  onSave: (provider: string, key: string) => Promise<void>;
  onRemove?: (provider: string) => Promise<void>;
}) {
  const [key, setKey] = useState('');
  const [editing, setEditing] = useState(!provider.key_saved);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  async function save() {
    if (saving || !key.trim()) return;
    setSaving(true);
    setError('');
    // Clear the password immediately; never put it in the agent draft or storage.
    const supplied = key.trim();
    setKey('');
    try {
      await onSave(provider.id, supplied);
      setEditing(false);
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Could not save the key.');
    } finally {
      setSaving(false);
    }
  }
  async function remove() {
    if (!onRemove || saving) return;
    setSaving(true);
    setError('');
    setKey('');
    try {
      await onRemove(provider.id);
      setEditing(true);
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Could not remove the key.');
    } finally {
      setSaving(false);
    }
  }
  return (
    <div className="agent-provider-key">
      {provider.key_saved && (
        <div className="agent-key-status">
          <span>Key saved on this machine</span>
          <button
            type="button"
            onClick={() => {
              setEditing(!editing);
              setKey('');
            }}
            disabled={saving}
          >
            {editing ? 'Cancel' : 'Replace'}
          </button>
          {onRemove && (
            <button type="button" disabled={saving} onClick={() => void remove()}>
              Remove
            </button>
          )}
        </div>
      )}
      {(editing || !provider.key_saved) && (
        <>
          <label>
            {provider.name} API key
            <input
              type="password"
              autoComplete="off"
              spellCheck={false}
              maxLength={4096}
              value={key}
              onChange={(event) => setKey(event.target.value)}
              disabled={saving}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void save();
                }
              }}
              placeholder="Paste your API key"
            />
          </label>
          <button
            type="button"
            className="agent-key-save"
            disabled={saving || !key.trim()}
            onClick={() => void save()}
          >
            {saving ? 'Saving key…' : 'Save key securely'}
          </button>
        </>
      )}
      {error && <p role="alert">{error}</p>}
      <small>
        Shared on this machine. Saved in your OS credential store; not yet verified with the
        provider.
      </small>
    </div>
  );
}
