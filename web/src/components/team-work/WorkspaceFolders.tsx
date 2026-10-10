import { useCallback, useEffect, useRef, useState } from 'react';
import { FolderOpen } from 'lucide-react';
import type { LocalEngine } from '../../engine/client';
import type { EngineAgent, WorkspaceFolders as FolderCatalog } from '../../engine/contracts';
import { AgentFolderSetup } from '../views/AgentFolderSetup';

export function WorkspaceFolders({
  client,
  agents,
  onChanged,
}: {
  client: LocalEngine;
  agents: EngineAgent[];
  onChanged: () => Promise<void>;
}) {
  const [catalog, setCatalog] = useState<FolderCatalog | null>(null);
  const [error, setError] = useState('');
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState(false);
  const active = useRef(true);
  const pending = useRef<{ request_id: string; expected_revision: number; paths: string[] } | null>(
    null,
  );
  const load = useCallback(async () => {
    const next = await client.workspaceFolders();
    if (active.current) {
      setCatalog(next);
      setError('');
    }
    return next;
  }, [client]);
  useEffect(() => {
    active.current = true;
    void load().catch((e) => {
      if (active.current)
        setError(e instanceof Error ? e.message : 'Could not load working folders.');
    });
    return () => {
      active.current = false;
    };
  }, [load]);
  async function remove() {
    if (!catalog || !removing || busy) return;
    setBusy(true);
    setError('');
    pending.current ??= {
      request_id: crypto.randomUUID(),
      expected_revision: catalog.revision,
      paths: catalog.paths.filter((path) => path !== removing),
    };
    try {
      const saved = await client.saveWorkspaceFolders(pending.current);
      if (saved.paths.includes(removing))
        throw new Error('Folder access changed. Refresh folders and review it again.');
      if (active.current) {
        setCatalog(saved);
        setRemoving(null);
        pending.current = null;
      }
      await onChanged();
    } catch (e) {
      if (active.current)
        setError(e instanceof Error ? e.message : 'Could not remove this folder.');
    } finally {
      if (active.current) setBusy(false);
    }
  }
  const paths = [...new Set([...(catalog?.configured_paths || []), ...(catalog?.paths || [])])];
  return (
    <section className="workspace-folders" aria-label="Working folders">
      <div className="workspace-setting-row">
        <FolderOpen size={19} aria-hidden="true" />
        <div>
          <h3>Working folders</h3>
          <p>Make folders available here. Choose who can use them in Agents.</p>
        </div>
      </div>
      {!catalog && !error && <p role="status">Loading folders…</p>}
      {catalog && !paths.length && (
        <p className="agent-field-note">
          No folders added yet. You can still chat and plan without one.
        </p>
      )}
      {paths.length > 0 && (
        <ul className="workspace-folder-list">
          {paths.map((path) => {
            const configured = catalog!.configured_paths.includes(path);
            const assigned = agents.filter((agent) => agent.workspace_root === path);
            return (
              <li key={path}>
                <div>
                  <code>{path}</code>
                  <p className="agent-field-note">
                    {configured
                      ? 'Managed by host'
                      : !catalog!.can_manage
                        ? 'Disabled by host'
                        : catalog!.unavailable_paths?.includes(path)
                          ? 'Unavailable · Reconnect this folder or remove it'
                          : 'Available'}
                    {assigned.length > 0
                      ? ` · Used by ${assigned.map((a) => a.name).join(', ')}`
                      : ' · Not assigned to an agent'}
                  </p>
                </div>
                {catalog!.can_manage && !configured && removing !== path && (
                  <button
                    type="button"
                    className="workspace-settings-action"
                    aria-label={`Remove ${path}`}
                    disabled={busy || adding || !!removing}
                    onClick={() => {
                      setRemoving(path);
                      pending.current = null;
                      setError('');
                    }}
                  >
                    Remove
                  </button>
                )}
                {removing === path && (
                  <div
                    className="workspace-folder-confirm"
                    role="group"
                    aria-label="Confirm folder removal"
                  >
                    <p>
                      Remove this folder from future work?{' '}
                      {assigned.length
                        ? 'Agents using it will need another folder before starting new work. '
                        : ''}
                      Files stay on disk. Already running work keeps its access; use Stop to end it.
                    </p>
                    <button
                      type="button"
                      className="workspace-settings-action"
                      disabled={busy}
                      onClick={() => void remove()}
                    >
                      {busy ? 'Removing…' : 'Remove folder'}
                    </button>
                    <button
                      type="button"
                      className="px-text-button"
                      disabled={busy}
                      onClick={() => {
                        setRemoving(null);
                        pending.current = null;
                        setError('');
                      }}
                    >
                      Keep folder
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {catalog?.can_manage && (
        <AgentFolderSetup
          client={client}
          disabled={busy || !!removing}
          workspaceOnly
          onActiveChange={setAdding}
          onAdded={async () => {
            await load();
            await onChanged();
          }}
        />
      )}
      {error && (
        <p role="alert" className="local-notice">
          {error}
        </p>
      )}
      {(catalog || error) && !adding && (
        <button
          type="button"
          className="px-text-button"
          disabled={busy}
          onClick={() => {
            setRemoving(null);
            pending.current = null;
            void load().catch((e) =>
              setError(e instanceof Error ? e.message : 'Could not refresh folders.'),
            );
          }}
        >
          Refresh folders
        </button>
      )}
    </section>
  );
}
