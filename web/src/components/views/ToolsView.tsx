import { useEffect, useRef, useState } from 'react';
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  Database,
  Plug,
  Plus,
  Search,
  Terminal,
  X,
} from 'lucide-react';
import type { Team } from '../../types';
import {
  endpointError,
  resourceCatalog,
  resourceKindLabel,
  type ResourceKind,
  type ResourceTemplate,
  type WorkspaceResource,
} from '../../lib/toolLibrary';

interface Props {
  resources: WorkspaceResource[];
  teams: Team[];
  initialId?: string | null;
  initialTeam?: string;
  onAdd: (resource: WorkspaceResource) => void;
  onTeams: (id: string, teamIds: string[]) => void;
  onRemove: (id: string) => void;
  onActivity: (id: string) => void;
}
function ResourceIcon({ kind }: { kind: ResourceKind }) {
  const Icon = kind === 'mcp' ? Plug : kind === 'storage' ? Database : Terminal;
  return (
    <span className="tl-icon" data-kind={kind}>
      <Icon size={21} strokeWidth={1.6} />
    </span>
  );
}
const custom: ResourceTemplate = {
  id: 'custom',
  name: 'Custom MCP',
  description: 'Bring a remote MCP server your team already uses.',
  kind: 'mcp',
};
export function ToolsView({
  resources,
  teams,
  initialId,
  initialTeam = 'all',
  onAdd,
  onTeams,
  onRemove,
  onActivity,
}: Props) {
  const [page, setPage] = useState<'workspace' | 'catalog' | 'setup'>('workspace');
  const [selected, setSelected] = useState<string | null>(initialId || null);
  const [query, setQuery] = useState('');
  const [kind, setKind] = useState<'all' | 'mcp' | 'tool'>('all');
  const [teamId, setTeamId] = useState(initialTeam);
  const [template, setTemplate] = useState<ResourceTemplate>(custom);
  const [name, setName] = useState(''),
    [endpoint, setEndpoint] = useState(''),
    [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const lastRow = useRef<HTMLButtonElement | null>(null);
  const resource = resources.find((r) => r.id === selected);
  useEffect(() => {
    if (resource) detailHeading.current?.focus();
  }, [selected, page]);
  function setup(choice: ResourceTemplate) {
    setEditing(null);
    setTemplate(choice);
    setName(choice.id === 'custom' ? '' : choice.name);
    setEndpoint('');
    setError('');
    setPage('setup');
  }
  function show(id: string) {
    setSelected(id);
    setPage('workspace');
  }
  function closeDetails() {
    setSelected(null);
    requestAnimationFrame(() => lastRow.current?.focus());
  }
  const matching = resources.filter(
    (r) =>
      (kind === 'all' || (kind === 'mcp' ? r.kind === 'mcp' : r.kind !== 'mcp')) &&
      (teamId === 'all' ||
        (teamId === 'unassigned' ? !r.teamIds.length : r.teamIds.includes(teamId))) &&
      `${r.name} ${r.description}`.toLowerCase().includes(query.toLowerCase().trim()),
  );
  const choices = [...resourceCatalog, custom].filter(
    (r) =>
      (kind === 'all' || r.kind === kind) &&
      `${r.name} ${r.description}`.toLowerCase().includes(query.toLowerCase().trim()),
  );
  return (
    <div className="tool-library" data-detail-open={page === 'workspace' && !!resource}>
      <header className="tl-header">
        <div>
          <span className="tl-eyebrow">Workspace / resources</span>
          <h2>
            Tools <i>&</i> MCPs<span>.</span>
          </h2>
          <p>A shared toolkit. Give each team what it needs.</p>
        </div>
        {page === 'workspace' ? (
          <button
            className="canvas-primary"
            onClick={() => {
              setPage('catalog');
              setQuery('');
              setKind('all');
            }}
          >
            <Plus size={16} /> Add tool or MCP
          </button>
        ) : (
          <button
            className="quiet-back"
            onClick={() => {
              setPage('workspace');
              setQuery('');
            }}
          >
            <ArrowLeft size={15} /> Your toolkit
          </button>
        )}
      </header>
      <p className="tl-notice" role="status">
        {notice ||
          'Preview workspace · setup and team availability stay in this tab. No services are connected.'}
      </p>
      {page === 'setup' ? (
        <form
          className="tl-setup"
          onSubmit={(e) => {
            e.preventDefault();
            const problem = template.kind === 'mcp' ? endpointError(endpoint) : '';
            if (problem) {
              setError(problem);
              return;
            }
            if (!name.trim()) return;
            const duplicate =
              template.kind === 'mcp' &&
              resources.find(
                (r) =>
                  r.id !== editing &&
                  r.endpoint &&
                  new URL(r.endpoint).href === new URL(endpoint.trim()).href,
              );
            if (duplicate) {
              setError(
                `This server already has a setup: ${duplicate.name}. Open it from your toolkit.`,
              );
              return;
            }
            const id = editing || `resource-${crypto.randomUUID()}`;
            onAdd({
              id,
              name: name.trim(),
              description: template.description,
              kind: template.kind,
              source: 'draft',
              catalogId: template.id,
              endpoint: template.kind === 'mcp' ? endpoint.trim() : undefined,
              teamIds: editing
                ? resources.find((r) => r.id === editing)!.teamIds
                : teamId !== 'all' && teamId !== 'unassigned'
                  ? [teamId]
                  : [],
            });
            setNotice(
              `${name.trim()} saved as a setup draft. No connection or installation was made.`,
            );
            setQuery('');
            setKind('all');
            setTeamId('all');
            show(id);
          }}
        >
          <button type="button" className="quiet-back" onClick={() => setPage('catalog')}>
            <ArrowLeft size={15} /> Browse choices
          </button>
          <ResourceIcon kind={template.kind} />
          <h3>{template.id === 'custom' ? 'Bring your MCP.' : `Set up ${template.name}.`}</h3>
          <p>{template.description}</p>
          <label>
            Name
            <input
              autoFocus
              required
              maxLength={80}
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="A name your team recognizes"
            />
          </label>
          {template.kind === 'mcp' && (
            <label>
              Server URL
              <input
                type="url"
                aria-label="Server URL"
                required
                value={endpoint}
                onChange={(e) => {
                  setEndpoint(e.target.value);
                  setError('');
                }}
                placeholder="https://mcp.example.com/mcp"
                aria-describedby="mcp-endpoint-note"
                aria-invalid={!!error}
              />
              <small id="mcp-endpoint-note">
                Remote MCP endpoint, without credentials. Authentication comes when the engine is
                connected.
              </small>
            </label>
          )}
          {error && (
            <p role="alert" className="tl-error">
              {error}
            </p>
          )}
          <button className="canvas-primary" type="submit">
            Save setup <ArrowUpRight size={16} />
          </button>
          <p className="tl-hint">
            This saves the configuration for review. It does not download software, contact the
            server, or grant access.
          </p>
        </form>
      ) : (
        <>
          <div className="tl-controls">
            <label className="tl-search">
              <Search size={17} />
              <input
                aria-label="Search tools and MCPs"
                placeholder="Search tools and MCPs"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <div className="tl-filters" aria-label="Resource type">
              {(['all', 'mcp', 'tool'] as const).map((k) => (
                <button key={k} aria-pressed={kind === k} onClick={() => setKind(k)}>
                  {k === 'all' ? 'All' : k === 'mcp' ? 'MCPs' : 'Tools'}
                </button>
              ))}
            </div>
            {page === 'workspace' && (
              <select
                aria-label="Filter resources by team"
                value={teamId}
                onChange={(e) => setTeamId(e.target.value)}
              >
                <option value="all">All teams</option>
                <option value="unassigned">Unassigned</option>
                {teams.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            )}
          </div>
          {page === 'catalog' ? (
            <section className="tl-catalog" aria-label="Add tools and MCPs">
              <p className="tl-hint">
                Choose a starting point, or bring your own remote MCP. These are setup templates,
                not verified installations.
              </p>
              <div className="tl-catalog-grid">
                {choices.map((choice) => {
                  const existing = resources.find(
                    (r) => r.catalogId === choice.id && choice.id !== 'custom',
                  );
                  return (
                    <button
                      className="tl-catalog-choice"
                      key={choice.id}
                      onClick={() => (existing ? show(existing.id) : setup(choice))}
                    >
                      <ResourceIcon kind={choice.kind} />
                      <span>
                        <small>{resourceKindLabel(choice.kind)}</small>
                        <strong>{choice.name}</strong>
                        <p>{choice.description}</p>
                        <em>
                          {existing ? (
                            <>
                              <Check size={13} /> In your toolkit
                            </>
                          ) : (
                            <>
                              Set up <ArrowUpRight size={13} />
                            </>
                          )}
                        </em>
                      </span>
                    </button>
                  );
                })}
              </div>
              {!choices.length && (
                <p className="tl-empty">
                  No matching templates. Try another search or choose Custom MCP.
                </p>
              )}
            </section>
          ) : (
            <div className={'tl-workspace' + (resource ? ' has-selection' : '')}>
              <section className="tl-list" aria-label="Workspace resources">
                {(['mcp', 'tool'] as const).map((group) => {
                  const rows = matching.filter((r) =>
                    group === 'mcp' ? r.kind === 'mcp' : r.kind !== 'mcp',
                  );
                  return (
                    rows.length > 0 && (
                      <section key={group}>
                        <h3>
                          {group === 'mcp' ? 'MCP connections' : 'Tools & storage'}{' '}
                          <span>{rows.length}</span>
                        </h3>
                        {rows.map((item) => (
                          <button
                            className="tl-resource-row"
                            key={item.id}
                            aria-label={`Manage ${item.name}`}
                            aria-pressed={selected === item.id}
                            onClick={(e) => {
                              lastRow.current = e.currentTarget;
                              show(item.id);
                            }}
                          >
                            <ResourceIcon kind={item.kind} />
                            <span className="tl-row-copy">
                              <strong>{item.name}</strong>
                              <small>
                                {item.teamIds.length
                                  ? `${teams.find((t) => t.id === item.teamIds[0])?.name || 'Team'}${item.teamIds.length > 1 ? ` + ${item.teamIds.length - 1}` : ''}`
                                  : 'No teams assigned'}
                              </small>
                            </span>
                            <span className="tl-status" data-draft={item.source === 'draft'}>
                              {item.source === 'sample' ? 'Sample' : 'Setup draft'}
                            </span>
                            <ArrowUpRight size={14} />
                          </button>
                        ))}
                      </section>
                    )
                  );
                })}
                {!matching.length && (
                  <div className="tl-empty">
                    <h3>No resources here yet.</h3>
                    <p>
                      {query || teamId !== 'all' || kind !== 'all'
                        ? 'Try another search or team filter.'
                        : 'Start with a tool or bring an MCP connection.'}
                    </p>
                    <button
                      className="quiet-action"
                      onClick={() => {
                        setQuery('');
                        setKind('all');
                        setTeamId('all');
                      }}
                    >
                      Clear filters
                    </button>
                  </div>
                )}
              </section>
              {resource ? (
                <aside className="tl-detail" aria-label={`${resource.name} resource details`}>
                  <header>
                    <ResourceIcon kind={resource.kind} />
                    <button aria-label="Close resource details" onClick={closeDetails}>
                      <X size={17} />
                    </button>
                    <span>{resourceKindLabel(resource.kind)}</span>
                    <h3 ref={detailHeading} tabIndex={-1}>
                      {resource.name}
                    </h3>
                    <p>{resource.description}</p>
                  </header>
                  <div className="tl-detail-body">
                    <span className="tl-status" data-draft={resource.source === 'draft'}>
                      {resource.source === 'draft'
                        ? 'Setup draft · not connected'
                        : 'Sample resource · not live'}
                    </span>
                    {resource.endpoint && (
                      <div className="tl-endpoint">
                        <small>Server URL</small>
                        <code>{resource.endpoint}</code>
                      </div>
                    )}
                    {resource.kind === 'mcp' && resource.source === 'sample' && (
                      <button
                        className="tl-configure"
                        onClick={() =>
                          setup(resourceCatalog.find((c) => c.id === resource.catalogId) || custom)
                        }
                      >
                        Add another connection <Plus size={14} />
                      </button>
                    )}
                    {resource.source === 'draft' && (
                      <button
                        className="tl-configure"
                        onClick={() => {
                          setup(resourceCatalog.find((c) => c.id === resource.catalogId) || custom);
                          setEditing(resource.id);
                          setName(resource.name);
                          setEndpoint(resource.endpoint || '');
                        }}
                      >
                        Edit setup <ArrowUpRight size={14} />
                      </button>
                    )}
                    <fieldset>
                      <legend>Team availability</legend>
                      <p>
                        Choose teams for this preview. This does not change live permissions or
                        recorded activity.
                      </p>
                      <div className="tl-team-list">
                        {teams.map((t) => (
                          <label key={t.id}>
                            <input
                              type="checkbox"
                              checked={resource.teamIds.includes(t.id)}
                              onChange={(e) => {
                                onTeams(
                                  resource.id,
                                  e.target.checked
                                    ? [...resource.teamIds, t.id]
                                    : resource.teamIds.filter((id) => id !== t.id),
                                );
                                setNotice('Team availability updated in this preview.');
                              }}
                            />
                            <span>{t.name}</span>
                          </label>
                        ))}
                      </div>
                    </fieldset>
                    {resource.source === 'sample' ? (
                      <button className="tl-activity" onClick={() => onActivity(resource.id)}>
                        View recorded activity <ArrowUpRight size={15} />
                      </button>
                    ) : (
                      <div className="tl-draft-note">
                        <p>
                          Ready for connection setup when an engine is available. Drafts stay off
                          the active map.
                        </p>
                        <button
                          className="quiet-action"
                          onClick={() => {
                            onRemove(resource.id);
                            setSelected(null);
                            setNotice('Setup draft removed.');
                          }}
                        >
                          Remove draft
                        </button>
                      </div>
                    )}
                  </div>
                </aside>
              ) : (
                <aside className="tl-welcome">
                  <Plug size={28} strokeWidth={1.2} />
                  <h3>
                    Shared once.
                    <br />
                    Available where needed.
                  </h3>
                  <p>Select a resource to see its teams and recorded work.</p>
                  <p>
                    MCPs connect outside services. Runtime tools work inside the agent’s
                    environment.
                  </p>
                </aside>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
