import { useLocalEngine } from '../../context/LocalEngineContext';

const internalTools = new Set(['finish', 'dispatch_assignment', 'ask_human']);

export function EngineTools({ onAgent }: { onAgent: (key: string) => void }) {
  const { workspace, catalog, readErrors, isConnected } = useLocalEngine();
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
          <h3>{tool.replaceAll('_', ' ')}</h3>
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
      <p>
        Connecting a service is not available in this preview yet. Asking an agent to use a service
        does not connect it or grant access.
      </p>
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
