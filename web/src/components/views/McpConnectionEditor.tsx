import { useRef, useState } from 'react';
import { CheckCircle2, Plug, RefreshCw } from 'lucide-react';
import { type LocalEngine } from '../../engine/client';
import { type McpConnection } from '../../engine/contracts';
import { endpointError, mcpConnectionName, normalizeMcpEndpoint } from '../../lib/toolLibrary';

/** One setup surface, shared by the library and the agent editor. No agent grants. */
export function McpConnectionEditor({
  client,
  connection,
  onChanged,
}: {
  client: LocalEngine;
  connection?: McpConnection;
  onChanged: () => Promise<void>;
}) {
  const [saved, setSaved] = useState(connection);
  const [id] = useState(
    () => connection?.id || crypto.randomUUID().replaceAll('-', '').slice(0, 20),
  );
  const [name, setName] = useState(connection?.name || '');
  const [endpoint, setEndpoint] = useState(connection?.endpoint || '');
  const [auth, setAuth] = useState<'none' | 'bearer'>(connection?.auth || 'none');
  const [token, setToken] = useState('');
  const [editing, setEditing] = useState(!connection);
  const [chosen, setChosen] = useState(
    connection?.tools.filter((t) => t.approved !== false).map((t) => t.id) || [],
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const lock = useRef(false);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  async function run(action: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError('');
    setMessage('');
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not update this connection.');
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  function adopt(next: McpConnection) {
    setSaved(next);
    setName(next.name);
    setEndpoint(next.endpoint);
    setAuth(next.auth || 'none');
    setChosen(next.tools.filter((t) => t.approved !== false).map((t) => t.id));
  }
  async function discover(record: McpConnection) {
    const checked = await client.discoverMcp(record.id);
    adopt(checked);
    await onChanged();
  }
  async function connect() {
    const invalid = endpointError(endpoint);
    if (invalid) {
      setError(invalid);
      return;
    }
    await run(async () => {
      try {
        const record = await client.saveMcpConnection({
          id,
          expected_revision: saved?.revision || 0,
          name: name.trim() || mcpConnectionName(endpoint),
          endpoint: normalizeMcpEndpoint(endpoint),
          auth,
          enabled: true,
          ...(auth === 'bearer' && token ? { token } : {}),
        });
        adopt(record);
        setEditing(false);
        await onChanged();
        await discover(record);
      } finally {
        setToken('');
      }
    });
  }
  return (
    <section
      className="mcp-connection-editor"
      aria-label={saved ? `Manage connection ${saved.name}` : 'Connect a service'}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target instanceof HTMLInputElement) e.preventDefault();
      }}
    >
      {editing ? (
        <fieldset disabled={busy} className="capability-editor">
          <legend>{saved ? 'Connection settings' : 'Connect a service'}</legend>
          <p>Paste the MCP address provided by your service. We’ll check it and find its tools.</p>
          <label>
            Service address
            <input
              autoFocus
              value={endpoint}
              readOnly={!!saved}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder="https://service.example/mcp"
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          {normalizeMcpEndpoint(endpoint).startsWith('http://') && !endpointError(endpoint) && (
            <p className="capability-scope-note">
              On this computer. The service needs to be running before you connect.
            </p>
          )}
          <details className="capability-source" open={!!saved}>
            <summary>Name and sign-in options</summary>
            <label>
              Name (optional)
              <input
                value={name}
                maxLength={80}
                onChange={(e) => setName(e.target.value)}
                placeholder={mcpConnectionName(endpoint) || 'My research service'}
              />
            </label>
            <label>
              Sign-in method
              <select value={auth} onChange={(e) => setAuth(e.target.value as 'none' | 'bearer')}>
                <option value="none">No sign-in</option>
                <option value="bearer">Service token</option>
              </select>
            </label>
            {auth === 'bearer' && (
              <label>
                {saved?.auth === 'bearer' && saved.enabled
                  ? 'Replacement token (optional)'
                  : 'Service token'}
                <input
                  type="password"
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  maxLength={8192}
                />
              </label>
            )}
            <p className="capability-scope-note">
              If your service gave you a token, add it here. Tokens are stored securely on this
              computer. Services that require browser sign-in aren’t supported yet.
            </p>
            {saved && (
              <p className="capability-scope-note">
                Changing sign-in details requires reviewing the tools and updating agents’ tool
                selections.
              </p>
            )}
          </details>
          <div className="capability-actions">
            <button
              type="button"
              className="canvas-primary"
              disabled={
                !endpoint.trim() ||
                (auth === 'bearer' && !token && !(saved?.auth === 'bearer' && saved.enabled))
              }
              onClick={() => void connect()}
            >
              {busy ? 'Connecting…' : saved ? 'Save & check connection' : 'Connect'}
            </button>
            {saved && (
              <button
                type="button"
                onClick={() => {
                  setEditing(false);
                  setToken('');
                  setName(saved.name);
                  setEndpoint(saved.endpoint);
                  setAuth(saved.auth || 'none');
                  setError('');
                }}
              >
                Cancel
              </button>
            )}
          </div>
        </fieldset>
      ) : (
        saved && (
          <>
            {!connection && <h4 className="mcp-connection-name">{saved.name}</h4>}
            <div className="mcp-connection-state" data-state={saved.status}>
              {saved.status === 'discovered' ? <CheckCircle2 size={18} /> : <Plug size={18} />}
              <div>
                <strong>
                  {saved.status === 'discovered'
                    ? 'Connected'
                    : saved.status === 'disconnected'
                      ? 'Disconnected'
                      : saved.status === 'unchecked'
                        ? 'Saved connection'
                        : 'Connection needs attention'}
                </strong>
                <p>{saved.message}</p>
              </div>
            </div>
            <div className="capability-actions">
              <button
                type="button"
                disabled={busy || saved.enabled === false}
                onClick={() => void run(() => discover(saved))}
              >
                <RefreshCw size={14} /> {busy ? 'Working…' : 'Check connection'}
              </button>
              <button type="button" disabled={busy} onClick={() => setEditing(true)}>
                {saved.enabled === false ? 'Reconnect' : 'Edit connection'}
              </button>
            </div>
            {saved.enabled !== false && !!saved.tools.length && (
              <fieldset disabled={busy} className="mcp-read-review">
                <legend>Available tools</legend>
                <p>
                  Choose which tools to make available to your workspace. You’ll assign them to
                  agents in their settings.
                </p>
                {saved.tools.map((tool) => (
                  <label
                    className="agent-tool-choice"
                    key={tool.id}
                    data-selected={chosen.includes(tool.id)}
                  >
                    <input
                      type="checkbox"
                      checked={chosen.includes(tool.id)}
                      onChange={(e) =>
                        setChosen(
                          e.target.checked
                            ? [...chosen, tool.id]
                            : chosen.filter((id) => id !== tool.id),
                        )
                      }
                    />
                    <span>
                      <strong>{tool.name}</strong>
                      <small>
                        {tool.read_only === true
                          ? 'Read-only (reported by service)'
                          : 'Action · can change state'}
                        {tool.read_only !== true && tool.destructive !== false
                          ? ' · may be destructive'
                          : ''}
                      </small>
                      <small>{tool.description}</small>
                    </span>
                  </label>
                ))}
                <button
                  type="button"
                  className="canvas-primary"
                  onClick={() =>
                    void run(async () => {
                      const record = await client.saveMcpConnection({
                        id: saved.id,
                        expected_revision: saved.revision!,
                        name: saved.name,
                        endpoint: saved.endpoint,
                        auth: saved.auth!,
                        enabled: true,
                        approved_tools: chosen,
                      });
                      adopt(record);
                      await onChanged();
                      setMessage(
                        'Workspace access saved. Select these tools in an agent’s settings to give them access.',
                      );
                    })
                  }
                >
                  Save tool access
                </button>
              </fieldset>
            )}
            {saved.status === 'discovered' && !saved.tools.length && (
              <p>
                No compatible tools were advertised. Tools that require background tasks are not
                supported yet.
              </p>
            )}
            {saved.enabled !== false && (
              <div className="mcp-disconnect">
                {confirmDisconnect ? (
                  <>
                    <p>
                      Disconnect {saved.name}? Agents will lose access to these tools. Completed
                      service actions cannot be undone.
                    </p>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void run(async () => {
                          const record = await client.saveMcpConnection({
                            id: saved.id,
                            expected_revision: saved.revision!,
                            name: saved.name,
                            endpoint: saved.endpoint,
                            auth: saved.auth!,
                            enabled: false,
                          });
                          adopt(record);
                          setConfirmDisconnect(false);
                          await onChanged();
                        })
                      }
                    >
                      Disconnect service
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => setConfirmDisconnect(false)}
                    >
                      Keep connected
                    </button>
                  </>
                ) : (
                  <button type="button" disabled={busy} onClick={() => setConfirmDisconnect(true)}>
                    Disconnect…
                  </button>
                )}
              </div>
            )}
          </>
        )
      )}
      {message && (
        <p className="capability-status" role="status">
          {message}
        </p>
      )}
      {error && (
        <p className="capability-status" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
