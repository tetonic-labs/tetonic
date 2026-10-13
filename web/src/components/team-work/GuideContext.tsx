import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { GuideContextView } from '../../engine/contracts';
import { EngineRequestError } from '../../engine/failure';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

/** Context is an inspectable association, never an execution target selector. */
export function GuideContext({
  conversationId,
  sequence,
  active,
}: {
  conversationId: string;
  sequence: number;
  active: boolean;
}) {
  const { client, isConnected } = useLocalEngine();
  const [view, setView] = useState<GuideContextView>();
  const [open, setOpen] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const pending = useRef<{ subject: string | null; revision: number; id: string } | undefined>(
    undefined,
  );
  useEffect(() => {
    if (!isConnected) return;
    const abort = new AbortController();
    let inFlight = false;
    async function read() {
      if (inFlight) return;
      inFlight = true;
      try {
        const result = await client.guideContext(conversationId, abort.signal);
        if (!abort.signal.aborted) {
          setView(result);
          setError('');
        }
      } catch {
        if (!abort.signal.aborted)
          setError('Discussion context could not be refreshed. Your conversation is still here.');
      } finally {
        inFlight = false;
      }
    }
    void read();
    const timer = active ? setInterval(() => void read(), 3000) : undefined;
    return () => {
      abort.abort();
      clearInterval(timer);
    };
  }, [client, conversationId, sequence, active, isConnected]);

  async function focus(subject: string | null) {
    if (!view || gate.current || !isConnected) return;
    gate.current = true;
    setBusy(true);
    setError('');
    if (!pending.current || pending.current.subject !== subject)
      pending.current = { subject, revision: view.focus.revision, id: crypto.randomUUID() };
    try {
      await client.focusGuide(
        conversationId,
        pending.current.revision,
        subject,
        pending.current.id,
      );
      pending.current = undefined;
      setView(await client.guideContext(conversationId));
    } catch (error) {
      if (error instanceof EngineRequestError && error.status === 409) {
        pending.current = undefined;
        try {
          setView(await client.guideContext(conversationId));
        } catch {
          /* Keep the conflict visible. */
        }
      }
      setError(error instanceof Error ? error.message : 'Could not change the discussion context.');
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  if (!view?.subjects.length && !view?.subject)
    return error ? (
      <p className="guide-context-error" role="status">
        {error}
      </p>
    ) : null;
  return (
    <section className="guide-understanding guide-context" aria-label="Discussion context">
      <div className="guide-understanding-heading">
        <button type="button" aria-expanded={open} onClick={() => setOpen(!open)}>
          {view?.subject ? `Thinking about ${view.subject.title}` : 'Related thinking'}
          <span>{open ? 'Hide' : 'View'}</span>
        </button>
      </div>
      {open && (
        <div>
          {view?.subject && <FormattedMarkdown text={view.subject.understanding} />}
          <label>
            Keep this discussion connected to
            <select
              aria-label="Discussion subject"
              value={view?.focus.subject_id || ''}
              disabled={busy || !isConnected}
              onChange={(e) => void focus(e.target.value || null)}
            >
              <option value="">No particular subject</option>
              {view?.subjects.map((subject) => (
                <option key={subject.id} value={subject.id}>
                  {subject.title}
                </option>
              ))}
              {view?.subject && !view.subjects.some((s) => s.id === view.subject!.id) && (
                <option value={view.subject.id}>{view.subject.title}</option>
              )}
            </select>
          </label>
          <small>
            You can also tell the Guide what belongs here. Changing this does not move or start
            work.
          </small>
          {view?.partial && (
            <small>Recent subjects shown. Ask the Guide to find an older one.</small>
          )}
        </div>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
