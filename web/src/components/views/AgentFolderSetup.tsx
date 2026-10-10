import { useEffect, useRef, useState } from 'react';
import { FolderPlus } from 'lucide-react';
import type { LocalEngine } from '../../engine/client';
import type { WorkspaceFolders } from '../../engine/contracts';

/** Inline so adding access never unmounts the agent draft or opens another modal. */
export function AgentFolderSetup({
  client,
  disabled,
  onAdded,
  onActiveChange,
  workspaceOnly = false,
}: {
  client: LocalEngine;
  disabled: boolean;
  onAdded: (path: string) => Promise<void>;
  onActiveChange: (active: boolean) => void;
  workspaceOnly?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState('');
  const [review, setReview] = useState<{ path: string; catalog: WorkspaceFolders } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const inFlight = useRef(false);
  useEffect(() => () => onActiveChange(false), [onActiveChange]);
  const pending = useRef<{ request_id: string; expected_revision: number; paths: string[] } | null>(
    null,
  );

  function close() {
    setOpen(false);
    setReview(null);
    setError('');
    onActiveChange(false);
  }
  async function check() {
    if (inFlight.current || disabled || !path.trim()) return;
    inFlight.current = true;
    setBusy(true);
    setError('');
    try {
      const [folder, catalog] = await Promise.all([
        client.inspectFolder(path.trim()),
        client.workspaceFolders(),
      ]);
      if (!catalog.can_manage) throw new Error('Working folders are managed by the host operator.');
      setReview({ path: folder.path, catalog });
      pending.current = null;
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not check this folder.');
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  async function add() {
    if (!review || inFlight.current || disabled) return;
    inFlight.current = true;
    setBusy(true);
    setError('');
    try {
      if (![...review.catalog.paths, ...review.catalog.configured_paths].includes(review.path)) {
        pending.current ??= {
          request_id: crypto.randomUUID(),
          expected_revision: review.catalog.revision,
          paths: [...new Set([...review.catalog.paths, review.path])].sort(),
        };
        const saved = await client.saveWorkspaceFolders(pending.current);
        if (!saved.can_manage || !saved.paths.includes(review.path))
          throw new Error('Folder access changed. Check this folder again before selecting it.');
      }
      await onAdded(review.path);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not add this folder.');
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  if (!open)
    return (
      <button
        type="button"
        className="px-text-button"
        disabled={disabled}
        onClick={() => {
          setOpen(true);
          onActiveChange(true);
        }}
      >
        <FolderPlus size={16} aria-hidden="true" /> Add a working folder
      </button>
    );

  return (
    <div className="agent-folder-setup simple-form" aria-label="Add a working folder">
      {review ? (
        <>
          <strong>Use this folder?</strong>
          <code className="agent-folder-path">{review.path}</code>
          <p className="agent-field-note">
            {workspaceOnly
              ? 'This makes the folder available to choose in agent settings. Adding it does not grant access to any agent.'
              : 'This makes the folder available in this workspace. Save the agent to grant its selected file or terminal tools access. Other agents keep their current folders.'}
          </p>
          <div className="agent-folder-actions">
            <button
              type="button"
              className="canvas-primary"
              disabled={disabled || busy}
              onClick={() => void add()}
            >
              {busy ? 'Adding…' : workspaceOnly ? 'Add folder' : 'Add and select folder'}
            </button>
            <button
              type="button"
              className="px-text-button"
              disabled={busy}
              onClick={() => {
                setReview(null);
                setError('');
              }}
            >
              Change folder
            </button>
          </div>
        </>
      ) : (
        <>
          <label>
            Folder on the engine computer
            <input
              autoFocus
              value={path}
              disabled={busy || disabled}
              placeholder="Paste an existing folder’s full path"
              onChange={(e) => {
                setPath(e.target.value);
                setError('');
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  e.stopPropagation();
                  void check();
                }
              }}
            />
          </label>
          <p className="agent-field-note">
            Choose only the folder needed for this work. The engine checks access before you add it.
          </p>
          <button
            type="button"
            className="canvas-primary"
            disabled={!path.trim() || busy || disabled}
            onClick={() => void check()}
          >
            {busy ? 'Checking folder…' : 'Check folder'}
          </button>
        </>
      )}
      {error && (
        <p className="local-notice" role="alert">
          {error}
        </p>
      )}
      <button type="button" className="px-text-button" disabled={busy} onClick={close}>
        Cancel folder setup
      </button>
    </div>
  );
}
