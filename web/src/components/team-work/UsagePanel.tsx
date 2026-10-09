import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { type WorkUsage } from '../../engine/contracts';
import './usage.css';

const number = (value: number) => value.toLocaleString();
export function WorkUsageSummary({ usage, stale = false }: { usage: WorkUsage; stale?: boolean }) {
  const used = usage.input_tokens + usage.output_tokens;
  const limit = usage.budget?.token_limit;
  const unknown = usage.unknown_calls > 0;
  const held = usage.held_tokens;
  return (
    <div className="tw-usage-detail">
      <p className="tw-usage-state">
        {stale
          ? 'Last recorded usage'
          : usage.over_limit
            ? 'Allowance exceeded · work stopped'
            : unknown
              ? 'Some usage is unconfirmed'
              : usage.pending_calls
                ? 'Model request in progress'
                : usage.calls
                  ? 'Usage recorded'
                  : usage.budget
                    ? 'Allowance ready'
                    : 'Usage not tracked for this request'}
      </p>
      {limit !== undefined && (
        <div
          className="tw-usage-bar"
          aria-label={`${number(used)} tokens reported, ${number(held)} held, ${number(usage.budget!.available_tokens)} available`}
        >
          <span className="used" style={{ width: `${Math.min(100, (used / limit) * 100)}%` }} />
          <span
            className="held"
            style={{
              width: `${Math.min(100 - Math.min(100, (used / limit) * 100), (held / limit) * 100)}%`,
            }}
          />
        </div>
      )}
      {(usage.calls > 0 || usage.budget) && (
        <dl className="tw-usage-numbers">
          <div>
            <dt>Reported</dt>
            <dd>
              {unknown || usage.pending_calls ? '≥ ' : ''}
              {number(used)}
            </dd>
          </div>
          {usage.budget && (
            <>
              <div>
                <dt>Held</dt>
                <dd>{number(held)}</dd>
              </div>
              <div>
                <dt>Available</dt>
                <dd>{number(usage.budget.available_tokens)}</dd>
              </div>
            </>
          )}
        </dl>
      )}
      {usage.calls > 0 && (
        <p className="tw-usage-note">
          {number(usage.input_tokens)} input · {number(usage.output_tokens)} output ·{' '}
          {number(usage.calls)} model {usage.calls === 1 ? 'request' : 'requests'}
        </p>
      )}
      {unknown && (
        <p className="tw-usage-warning">
          A request ended without a complete usage report. Its allowance stays held; it has not been
          counted as free.
        </p>
      )}
      {usage.budget && (
        <p className="tw-usage-note">
          {number(limit!)} token allowance
          {usage.budget.delegated_tokens
            ? ` · ${number(usage.budget.delegated_tokens)} allocated to child work`
            : ''}
          {usage.released_tokens
            ? ` · ${number(usage.released_tokens)} unused tokens returned`
            : ''}
        </p>
      )}
    </div>
  );
}

export function UsagePanel({ onWork }: { onWork: (id: string) => void }) {
  const engine = useLocalEngine();
  const { workspace, isConnected } = engine;
  const setting = workspace?.budget_setting;
  const rows = workspace?.usage;
  const max = workspace?.budget_max_tokens || 4096;
  const [draft, setDraft] = useState(String(setting?.token_limit ?? max));
  const [revision, setRevision] = useState(setting?.revision ?? 0);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [more, setMore] = useState(false);
  const gate = useRef(false);
  const edited = useRef(false);
  const pending = useRef<{
    request_id: string;
    expected_revision: number;
    token_limit: number;
  } | null>(null);
  useEffect(() => {
    if (
      !edited.current &&
      !gate.current &&
      !dirty &&
      !busy &&
      setting &&
      setting.revision >= revision
    ) {
      setDraft(String(setting.token_limit ?? max));
      setRevision(setting.revision);
    }
  }, [setting?.revision, setting?.token_limit, max, dirty, busy, revision]);
  async function save() {
    if (!isConnected || gate.current) return;
    const tokens = Number(draft);
    if (!Number.isSafeInteger(tokens) || tokens < 1 || tokens > max) {
      setError(`Choose a whole number from 1 to ${number(max)}.`);
      return;
    }
    gate.current = true;
    setBusy(true);
    setError('');
    setMessage('');
    const command = (pending.current ||= {
      request_id: crypto.randomUUID(),
      expected_revision: revision,
      token_limit: tokens,
    });
    try {
      const result = await engine.client.setBudgetSetting(command);
      if (
        result.revision !== command.expected_revision + 1 ||
        result.token_limit !== command.token_limit
      )
        throw new Error(
          'The engine did not confirm this allowance. Refresh usage before trying again.',
        );
      setRevision(result.revision);
      edited.current = false;
      setDirty(false);
      pending.current = null;
      setMessage('Saved. This applies to new requests; existing allowances stay the same.');
      await engine.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'The allowance was not confirmed.');
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  if (!rows || !setting)
    return (
      <p>Usage reporting is not available from this engine yet. No usage total has been assumed.</p>
    );
  const total = rows.reduce((n, r) => n + r.input_tokens + r.output_tokens, 0);
  const held = rows.reduce((n, r) => n + r.held_tokens, 0);
  const incomplete = rows.filter((r) => r.unknown_calls > 0).length;
  const pendingCalls = rows.reduce((n, r) => n + r.pending_calls, 0);
  const tracked = rows.filter((r) => r.calls > 0).length;
  const ordered = [...rows].sort(
    (a, b) =>
      Number(b.over_limit || b.unknown_calls > 0) - Number(a.over_limit || a.unknown_calls > 0) ||
      b.input_tokens + b.output_tokens - a.input_tokens - a.output_tokens,
  );
  return (
    <section className="tw-usage" aria-label="Usage and allowances">
      {!isConnected && <p role="status">Connection lost. These are the last recorded figures.</p>}
      <div className="tw-usage-hero">
        <span>Tokens reported</span>
        <strong>
          {incomplete || pendingCalls ? '≥ ' : ''}
          {number(total)}
        </strong>
        <p>
          {tracked
            ? `Across ${tracked} recorded ${tracked === 1 ? 'request' : 'requests'}, including planning.`
            : 'New work will appear here as it uses the model.'}
        </p>
      </div>
      <div className="tw-usage-overview">
        <div>
          <strong>{number(held)}</strong>
          <span>held in allowances</span>
        </div>
        <div>
          <strong>{incomplete || pendingCalls}</strong>
          <span>{incomplete ? 'requests need usage review' : 'model requests in progress'}</span>
        </div>
      </div>
      {incomplete > 0 && (
        <p className="tw-usage-warning" role="status">
          Some usage is unconfirmed. Reported totals are a lower bound.
        </p>
      )}
      <details className="tw-usage-setting">
        <summary>
          Allowance for new requests <span>{number(setting.token_limit ?? max)} tokens</span>
        </summary>
        <p>
          Limit how much model work each new request can use. An agent’s lower limit still applies.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label htmlFor="work-token-allowance">Tokens per request</label>
          <div>
            <input
              id="work-token-allowance"
              type="number"
              min={1}
              max={max}
              step={1}
              value={draft}
              disabled={busy || !isConnected}
              onChange={(e) => {
                if (!edited.current) setRevision(setting.revision);
                edited.current = true;
                setDraft(e.target.value);
                setDirty(true);
                pending.current = null;
                setMessage('');
                setError('');
              }}
            />
            <button type="submit" disabled={busy || !isConnected || !dirty}>
              {busy ? 'Saving…' : 'Save allowance'}
            </button>
          </div>
        </form>
        {message && <p role="status">{message}</p>}
        {error && (
          <>
            <p role="alert">{error}</p>
            <button
              className="tw-usage-link"
              disabled={busy || !isConnected}
              onClick={() => {
                pending.current = null;
                edited.current = false;
                setDirty(false);
                setError('');
                setMessage('');
                void engine.refresh();
              }}
            >
              Reload saved allowance
            </button>
          </>
        )}
        <p className="tw-usage-note">
          Stops further model calls when reported usage reaches the allowance. A request already
          sent can exceed it. This is not a provider billing cap.
        </p>
      </details>
      <div className="tw-usage-list">
        <h3>Where it went</h3>
        {ordered.length === 0 ? (
          <p>No work yet.</p>
        ) : (
          (more ? ordered : ordered.slice(0, 8)).map((row) => (
            <details key={row.work_id} className="tw-usage-row">
              <summary>
                <span>
                  {row.title}
                  <small>
                    {row.purpose === 'explore' ? 'Shaping & planning' : 'Work'}
                    {row.unknown_calls
                      ? ' · usage unconfirmed'
                      : row.over_limit
                        ? ' · over allowance'
                        : ''}
                  </small>
                </span>
                <strong>
                  {row.calls
                    ? `${row.pending_calls || row.unknown_calls ? '≥ ' : ''}${number(row.input_tokens + row.output_tokens)}`
                    : row.budget
                      ? 'Ready'
                      : 'Not tracked'}
                </strong>
              </summary>
              <WorkUsageSummary usage={row} stale={!isConnected} />
              <button className="tw-usage-link" onClick={() => onWork(row.work_id)}>
                Open work →
              </button>
            </details>
          ))
        )}
      </div>
      {!more && ordered.length > 8 && (
        <button className="tw-usage-link" onClick={() => setMore(true)}>
          Show all {ordered.length} requests
        </button>
      )}
      <details className="tw-usage-help">
        <summary>How to read usage</summary>
        <p>
          Tokens measure model input and output. Reported is what the provider returned. Held is
          reserved for work whose unused allowance has not been safely returned. Available belongs
          to that work item; it is not a shared workspace balance.
        </p>
        <p>
          New requests include follow-ups and planning. Older work may have no usage records. Local
          inference still uses compute; token counts are not a dollar cost.
        </p>
      </details>
    </section>
  );
}
