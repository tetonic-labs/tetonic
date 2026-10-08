import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  ArrowLeft,
  ArrowUpRight,
  BookOpen,
  FolderOpen,
  Plug,
  Plus,
  Search,
  Terminal,
  X,
} from 'lucide-react';
import type { McpConnection } from '../../lib/localEngine';
import '../../tools.css';

export interface ToolkitResource {
  id: string;
  name: string;
  description: string;
  kind: 'tool' | 'mcp' | 'skill';
  detail?: ReactNode;
  revoked?: boolean;
  tools: { id: string; name: string; available: boolean }[];
  agents: { key: string; name: string }[];
  connection?: McpConnection;
}
function ResourceIcon({ resource }: { resource: Pick<ToolkitResource, 'kind' | 'id'> }) {
  const Icon =
    resource.kind === 'skill'
      ? BookOpen
      : resource.kind === 'mcp'
        ? Plug
        : resource.id === 'files'
          ? FolderOpen
          : Terminal;
  return (
    <span className="tl-icon" data-kind={resource.kind}>
      <Icon size={21} strokeWidth={1.6} />
    </span>
  );
}
function resourceStatus(resource: ToolkitResource, ready: boolean) {
  if (!ready) return 'Unchecked';
  if (resource.revoked) return 'Revoked';
  if (resource.connection?.status === 'unchecked') return 'Not checked';
  if (resource.connection?.status === 'unavailable') return 'Needs attention';
  const count = resource.tools.filter((t) => t.available).length;
  if (!count) return 'Unavailable';
  return resource.kind === 'mcp'
    ? `${count} tools`
    : count < resource.tools.length
      ? 'Partial access'
      : 'Available';
}

// The established toolkit layout, backed by engine state instead of preview resources.
export function ToolsView({
  resources,
  library,
  ready,
  workspaceRoot,
  mcpSupported,
  onAgent,
  onDiscover,
}: {
  resources: ToolkitResource[];
  library?: ReactNode;
  ready: boolean;
  workspaceRoot?: string | null;
  mcpSupported: boolean;
  onAgent: (key: string) => void;
  onDiscover: (id: string) => Promise<void>;
}) {
  const [setup, setSetup] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [kind, setKind] = useState<'all' | 'mcp' | 'tool' | 'skill'>('all');
  const [busy, setBusy] = useState('');
  const [error, setError] = useState<{ id: string; message: string } | null>(null);
  const locked = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const lastRow = useRef<HTMLButtonElement | null>(null);
  const resource = resources.find((r) => r.id === selected);
  const matching = resources.filter(
    (r) =>
      (kind === 'all' || r.kind === kind) &&
      `${r.name} ${r.description} ${r.tools.map((t) => t.name).join(' ')}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
  );
  useEffect(() => {
    if (selected || setup) heading.current?.focus();
  }, [selected, setup]);
  function closeDetails() {
    setSelected(null);
    setError(null);
    requestAnimationFrame(() => lastRow.current?.focus());
  }
  async function discover(id: string) {
    if (locked.current || !ready) return;
    locked.current = true;
    setBusy(id);
    setError(null);
    try {
      await onDiscover(id);
    } catch (e) {
      setError({
        id,
        message: e instanceof Error ? e.message : 'Could not check this connection.',
      });
    } finally {
      locked.current = false;
      setBusy('');
    }
  }
  return (
    <div className="tool-library" data-detail-open={!setup && !!resource}>
      <header className="tl-header">
        <p>A shared toolkit. Give each agent what they need.</p>
        {setup ? (
          <button className="quiet-back" onClick={() => setSetup(false)}>
            <ArrowLeft size={15} /> Your toolkit
          </button>
        ) : (
          <button className="canvas-primary" onClick={() => setSetup(true)}>
            <Plus size={16} /> Add tools, MCPs or skills
          </button>
        )}
      </header>
      {!ready && (
        <p className="tl-notice" role="status">
          Tool information may be out of date. Reconnect to check availability.
        </p>
      )}
      {setup ? (
        <section className="tl-setup">
          {library}
          <details>
            <summary>Operator setup for tools & MCPs</summary>
            <ResourceIcon resource={{ kind: 'mcp', id: 'setup' }} />
            <h3 ref={heading} tabIndex={-1}>
              Connect your toolkit.
            </h3>
            <p>
              File and Terminal tools come from your engine. Select the tools an agent can use when
              you create them in Agents.
            </p>
            <h4>Add an MCP connection</h4>
            <p>
              Your engine operator can connect a local HTTP MCP server using{' '}
              <code>--mcp-config</code>. It will appear in your toolkit after the engine restarts.
            </p>
            <details>
              <summary>Engine configuration</summary>
              <p>
                Start the approved MCP server separately, then pass a JSON file listing its endpoint
                and exact allowed read tools.
              </p>
              <pre>
                {JSON.stringify(
                  {
                    connections: [
                      {
                        id: 'knowledge',
                        name: 'Knowledge library',
                        endpoint: 'http://127.0.0.1:8765/mcp',
                        read_tools: ['search', 'lookup'],
                      },
                    ],
                  },
                  null,
                  2,
                )}
              </pre>
              <p>
                Remote servers, authentication and MCP write tools are not supported by this
                connection yet. The operator must vet the server and its tools.
              </p>
            </details>
            <p className="tl-hint">
              This screen does not install servers or grant access. Hosted models need your consent
              to receive selected tool inputs and results.
            </p>
          </details>
        </section>
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
              {(['all', 'mcp', 'tool', 'skill'] as const).map((value) => (
                <button key={value} aria-pressed={kind === value} onClick={() => setKind(value)}>
                  {value === 'all'
                    ? 'All'
                    : value === 'mcp'
                      ? 'MCPs'
                      : value === 'skill'
                        ? 'Skills'
                        : 'Tools'}
                </button>
              ))}
            </div>
          </div>
          <div className={'tl-workspace' + (resource ? ' has-selection' : '')}>
            <section className="tl-list" aria-label="Workspace resources">
              {(['skill', 'mcp', 'tool'] as const)
                .filter((group) => kind === 'all' || kind === group)
                .map((group) => {
                  const rows = matching.filter((r) => r.kind === group);
                  return (
                    <section key={group}>
                      <h3>
                        {group === 'mcp'
                          ? 'MCP connections'
                          : group === 'skill'
                            ? 'Skills'
                            : 'Tools'}{' '}
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
                            setSelected(item.id);
                            setError(null);
                          }}
                        >
                          <ResourceIcon resource={item} />
                          <span className="tl-row-copy">
                            <strong>{item.name}</strong>
                            <small>
                              {item.agents.length
                                ? `${item.agents.length} ${item.agents.length === 1 ? 'agent' : 'agents'} ${item.revoked ? 'need attention' : 'with access'}`
                                : 'No agents assigned'}
                            </small>
                          </span>
                          <span className="tl-status">{resourceStatus(item, ready)}</span>
                          <ArrowUpRight size={14} />
                        </button>
                      ))}
                      {!rows.length && (
                        <p className="tl-empty">
                          {query
                            ? 'No matches. Try another search.'
                            : group === 'skill'
                              ? 'Add a skill, then select it in an agent’s settings.'
                              : group === 'mcp'
                                ? !ready
                                  ? 'Reconnect to check configured services.'
                                  : !mcpSupported
                                    ? 'Update and restart the engine to enable MCP connections.'
                                    : 'No MCP servers are configured on this engine.'
                                : ready
                                  ? 'No local tools are available in this connection.'
                                  : 'Tool availability has not been confirmed.'}
                        </p>
                      )}
                    </section>
                  );
                })}
            </section>
            {resource ? (
              <aside className="tl-detail" aria-label={`${resource.name} resource details`}>
                <header>
                  <ResourceIcon resource={resource} />
                  <button aria-label="Close resource details" onClick={closeDetails}>
                    <X size={17} />
                  </button>
                  <span>
                    {resource.kind === 'mcp'
                      ? 'MCP connection'
                      : resource.kind === 'skill'
                        ? 'Skill'
                        : 'Runtime tools'}
                  </span>
                  <h3 ref={heading} tabIndex={-1}>
                    {resource.name}
                  </h3>
                  <p>{resource.description}</p>
                </header>
                <div className="tl-detail-body">
                  {resource.detail}
                  <span className="tl-status">{resourceStatus(resource, ready)}</span>
                  {!!resource.connection && (
                    <>
                      <p className="tl-hint">{resource.connection.message}</p>
                      <button
                        className="tl-configure"
                        disabled={!ready || !!busy}
                        onClick={() => void discover(resource.connection!.id)}
                      >
                        {busy === resource.connection.id
                          ? 'Checking connection…'
                          : `${resource.connection.status === 'unchecked' ? 'Discover' : 'Refresh'} ${resource.name} tools`}
                      </button>
                      {error?.id === resource.connection.id && <p role="alert">{error.message}</p>}
                    </>
                  )}
                  {!!resource.tools.length && (
                    <section className="tl-capabilities" aria-label="Included tools">
                      <h4>Included tools</h4>
                      <ul>
                        {resource.tools.map((tool) => (
                          <li key={tool.id}>
                            <span>{tool.name}</span>
                            {!tool.available && (
                              <small>{ready ? 'Unavailable' : 'Unchecked'}</small>
                            )}
                          </li>
                        ))}
                      </ul>
                    </section>
                  )}
                  <fieldset>
                    <legend>
                      {resource.revoked ? 'Agents needing attention' : 'Agents with access'}
                    </legend>
                    <p>Saved tool selections. Check an agent to see their exact access.</p>
                    <div className="tl-team-list">
                      {resource.agents.map((agent) => (
                        <button
                          className="tl-agent"
                          key={agent.key}
                          onClick={() => onAgent(agent.key)}
                        >
                          {agent.name}
                          <ArrowUpRight size={14} />
                        </button>
                      ))}
                    </div>
                    {!resource.agents.length && <p>No agent configured with this tool.</p>}
                  </fieldset>
                  {resource.kind !== 'skill' && (
                    <details className="tl-connection-details">
                      <summary>
                        {resource.kind === 'mcp'
                          ? 'Connection details'
                          : 'Working folder and access'}
                      </summary>
                      {resource.connection ? (
                        <>
                          <p>{resource.connection.endpoint}</p>
                          <p>
                            Only operator-approved tools are offered. Checking a connection does not
                            give an agent access.
                          </p>
                        </>
                      ) : (
                        <>
                          <p>{workspaceRoot || 'No working folder is configured.'}</p>
                          <p>
                            {resource.id === 'run_shell'
                              ? 'Review each command in Needs you before it runs. The working folder is not a security boundary; available isolation depends on this computer.'
                              : resource.kind === 'mcp'
                                ? 'Restore this connection on the engine to make these saved tools available again.'
                                : 'File tools stay within the configured folder. Select individual permissions when creating an agent.'}
                          </p>
                        </>
                      )}
                    </details>
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
                <p>Select a resource to see its tools and the agents with access.</p>
                <p>
                  MCPs connect outside services. Runtime tools work inside the agent’s environment.
                </p>
              </aside>
            )}
          </div>
        </>
      )}
    </div>
  );
}
