import { useLocalEngine } from '../../context/LocalEngineContext';
import { McpConnections } from '../views/AgentMcpTools';
import { toolDescription } from '../../lib/agentCapabilities';

const internalTools = new Set(['finish', 'dispatch_assignment', 'ask_human']);

export function EngineTools({ onAgent }: { onAgent: (key: string) => void }) {
  const engine = useLocalEngine();
  const { workspace, catalog, readErrors, isConnected } = engine;
  const tools = [
    ...new Set([
      ...(catalog?.tools || []),
      ...(workspace?.agents.flatMap((a) => a.tools || []) || []),
    ]),
  ];
  const workTools = tools.filter((tool) => !internalTools.has(tool));
  const coordinationTools = tools.filter((tool) => internalTools.has(tool));
  const catalogReady = isConnected && Array.isArray(catalog?.tools) && !readErrors['Agent setup'];
  return (
    <section className="tw-tools">
      <p>Tools let agents work with files and other services.</p>
      {(!isConnected || readErrors['Agent setup']) && (
        <p role="status">Tool information may be out of date.</p>
      )}
      {workTools.map((tool) => (
        <article className="px-team" key={tool}>
          <h3>{toolDescription([tool], catalog)}</h3>
          <p>
            {catalog?.tools?.includes(tool)
              ? 'Available on this host'
              : 'Recorded in an agent profile'}
          </p>
          {workspace?.agents
            .filter((a) => a.tools?.includes(tool))
            .map((a) => (
              <button className="px-text-button" key={a.key} onClick={() => onAgent(a.key)}>
                {a.name}
              </button>
            ))}
          {!workspace?.agents.some((a) => a.tools?.includes(tool)) && (
            <small>No agent configured with this tool.</small>
          )}
        </article>
      ))}
      {!workTools.length && (
        <p>
          {catalogReady
            ? 'No external or file tools are available in this connection. Agents can work with information you supply, but cannot act in other systems.'
            : 'Tool availability has not been confirmed. Reconnect to the engine to check access.'}
        </p>
      )}
      <h3>MCP connections</h3>
      {!catalogReady ? (
        <p>Reconnect to check configured services.</p>
      ) : !catalog?.mcp_connections ? (
        <p>Update and restart the engine to enable MCP connections.</p>
      ) : (
        <McpConnections
          connections={catalog?.mcp_connections || []}
          disabled={!catalogReady}
          onDiscover={async (id) => {
            await engine.client.discoverMcp(id);
            await engine.refresh();
          }}
        />
      )}
      <details>
        <summary>Add a connection to this engine</summary>
        <p>
          The engine operator can supply a JSON file with <code>--mcp-config</code>. Start an
          approved local HTTP MCP server separately, then list its exact read tools in the
          configuration. No server is installed or launched from this screen.
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
          Remote URLs, credentials, write tools and hosted-model MCP access are not supported yet.
          Server read-only hints are checked but are not a security guarantee; the operator must vet
          the server and its tools.
        </p>
      </details>
      {coordinationTools.length > 0 && (
        <details>
          <summary>Internal coordination</summary>
          <p>
            These controls manage replies and assignments. They do not provide access to your apps.
          </p>
          <ul>
            {coordinationTools.map((tool) => (
              <li key={tool}>{tool.replaceAll('_', ' ')}</li>
            ))}
          </ul>
        </details>
      )}
      <details>
        <summary>Access boundaries</summary>
        <p>
          Tool availability is not proof of activity. File tools stay within the host’s configured
          workspace; this screen does not change grants.
        </p>
      </details>
    </section>
  );
}
