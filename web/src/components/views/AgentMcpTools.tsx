import { useRef, useState } from 'react';
import type { McpConnection, McpTool } from '../../lib/localEngine';

export function McpConnections({
  connections,
  onDiscover,
  disabled,
}: {
  connections: McpConnection[];
  onDiscover: (id: string) => Promise<void>;
  disabled?: boolean;
}) {
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  return (
    <div>
      {!connections.length && <p>No MCP servers are configured on this engine.</p>}
      {connections.map((connection) => (
        <article className="px-team" key={connection.id}>
          <h4>{connection.name}</h4>
          <p>
            {connection.status === 'discovered'
              ? `${connection.tools.length} read tools discovered`
              : connection.status === 'unchecked'
                ? 'Not checked yet'
                : 'Connection needs attention'}
          </p>
          <small>{connection.message}</small>
          <button
            type="button"
            className="px-text-button"
            disabled={disabled || !!busy}
            onClick={async () => {
              setBusy(connection.id);
              setError('');
              try {
                await onDiscover(connection.id);
              } catch (e) {
                setError(e instanceof Error ? e.message : 'Could not check this connection.');
              } finally {
                setBusy('');
              }
            }}
          >
            {busy === connection.id
              ? 'Checking connection…'
              : connection.status === 'unchecked'
                ? `Discover ${connection.name} tools`
                : `Refresh ${connection.name} tools`}
          </button>
          <details>
            <summary>Connection details</summary>
            <p>{connection.endpoint}</p>
            <p>
              Configured by the engine operator. Only explicitly allowed read tools are offered.
              Connecting does not give an agent access.
            </p>
          </details>
        </article>
      ))}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}

export function AgentMcpTools({
  connections,
  selected,
  supported,
  onSelect,
  onDiscover,
  disabled,
}: {
  connections: McpConnection[];
  selected: string[];
  supported: string[];
  onSelect: (names: string[]) => void;
  onDiscover?: (id: string) => Promise<void>;
  disabled?: boolean;
}) {
  const known = useRef(new Map<string, McpTool & { connection: string }>());
  const current = connections.flatMap((connection) =>
    connection.tools.map((tool) => ({ ...tool, connection: connection.name })),
  );
  current.forEach((tool) => known.current.set(tool.id, tool));
  const ids = [
    ...new Set([
      ...current.map((tool) => tool.id),
      ...selected.filter((id) => id.startsWith('mcp_')),
    ]),
  ];
  return (
    <section>
      <h3>Connected services</h3>
      <p>
        Choose the individual tools this teammate can use. MCP tools currently work with local
        models.
      </p>
      {onDiscover && (
        <McpConnections connections={connections} onDiscover={onDiscover} disabled={disabled} />
      )}
      <div className="agent-tool-grid">
        {ids.map((id) => {
          const tool = known.current.get(id);
          const checked = selected.includes(id);
          const available = supported.includes(id);
          return (
            <label className="agent-tool-choice" key={id} data-selected={checked}>
              <input
                type="checkbox"
                aria-label={tool ? `${tool.connection}: ${tool.name}` : 'Unavailable MCP tool'}
                checked={checked}
                disabled={disabled || (!checked && !available)}
                onChange={(e) =>
                  onSelect(
                    e.target.checked ? [...selected, id] : selected.filter((name) => name !== id),
                  )
                }
              />
              <span>
                <strong>{tool?.name || 'Unavailable MCP tool'}</strong>
                <small>{tool?.connection}</small>
                <small>
                  {available
                    ? tool?.description
                    : 'Unavailable with this setup. Remove this selection or restore access.'}
                </small>
              </span>
            </label>
          );
        })}
      </div>
    </section>
  );
}
