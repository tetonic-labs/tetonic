import { useState } from 'react';
import { Layers } from 'lucide-react';
import type { AgentCatalog, ToolPack } from '../../engine/contracts';
import { changePack, packToolLabel } from '../../lib/toolPacks';
import '../../tool-packs.css';

export function AgentToolPacks({
  catalog,
  packs,
  tools,
  supported,
  disabled,
  onChange,
}: {
  catalog: AgentCatalog;
  packs: ToolPack[];
  tools: string[];
  supported: string[];
  disabled: boolean;
  onChange: (packs: ToolPack[], tools: string[]) => void;
}) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const [updating, setUpdating] = useState<string | null>(null);
  const available = [
    ...(catalog.tool_packs || []),
    ...packs.filter((p) => !catalog.tool_packs?.some((x) => x.id === p.id)),
  ];
  function change(id: string, pack?: ToolPack) {
    const next = changePack(packs, tools, id, pack);
    onChange(next.packs, next.tools);
    setUpdating(null);
  }
  if (!available.length) return null;
  return (
    <section className="tool-packs" aria-label="Agent tool packs">
      <div className="tp-heading">
        <div>
          <h3>
            <Layers size={16} /> Start with a tool pack
          </h3>
          <p>Pick a set, then adjust the tools below. Permissions still apply.</p>
        </div>
      </div>
      <div className="tp-grid">
        {available.map((latest) => {
          const pinned = packs.find((p) => p.id === latest.id);
          const pack = pinned || latest;
          const missing = pack.tools.filter((t) => !supported.includes(t));
          const update = !!pinned && latest.revision > pinned.revision;
          const additions = latest.tools.filter((t) => !pack.tools.includes(t));
          const removals = pack.tools.filter((t) => !latest.tools.includes(t));
          const show = expanded === pack.id;
          return (
            <article className="tp-card" key={pack.id} data-selected={!!pinned}>
              <label className="tp-choice">
                <input
                  type="checkbox"
                  aria-label={`Use ${pack.name} pack`}
                  checked={!!pinned}
                  disabled={disabled || (!pinned && !!missing.length)}
                  onChange={(e) => change(pack.id, e.target.checked ? latest : undefined)}
                />
                <span>
                  <strong>{pack.name}</strong>
                  <small>
                    {pinned
                      ? `${pack.tools.filter((t) => tools.includes(t)).length} of ${pack.tools.length} selected · v${pack.revision}`
                      : `${pack.tools.length} ${pack.tools.length === 1 ? 'tool' : 'tools'}${pack.builtin ? ' · Built in' : ''}`}
                  </small>
                </span>
              </label>
              <p>{pack.description}</p>
              {!!missing.length && (
                <p className="tp-notice">
                  Needs access to: {missing.map((t) => packToolLabel(t, catalog)).join(', ')}. Add a
                  working folder or reconnect the service.
                </p>
              )}
              <div className="tp-actions">
                <button
                  type="button"
                  aria-expanded={show}
                  onClick={() => setExpanded(show ? null : pack.id)}
                >
                  {show ? 'Hide tools' : 'View tools'}
                </button>
                {update && (
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => setUpdating(updating === pack.id ? null : pack.id)}
                  >
                    Review v{latest.revision}
                  </button>
                )}
              </div>
              {show && (
                <ul className="tp-members">
                  {pack.tools.map((t) => (
                    <li key={t}>
                      <label>
                        <input
                          type="checkbox"
                          checked={tools.includes(t)}
                          disabled={
                            disabled || !pinned || (!supported.includes(t) && !tools.includes(t))
                          }
                          onChange={(e) =>
                            onChange(
                              packs,
                              e.target.checked
                                ? [...new Set([...tools, t])]
                                : tools.filter((x) => x !== t),
                            )
                          }
                        />
                        {packToolLabel(t, catalog)}
                        {!supported.includes(t) && <small>Unavailable</small>}
                      </label>
                    </li>
                  ))}
                </ul>
              )}
              {updating === pack.id && (
                <div className="tp-update" role="region" aria-label={`Update ${pack.name}`}>
                  <strong>Update to v{latest.revision}</strong>
                  <p>
                    {additions.length
                      ? `Adds: ${additions.map((t) => packToolLabel(t, catalog)).join(', ')}.`
                      : 'No tools added.'}
                  </p>
                  <p>
                    {removals.length
                      ? `Removes: ${removals.map((t) => packToolLabel(t, catalog)).join(', ')}.`
                      : 'No tools removed.'}
                  </p>
                  <p>
                    Your exclusions are kept. Applies when you save this agent; active work keeps
                    its current tools.
                  </p>
                  <button
                    type="button"
                    disabled={disabled || additions.some((t) => !supported.includes(t))}
                    onClick={() => change(pack.id, latest)}
                  >
                    Use this version
                  </button>
                  <button type="button" onClick={() => setUpdating(null)}>
                    Keep v{pack.revision}
                  </button>
                </div>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}
