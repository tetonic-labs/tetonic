import { useId, useState } from 'react';
import type { ConnectionProblem } from '../../engine/failure';
import './connection-recovery.css';

export function ConnectionRecovery({
  problem,
  onConnect,
}: {
  problem?: ConnectionProblem | null;
  onConnect: (link: string) => void;
}) {
  const id = useId();
  const [link, setLink] = useState('');
  const [error, setError] = useState<string | null>(null);
  return (
    <form
      className="engine-connection-form"
      onSubmit={(event) => {
        event.preventDefault();
        try {
          onConnect(link);
          setLink('');
          setError(null);
        } catch (failure) {
          setError(
            failure instanceof Error ? failure.message : 'Could not use this connection link.',
          );
        }
      }}
    >
      <p id={`${id}-help`}>
        {problem === 'authorization'
          ? 'This connection is no longer accepted. Use the latest link from the engine launcher or a connected browser’s Workspace settings.'
          : 'Each browser needs a connection link. Copy it from the engine launcher or from Workspace settings in a connected browser.'}
      </p>
      <label htmlFor={id}>Engine connection link</label>
      <div className="engine-connection-input">
        <input
          id={id}
          type="password"
          value={link}
          autoComplete="off"
          spellCheck={false}
          aria-describedby={`${id}-help${error ? ` ${id}-error` : ''}`}
          aria-invalid={!!error}
          placeholder="Paste the full connection link"
          onChange={(event) => {
            setLink(event.target.value);
            setError(null);
          }}
        />
        <button type="submit" disabled={!link.trim()}>
          Connect
        </button>
      </div>
      {error && (
        <p id={`${id}-error`} role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
