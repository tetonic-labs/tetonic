import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../engine/connection';
import { EngineRequestError } from '../../engine/failure';
import { type WorkBrief } from '../../engine/contracts';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

type Save = { request_id: string; expected_revision: number; body: string };
type Draft = { text: string; base: number; pending?: Save };

export function WorkingBrief({
  workId,
  suggestion,
  onContinue,
}: {
  workId: string;
  suggestion?: string;
  onContinue?: () => void;
}) {
  const { client, isConnected } = useLocalEngine();
  const storageKey = `tetonic_brief_draft:${connectionDraftScope()}:${workId}`;
  const [draft, setDraft] = useState<Draft | null>(() => {
    try {
      const value = JSON.parse(sessionStorage.getItem(storageKey) || 'null');
      return value &&
        typeof value.text === 'string' &&
        Number.isSafeInteger(value.base) &&
        value.base >= 0 &&
        (!value.pending ||
          (typeof value.pending.request_id === 'string' &&
            value.pending.body === value.text &&
            value.pending.expected_revision === value.base))
        ? value
        : null;
    } catch {
      return null;
    }
  });
  const [rows, setRows] = useState<WorkBrief[] | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const gate = useRef(false);
  const mounted = useRef(true);
  function keep(value: Draft) {
    setDraft(value);
    setSaved(false);
    try {
      sessionStorage.setItem(storageKey, JSON.stringify(value));
    } catch {
      setError('This draft cannot be retained in this tab. Keep a copy until it is saved.');
    }
  }
  useEffect(() => {
    mounted.current = true;
    const controller = new AbortController();
    if (isConnected)
      client
        .briefs(workId, controller.signal)
        .then((value) => {
          if (!controller.signal.aborted) {
            setRows(value);
            setDraft((old) => old || { text: value[0]?.body || '', base: value[0]?.revision || 0 });
          }
        })
        .catch((e) => {
          if (!controller.signal.aborted) setError(e.message);
        });
    return () => {
      mounted.current = false;
      controller.abort();
    };
  }, [client, workId, isConnected]);
  const current = rows?.[0];
  const conflict = !!draft && rows !== null && draft.base !== (current?.revision || 0);
  async function reload() {
    try {
      const value = await client.briefs(workId);
      if (!mounted.current) return;
      setRows(value);
      setDraft((old) => old || { text: value[0]?.body || '', base: value[0]?.revision || 0 });
      setError('');
    } catch (e) {
      if (mounted.current)
        setError(e instanceof Error ? e.message : 'Could not load the saved brief.');
    }
  }
  async function save() {
    if (!draft || !rows || gate.current || !isConnected || (conflict && !draft.pending)) return;
    gate.current = true;
    setBusy(true);
    setError('');
    const pending = draft.pending || {
      request_id: crypto.randomUUID(),
      expected_revision: draft.base,
      body: draft.text,
    };
    keep({ ...draft, pending });
    try {
      const receipt = await client.saveBrief(workId, pending);
      if (
        receipt.work_id !== workId ||
        receipt.request_id !== pending.request_id ||
        receipt.body !== pending.body ||
        receipt.revision !== pending.expected_revision + 1
      )
        throw new Error('This save was not confirmed. Retry to check the same operation.');
      if (!mounted.current) return;
      keep({ text: receipt.body, base: receipt.revision });
      setRows((old) =>
        [receipt, ...(old || []).filter((row) => row.revision !== receipt.revision)].sort(
          (a, b) => b.revision - a.revision,
        ),
      );
      setSaved(true);
    } catch (e) {
      if (!mounted.current) return;
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        keep({ ...draft, pending: undefined });
        // The old base remains pinned. Reload exposes any conflict before another save.
        try {
          const value = await client.briefs(workId);
          if (!mounted.current) return;
          setRows(value);
        } catch {
          if (mounted.current) setRows(null);
        }
      }
      if (!mounted.current) return;
      setError(e instanceof Error ? e.message : 'Could not confirm the save.');
    } finally {
      gate.current = false;
      if (mounted.current) setBusy(false);
    }
  }
  return (
    <section className="cw-working-brief" aria-label="Working brief">
      <p>Keep your decisions and open questions here. Saving won’t start work.</p>
      {rows === null && (
        <button type="button" onClick={() => void reload()} disabled={!isConnected}>
          Load saved brief
        </button>
      )}
      {draft && (
        <>
          <label htmlFor="working-brief">Current understanding</label>
          <textarea
            id="working-brief"
            value={draft.text}
            rows={12}
            readOnly={busy || !!draft.pending}
            placeholder="What we understand, what remains uncertain, and the approach we’re considering…"
            onChange={(e) => keep({ ...draft, text: e.target.value })}
          />
          {suggestion && !draft.text && (
            <button
              disabled={busy || !!draft.pending}
              onClick={() => keep({ ...draft, text: suggestion })}
            >
              Use the latest response as a draft
            </button>
          )}
          {conflict && (
            <div role="alert">
              <p>
                A newer revision is saved. Your draft is preserved. Review it before replacing or
                applying your changes.
              </p>
              <details>
                <summary>Latest saved brief · revision {current?.revision}</summary>
                <FormattedMarkdown text={current?.body || ''} />
              </details>
              <button
                disabled={!!draft.pending}
                onClick={() => keep({ text: current?.body || '', base: current?.revision || 0 })}
              >
                Use saved version
              </button>
              <button
                disabled={!!draft.pending}
                onClick={() => keep({ ...draft, base: current?.revision || 0 })}
              >
                Keep my text against this revision
              </button>
            </div>
          )}
          <div className="cw-brief-actions">
            <button
              className="cw-primary"
              onClick={() => void save()}
              disabled={
                !isConnected ||
                rows === null ||
                busy ||
                (conflict && !draft.pending) ||
                !draft.text.trim() ||
                new TextEncoder().encode(draft.text).length > 12000
              }
            >
              {busy ? 'Saving…' : draft.pending ? 'Retry save' : 'Save brief'}
            </button>
            <span role="status">
              {saved
                ? `Saved · revision ${draft.base}`
                : current
                  ? `Saved revision ${current.revision}`
                  : 'No saved brief yet'}
            </span>
          </div>
        </>
      )}
      {current && draft?.text === current.body && !draft.pending && onContinue && (
        <button className="tw-next-step" onClick={onContinue}>
          Shape the team’s plan <span>Use this saved brief →</span>
        </button>
      )}
      {error && <p role="alert">{error}</p>}
      {!!rows?.length && (
        <details>
          <summary>Saved revisions · latest {rows.length}</summary>
          {rows.map((row) => (
            <article key={row.revision}>
              <h3>Revision {row.revision}</h3>
              <FormattedMarkdown text={row.body} />
            </article>
          ))}
        </details>
      )}
    </section>
  );
}
