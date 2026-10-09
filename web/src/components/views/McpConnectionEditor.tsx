import { useRef, useState } from 'react';
import { CheckCircle2, Plug, RefreshCw } from 'lucide-react';
import { type LocalEngine } from '../../engine/client';
import { type McpConnection } from '../../engine/contracts';
import { endpointError } from '../../lib/toolLibrary';

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
  const [auth, setAuth] = useState<'none' | 'bearer'>(connection?.auth || 'bearer');
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
          name: name.trim(),
          endpoint: endpoint.trim(),
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
          <p>Connect once, then choose which agents can use its tools.</p>
          <label>
            Name
            <input
              autoFocus
              value={name}
              maxLength={80}
              onChange={(e) => setName(e.target.value)}
              placeholder="My research service"
            />
          </label>
          <label>
            MCP endpoint
            <input
              value={endpoint}
              readOnly={!!saved}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder="https://service.example/mcp"
              autoComplete="off"
            />
          </label>
          <label>
            Authentication
            <select value={auth} onChange={(e) => setAuth(e.target.value as 'none' | 'bearer')}>
              <option value="bearer">Service token</option>
              <option value="none">No authentication</option>
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
            Tokens stay in your computer’s credential store. Use a token issued for this service.
            Browser sign-in (OAuth) is not supported yet.
          </p>
          {saved && (
            <p className="capability-scope-note">
              Replacing credentials clears the tool review. Agents keep their old selections until
              you update them.
            </p>
          )}
          <div className="capability-actions">
            <button
              type="button"
              className="canvas-primary"
              disabled={
                !name.trim() ||
                !endpoint.trim() ||
                (auth === 'bearer' && !token && !(saved?.auth === 'bearer' && saved.enabled))
              }
              onClick={() => void connect()}
            >
              {busy ? 'Connecting…' : saved ? 'Save & check connection' : 'Connect & review tools'}
            </button>
            {saved && (
              <button
                type="button"
                onClick={() => {
                  setEditing(false);
                  setToken('');
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
                <legend>Available read tools</legend>
                <p>
                  Enable only tools you trust to read from this service. Agents still need their own
                  selection.
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
                No compatible read tools were advertised. Write tools and background tasks are not
                available in this profile.
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
