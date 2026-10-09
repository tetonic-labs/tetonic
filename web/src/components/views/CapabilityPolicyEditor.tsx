import { useEffect, useId, useRef, useState } from 'react';
import type { LocalEngine } from '../../engine/client';
import { EngineRequestError } from '../../engine/failure';
import type {
  AutonomyTier,
  Capability,
  CapabilityDecision,
  CapabilityPolicy,
  SaveCapabilityPolicy,
  ScopedCapabilityPolicy,
} from '../../engine/contracts';
import './capability-policy.css';

const capabilities: { id: Capability; name: string; scope: string }[] = [
  { id: 'file_read', name: 'Read files', scope: 'Within the agent’s assigned working folder.' },
  { id: 'file_write', name: 'Change files', scope: 'Within the agent’s assigned working folder.' },
  {
    id: 'shell',
    name: 'Run commands',
    scope: 'Granted command tools. Host approval and isolation rules still apply.',
  },
  {
    id: 'connections',
    name: 'Use connections',
    scope: 'Only the MCP tools and connections granted to the agent.',
  },
];
const names: Record<CapabilityDecision, string> = {
  allow: 'Automatic',
  ask: 'Ask first',
  deny: 'Blocked',
};
const rank = { allow: 0, ask: 1, deny: 2 };
function decision(
  policy: CapabilityPolicy | null | undefined,
  cap: Capability,
): CapabilityDecision {
  return (
    policy?.overrides[cap] ??
    (!policy || policy.tier === 'automatic' || cap === 'file_read'
      ? 'allow'
      : policy.tier === 'read_only'
        ? 'deny'
        : 'ask')
  );
}

/** Stored engine permissions, independent of the surrounding agent/team form. */
export function CapabilityPolicyEditor({
  scope,
  scopeId = '',
  client,
  isConnected = true,
}: {
  scope: ScopedCapabilityPolicy['scope'];
  scopeId?: string;
  client: LocalEngine;
  isConnected?: boolean;
}) {
  const id = useId();
  const [rows, setRows] = useState<ScopedCapabilityPolicy[]>([]);
  const [draft, setDraft] = useState<CapabilityPolicy | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<SaveCapabilityPolicy | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [reload, setReload] = useState(0);
  const saving = useRef(false);
  const current = rows.find((row) => row.scope === scope && row.scope_id === scopeId);
  const workspace =
    scope === 'workspace' ? undefined : rows.find((row) => row.scope === 'workspace');
  const dirty = JSON.stringify(draft) !== JSON.stringify(current?.policy ?? null);
  useEffect(() => {
    const controller = new AbortController();
    setLoaded(false);
    setError('');
    setNotice('');
    setPending(null);
    if (isConnected) {
      client
        .capabilityPolicies(controller.signal)
        .then((values) => {
          if (controller.signal.aborted) return;
          setRows(values);
          setDraft(
            values.find((row) => row.scope === scope && row.scope_id === scopeId)?.policy ?? null,
          );
          setLoaded(true);
        })
        .catch((e) => {
          if (!controller.signal.aborted)
            setError(e instanceof Error ? e.message : 'Permissions could not be loaded.');
        });
    }
    return () => controller.abort();
  }, [client, isConnected, scope, scopeId, reload]);
  async function save() {
    if (saving.current || !loaded || !isConnected) return;
    saving.current = true;
    setBusy(true);
    setError('');
    setNotice('');
    const request = pending || {
      scope,
      scope_id: scopeId,
      expected_revision: current?.revision ?? 0,
      request_id: crypto.randomUUID(),
      policy: draft,
    };
    setPending(request);
    try {
      const result = await client.saveCapabilityPolicy(request);
      if (
        result.scope !== request.scope ||
        result.scope_id !== request.scope_id ||
        result.revision !== request.expected_revision + 1 ||
        !samePolicy(result.policy, request.policy)
      )
        throw new Error('The engine did not confirm these permissions. Retry the same save.');
      setRows((values) => [
        ...values.filter((row) => !(row.scope === scope && row.scope_id === scopeId)),
        result,
      ]);
      setDraft(result.policy);
      setPending(null);
      setNotice('Saved. These limits apply to subsequent tool actions, including ongoing work.');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Permissions could not be saved.');
      if (e instanceof EngineRequestError && [400, 401, 403, 404, 409, 422].includes(e.status)) {
        setPending(null);
        setLoaded(false);
      }
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  const locked = !loaded || !isConnected || busy || !!pending;
  return (
    <section className="capability-policy" aria-labelledby={`${id}-title`}>
      <div className="capability-policy-heading">
        <h3 id={`${id}-title`}>
          {scope === 'workspace'
            ? 'Workspace permissions'
            : scope === 'team'
              ? 'Team permissions'
              : 'Agent permissions'}
        </h3>
        <span>Access & autonomy</span>
      </div>
      <p>
        {scope === 'workspace'
          ? 'Set the limits for every agent working here.'
          : scope === 'team'
            ? 'Limits for work assigned to this team. Each agent keeps its own permissions.'
            : 'These limits stay with this agent across its work.'}{' '}
        More restrictive workspace, team and agent rules always win.
      </p>
      <label className="capability-tier" htmlFor={`${id}-tier`}>
        How much freedom?
      </label>
      <select
        id={`${id}-tier`}
        disabled={locked}
        value={draft?.tier || 'inherit'}
        onChange={(e) => {
          setNotice('');
          setDraft(
            e.target.value === 'inherit'
              ? null
              : { tier: e.target.value as AutonomyTier, overrides: {} },
          );
        }}
      >
        <option value="inherit">
          {scope === 'workspace' ? 'Use host rules' : 'Use inherited limits'}
        </option>
        <option value="read_only">Read files only</option>
        <option value="review_changes">Ask before changes</option>
        <option value="automatic">Automatic within granted access</option>
      </select>
      <p className="capability-tier-note">
        {draft?.tier === 'read_only'
          ? 'File reads are allowed. File changes, commands and connections are blocked.'
          : draft?.tier === 'review_changes'
            ? 'File reads continue automatically. Changes, commands and connection calls need your approval.'
            : 'Existing folders, tool grants and host rules still apply. This does not grant full machine access.'}
      </p>
      {loaded && (
        <div className="capability-summary" aria-label="Capability limits">
          {capabilities.map((cap) => {
            const own = decision(draft, cap.id),
              inherited = decision(workspace?.policy, cap.id);
            const restricted = rank[inherited] > rank[own];
            const effective = restricted ? inherited : own;
            return (
              <div key={cap.id}>
                <span>{cap.name}</span>
                <strong data-permission={effective}>
                  {names[effective]}
                  {restricted && ' · Workspace'}
                </strong>
              </div>
            );
          })}
        </div>
      )}
      <button
        type="button"
        className="capability-text-button"
        aria-expanded={expanded}
        aria-controls={`${id}-rules`}
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? 'Hide capability rules' : 'Customize by capability'}
      </button>
      {expanded && (
        <div id={`${id}-rules`} className="capability-rules">
          {capabilities.map((cap) => (
            <label key={cap.id}>
              <span>
                <strong>{cap.name}</strong>
                <small>{cap.scope}</small>
              </span>
              <select
                aria-label={`${cap.name} permission`}
                disabled={locked}
                value={draft?.overrides[cap.id] || 'preset'}
                onChange={(e) => {
                  const policy = draft || { tier: 'automatic' as const, overrides: {} };
                  const overrides = { ...policy.overrides };
                  if (e.target.value === 'preset') delete overrides[cap.id];
                  else overrides[cap.id] = e.target.value as CapabilityDecision;
                  setDraft({ ...policy, overrides });
                  setNotice('');
                }}
              >
                <option value="preset">Use preset</option>
                <option value="allow">Automatic</option>
                <option value="ask">Ask first</option>
                <option value="deny">Blocked</option>
              </select>
            </label>
          ))}
        </div>
      )}
      <p className="capability-tier-note">
        {scope === 'agent' ? 'A team can narrow these limits during its work. ' : ''}Tool
        availability, working folders and mandatory host approvals are checked separately. Changing
        limits does not stop an action already running.
      </p>
      {!isConnected && <p role="status">Connect to the engine to manage permissions.</p>}
      {isConnected && !loaded && !error && <p role="status">Loading permissions…</p>}
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      <div className="capability-policy-actions">
        {(dirty || pending) && loaded && (
          <button
            type="button"
            className="canvas-primary"
            disabled={busy || !isConnected}
            onClick={() => void save()}
          >
            {busy ? 'Saving…' : pending ? 'Retry permission save' : 'Save permissions'}
          </button>
        )}
        {error && !pending && (
          <button
            type="button"
            disabled={busy || !isConnected}
            onClick={() => setReload((v) => v + 1)}
          >
            Reload permissions
          </button>
        )}
        {dirty && !pending && (
          <small>
            Saved separately from {scope === 'workspace' ? 'other settings' : `${scope} details`}.
          </small>
        )}
      </div>
    </section>
  );
}
function samePolicy(a: CapabilityPolicy | null, b: CapabilityPolicy | null) {
  return (
    a?.tier === b?.tier && capabilities.every(({ id }) => a?.overrides[id] === b?.overrides[id])
  );
}
