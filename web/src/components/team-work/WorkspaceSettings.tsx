import { Monitor, Radio, ShieldCheck, CircleStop } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import './workspace-settings.css';
import { CapabilityPolicyEditor } from '../views/CapabilityPolicyEditor';

export function WorkspaceSettings({
  dark,
  setDark,
}: {
  dark: boolean;
  setDark: (value: boolean) => void;
}) {
  const engine = useLocalEngine();
  const { workspace, readErrors, approvals } = engine;
  const issues = Object.entries(readErrors);
  const status = engine.isConnecting
    ? 'Connecting'
    : !engine.isConnected
      ? 'Disconnected'
      : issues.length
        ? 'Needs attention'
        : 'Connected';
  return (
    <div className="tw-settings">
      <section className="workspace-setting-row">
        <Monitor size={19} aria-hidden="true" />
        <div>
          <h3>Appearance</h3>
          <p>Make this space your own.</p>
        </div>
        <label className="workspace-theme-toggle">
          <input
            type="checkbox"
            role="switch"
            aria-label="Dark appearance"
            checked={dark}
            onChange={(event) => setDark(event.target.checked)}
          />
          <span aria-hidden="true" />
          Dark
        </label>
      </section>
      <CapabilityPolicyEditor
        scope="workspace"
        client={engine.client}
        isConnected={engine.isConnected}
      />
      <section className="workspace-connection">
        <div className="workspace-setting-row">
          <Radio size={19} aria-hidden="true" />
          <div>
            <h3>Engine connection</h3>
            <p>{workspace?.organization || 'Your local engine'}</p>
          </div>
          <span
            className="workspace-connection-status"
            data-ready={status === 'Connected'}
            role="status"
          >
            {status}
          </span>
        </div>
        {!engine.isConnected && !engine.isConnecting && (
          <p className="workspace-settings-note">
            Open the connection link printed by your local Tetonic engine.
          </p>
        )}
        <div className="workspace-connection-actions">
          <details open={issues.length > 0 || undefined}>
            <summary>Connection details</summary>
            <p>
              {engine.error ||
                (engine.isConnected ? 'Receiving engine state.' : 'Waiting for a connection.')}
            </p>
            {issues.map(([name, error]) => (
              <p key={name}>
                <strong>{name}</strong>
                <br />
                {error}
              </p>
            ))}
          </details>
          <button
            type="button"
            className="workspace-settings-action"
            onClick={engine.reconnect}
            disabled={engine.isConnecting}
          >
            Reconnect
          </button>
        </div>
      </section>
      {!!approvals?.active_stops.length && (
        <section className="workspace-restrictions" aria-label="Active restrictions">
          <h3>Active restrictions</h3>
          {approvals.active_stops.map((stop) => (
            <p key={`${stop.scope_kind}:${stop.scope_id}`}>
              <strong>{stop.mode}</strong> · {stop.reason} ({stop.scope_kind})
            </p>
          ))}
        </section>
      )}
      <section className="workspace-setting-help">
        <ShieldCheck size={19} aria-hidden="true" />
        <details>
          <summary>Who can see this work?</summary>
          <p>
            This is the local owner workspace. Requests and replies are saved in its engine storage.
            A configured hosted model may receive the conversation; check the agent’s settings for
            its provider. This is not a separate private chat boundary.
          </p>
        </details>
      </section>
      <section className="workspace-setting-help">
        <CircleStop size={19} aria-hidden="true" />
        <details>
          <summary>How do I stop work?</summary>
          <p>
            Open a running request and choose <strong>Stop this request</strong>. This asks the
            engine to stop that execution; it does not undo completed actions. Workspace-wide
            emergency stop is not exposed by this connection yet.
          </p>
        </details>
      </section>
    </div>
  );
}
