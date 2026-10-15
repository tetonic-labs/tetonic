import { useRef, useState } from 'react';
import { Layers, Plus } from 'lucide-react';
import type { AgentCatalog, EngineAgent, SaveToolPack, ToolPack } from '../../engine/contracts';
import type { LocalEngine } from '../../engine/client';
import { packToolLabel } from '../../lib/toolPacks';
import '../../tool-packs.css';

export function ToolPackLibrary({
  client,
  catalog,
  agents = [],
  onChanged,
}: {
  client: LocalEngine;
  catalog: AgentCatalog;
  agents?: EngineAgent[];
  onChanged: () => Promise<void>;
}) {
  const [editing, setEditing] = useState<{ pack?: ToolPack; copy: boolean } | null>(null);
  const [message, setMessage] = useState('');
  if (!catalog.tool_packs) return null;
  return (
    <section className="tool-packs tp-library" aria-label="Workspace tool packs">
      <div className="tp-heading">
        <div>
          <h3>
            <Layers size={17} /> Tool packs
          </h3>
          <p>Reusable sets for your agents. Build one from tools, MCPs and skills already here.</p>
        </div>
        {!editing && (
          <button
            type="button"
            className="canvas-secondary"
            onClick={() => {
              setMessage('');
              setEditing({ copy: false });
            }}
          >
            <Plus size={15} /> Create pack
          </button>
        )}
      </div>
      {message && (
        <p role="status" className="tp-notice">
          {message}
        </p>
      )}
      {editing ? (
        <ToolPackEditor
          key={`${editing.pack?.id || 'new'}-${editing.copy}`}
          client={client}
          catalog={catalog}
          pack={editing.pack}
          copy={editing.copy}
          onCancel={() => setEditing(null)}
          onSaved={async (pack) => {
            await onChanged();
            setEditing(null);
            setMessage(
              `${pack.name} saved. Select it in an agent’s settings. Existing agents keep their saved version until you update them.`,
            );
          }}
        />
      ) : (
        <div className="tp-grid">
          {catalog.tool_packs.map((pack) => {
            const users = agents.filter((a) => a.tool_packs?.some((p) => p.id === pack.id));
            const older = users.filter((a) =>
              a.tool_packs?.some((p) => p.id === pack.id && p.revision < pack.revision),
            );
            return (
              <article className="tp-card" key={pack.id}>
                <div className="tp-card-title">
                  <strong>{pack.name}</strong>
                  <small>{pack.builtin ? 'Built in' : `v${pack.revision}`}</small>
                </div>
                <p>{pack.description}</p>
                <p className="tp-count">
                  {pack.tools.length} {pack.tools.length === 1 ? 'tool' : 'tools'}
                  {users.length ? ` · ${users.length} agent${users.length === 1 ? '' : 's'}` : ''}
                </p>
                {older.length > 0 && (
                  <p className="tp-notice">
                    Update available for {older.map((a) => a.name).join(', ')}.
                  </p>
                )}
                <button
                  type="button"
                  className="tp-link"
                  onClick={() => {
                    setMessage('');
                    setEditing({ pack, copy: pack.builtin });
                  }}
                >
                  {pack.builtin ? 'Customize a copy' : 'Edit pack'}
                </button>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}

function ToolPackEditor({
  client,
  catalog,
  pack,
  copy,
  onCancel,
  onSaved,
}: {
  client: LocalEngine;
  catalog: AgentCatalog;
  pack?: ToolPack;
  copy: boolean;
  onCancel: () => void;
  onSaved: (p: ToolPack) => Promise<void>;
}) {
  const [name, setName] = useState(copy ? `${pack?.name || ''} copy` : pack?.name || '');
  const [description, setDescription] = useState(pack?.description || '');
  const [selected, setSelected] = useState(pack?.tools || []);
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const id = useRef(copy || !pack ? crypto.randomUUID() : pack.id);
  const pending = useRef<SaveToolPack | null>(null);
  const locked = useRef(false);
  const tools = [...new Set([...(catalog.tools || []), ...selected])];
  const missing = selected.filter((t) => !catalog.tools?.includes(t));
  async function save() {
    if (locked.current || !name.trim() || !selected.length || missing.length) return;
    const input: SaveToolPack = {
      request_id: pending.current?.request_id || crypto.randomUUID(),
      id: id.current,
      expected_revision: copy ? 0 : pack?.revision || 0,
      name: name.trim(),
      description: description.trim(),
      tools: [...selected].sort(),
    };
    if (pending.current && JSON.stringify(input) !== JSON.stringify(pending.current))
      input.request_id = crypto.randomUUID();
    pending.current = input;
    locked.current = true;
    setBusy(true);
    setError('');
    try {
      const saved = await client.saveToolPack(input);
      await onSaved(saved);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save this pack.');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  return (
    <div
      className="tp-editor"
      role="group"
      aria-label={pack && !copy ? 'Edit tool pack' : 'Create tool pack'}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target instanceof HTMLInputElement) e.preventDefault();
      }}
    >
      <label>
        Pack name
        <input
          value={name}
          maxLength={60}
          disabled={busy}
          placeholder="e.g. Research toolkit"
          onChange={(e) => setName(e.target.value)}
        />
      </label>
      <label>
        What is it for?
        <input
          value={description}
          maxLength={1000}
          disabled={busy}
          placeholder="A short description for your team"
          onChange={(e) => setDescription(e.target.value)}
        />
      </label>
      <div className="tp-heading">
        <strong>{selected.length} tools selected</strong>
        <input
          aria-label="Find pack tools"
          placeholder="Find tools, services or skills"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className="tp-tool-list">
        {(['Core tools', 'Connected services', 'Skills'] as const).map((group) => {
          const matches = tools.filter(
            (t) =>
              (t.startsWith('mcp_')
                ? 'Connected services'
                : t.startsWith('skill_')
                  ? 'Skills'
                  : 'Core tools') === group &&
              packToolLabel(t, catalog).toLowerCase().includes(query.toLowerCase()),
          );
          return matches.length ? (
            <fieldset key={group}>
              <legend>{group}</legend>
              {matches.map((t) => (
                <label key={t}>
                  <input
                    type="checkbox"
                    checked={selected.includes(t)}
                    disabled={busy || (!catalog.tools?.includes(t) && !selected.includes(t))}
                    onChange={(e) =>
                      setSelected(
                        e.target.checked ? [...selected, t] : selected.filter((x) => x !== t),
                      )
                    }
                  />
                  <span>
                    {packToolLabel(t, catalog)}
                    {!catalog.tools?.includes(t) && (
                      <small>Unavailable — remove or restore access</small>
                    )}
                  </span>
                </label>
              ))}
            </fieldset>
          ) : null;
        })}
        {!tools.length && (
          <p>Add a working folder, connect a service or import a skill to make tools available.</p>
        )}
      </div>
      <p className="tp-footnote">
        Saving a pack does not grant access or change existing agents. Choose it when creating or
        editing an agent.
      </p>
      {error && (
        <p role="alert" className="tp-notice">
          {error}
        </p>
      )}
      <div className="tp-actions">
        <button
          type="button"
          className="canvas-primary"
          disabled={busy || !name.trim() || !selected.length || !!missing.length}
          onClick={() => void save()}
        >
          {busy ? 'Saving…' : 'Save pack'}
        </button>
        <button type="button" disabled={busy} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
