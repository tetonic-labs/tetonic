import { useEffect, useState } from 'react';
import { type LocalEngine } from '../../engine/client';
import { type WorkspaceSkill } from '../../engine/contracts';

export function SkillDetails({
  skill,
  client,
  onChanged,
}: {
  skill: WorkspaceSkill;
  client: LocalEngine;
  onChanged: () => Promise<void>;
}) {
  const [content, setContent] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const abort = new AbortController();
    setContent('');
    setError('');
    client
      .skillContent(skill.id, abort.signal)
      .then((v) => {
        if (!abort.signal.aborted) setContent(v.content);
      })
      .catch((e) => {
        if (!abort.signal.aborted)
          setError(e instanceof Error ? e.message : 'Could not read skill.');
      });
    return () => abort.abort();
  }, [client, skill.id]);
  return (
    <section className="skill-details">
      <p>
        {skill.source || 'Imported skill'} · Version {skill.id.slice(6, 14)}
      </p>
      <details>
        <summary>Read skill instructions</summary>
        <pre>{content || 'Loading…'}</pre>
      </details>
      {skill.enabled ? (
        <details>
          <summary>Revoke workspace access</summary>
          <p>
            Agents using this version will need attention. Further execution is stopped when the
            engine checks access; completed actions cannot be undone.
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setError('');
              try {
                await client.revokeSkill(skill.id);
                await onChanged();
              } catch (e) {
                setError(e instanceof Error ? e.message : 'Could not revoke skill.');
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? 'Revoking…' : 'Revoke this skill'}
          </button>
        </details>
      ) : (
        <p role="status">
          Revoked. The agents listed below still have this version in their saved settings. Edit
          them to remove it or select another version.
        </p>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
