import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import {
  connectionDraftScope,
  EngineRequestError,
  type ContinuePlanRequest,
  type PlanView,
} from '../../lib/localEngine';

/** Preparing a continuation saves a proposal; only the existing start control runs it. */
export function PlanRecovery({ workId, view }: { workId: string; view: PlanView }) {
  const { client, isConnected, refresh } = useLocalEngine();
  const key = `tetonic_plan_continue:${connectionDraftScope()}:${workId}`;
  const [pending, setPending] = useState<ContinuePlanRequest | null>(() => {
    try {
      return JSON.parse(sessionStorage.getItem(key) || 'null');
    } catch {
      return null;
    }
  });
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(view.continuation_to);
  const continuation = view.continuation_to || saved;
  const recovery = view.recovery;
  async function prepare() {
    if (gate.current || !isConnected || !view.execution || (!pending && !recovery?.available))
      return;
    gate.current = true;
    setBusy(true);
    setError('');
    const request = pending || {
      request_id: crypto.randomUUID(),
      expected_root_work_id: view.execution.receipt.root_work_id,
    };
    try {
      sessionStorage.setItem(key, JSON.stringify(request));
      setPending(request);
      const result = await client.continuePlan(workId, request);
      if (
        result.source_work_id !== workId ||
        result.root_work_id !== request.expected_root_work_id ||
        !result.continuation_work_id
      )
        throw new Error('The proposal was not confirmed. Check the same request again.');
      setSaved(result);
      sessionStorage.removeItem(key);
      setPending(null);
      await refresh();
      window.location.hash = `shape=${encodeURIComponent(result.continuation_work_id)}`;
    } catch (e) {
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        sessionStorage.removeItem(key);
        setPending(null);
      }
      setError(e instanceof Error ? e.message : 'Could not prepare the continuation.');
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  if (!continuation && !recovery) return null;
  return (
    <section className="tw-plan-recovery" aria-label="Continue unfinished work">
      <h4>{continuation ? 'A continuation is saved' : 'Pick up where the team left off'}</h4>
      {continuation ? (
        <a href={`#shape=${encodeURIComponent(continuation.continuation_work_id)}`}>
          Open continuation →
        </a>
      ) : (
        <>
          <p>
            {recovery!.retained_count} finished{' '}
            {recovery!.retained_count === 1 ? 'contribution' : 'contributions'} kept ·{' '}
            {recovery!.unfinished_count}{' '}
            {recovery!.unfinished_count === 1 ? 'assignment' : 'assignments'} unfinished
          </p>
          <p>
            {recovery!.unfinished_count
              ? 'Review a new proposal for the unfinished work, with the finished results included.'
              : 'The contributions are ready. Review a proposal to bring them together.'}{' '}
            Nothing starts until you approve its additional allowance.
          </p>
          {recovery!.reason && <p role="status">{recovery!.reason}</p>}
          <button
            className="cw-primary"
            disabled={busy || !isConnected || (!pending && !recovery!.available)}
            onClick={() => void prepare()}
          >
            {busy
              ? 'Preparing your proposal…'
              : pending
                ? 'Check this proposal again'
                : 'Review unfinished work'}
          </button>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
