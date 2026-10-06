import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import {
  connectionDraftScope,
  EngineRequestError,
  type AmendPlanAssignment,
  type PlanExecutionView,
} from '../../lib/localEngine';

export function affectedAssignments(execution: PlanExecutionView, key: string) {
  const affected = new Set([key]);
  let count = -1;
  while (count !== affected.size) {
    count = affected.size;
    for (const a of execution.receipt.content.assignments)
      if (a.depends_on.some((dependency) => affected.has(dependency))) affected.add(a.key);
  }
  return execution.receipt.content.assignments.filter((a) => affected.has(a.key));
}
export function PlanDirectionEditor({
  execution,
  refresh,
}: {
  execution: PlanExecutionView;
  refresh: () => Promise<void>;
}) {
  const engine = useLocalEngine();
  const source = execution.receipt.source_work_id;
  const storageKey = `tetonic_plan_direction:${connectionDraftScope()}:${source}`;
  const read = (): AmendPlanAssignment | null => {
    try {
      return JSON.parse(sessionStorage.getItem(storageKey) || 'null');
    } catch {
      return null;
    }
  };
  const draft = () => {
    try {
      return JSON.parse(
        sessionStorage.getItem(`${storageKey}:draft`) || 'null',
      ) as AmendPlanAssignment | null;
    } catch {
      return null;
    }
  };
  const [pending, setPending] = useState(read);
  const [key, setKey] = useState(read()?.assignment_key || draft()?.assignment_key || '');
  const [instructions, setInstructions] = useState(
    read()?.instructions || draft()?.instructions || '',
  );
  const [baseRevision, setBaseRevision] = useState(
    read()?.expected_revision ??
      draft()?.expected_revision ??
      (execution.directions?.at(-1)?.revision || 0),
  );
  const revision = execution.directions?.at(-1)?.revision || 0;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const gate = useRef(false);
  const upcoming = execution.receipt.content.assignments.filter((a) =>
    affectedAssignments(execution, a.key).every((affected) => {
      const pin = execution.receipt.assignments.find((p) => p.assignment_key === affected.key);
      return execution.assignments.find((t) => t.id === pin?.work_id)?.state === 'not_started';
    }),
  );
  const active = ['running', 'waiting_human'].includes(execution.state);
  const affected = key ? affectedAssignments(execution, key) : [];
  const completed = execution.assignments.filter((t) => t.state === 'completed');
  function choose(value: string) {
    setKey(value);
    setSaved('');
    setBaseRevision(revision);
    try {
      sessionStorage.removeItem(`${storageKey}:draft`);
    } catch {
      /* Keep this edit in memory. */
    }
    setInstructions(
      execution.directions?.filter((d) => d.assignment_key === value).at(-1)?.instructions ||
        execution.receipt.content.assignments.find((a) => a.key === value)?.instructions ||
        '',
    );
  }
  function edit(value: string) {
    setInstructions(value);
    try {
      sessionStorage.setItem(
        `${storageKey}:draft`,
        JSON.stringify({
          assignment_key: key,
          instructions: value,
          expected_revision: baseRevision,
        }),
      );
    } catch {
      /* Keep this edit. */
    }
  }
  async function apply() {
    if (gate.current || !engine.isConnected) return;
    gate.current = true;
    setBusy(true);
    setError('');
    setSaved('');
    const command = read() ||
      pending || {
        request_id: crypto.randomUUID(),
        expected_revision: baseRevision,
        assignment_key: key,
        instructions: instructions.trim(),
      };
    try {
      sessionStorage.setItem(storageKey, JSON.stringify(command));
      setPending(command);
      const receipt = await engine.client.amendPlanAssignment(source, command);
      if (
        receipt.request_id !== command.request_id ||
        receipt.revision !== command.expected_revision + 1 ||
        receipt.instructions !== command.instructions ||
        receipt.assignment_key !== command.assignment_key
      )
        throw new Error('The change has not been confirmed. Check this change again.');
      sessionStorage.removeItem(storageKey);
      sessionStorage.removeItem(`${storageKey}:draft`);
      setPending(null);
      setKey('');
      setInstructions('');
      setSaved('Direction saved for upcoming work. Completed contributions are retained.');
      await refresh();
    } catch (e) {
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        sessionStorage.removeItem(storageKey);
        setPending(null);
      }
      setError(e instanceof Error ? e.message : 'The change has not been confirmed.');
      await refresh().catch(() => {});
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  if (!active && !pending && !execution.directions?.length && !instructions) return null;
  return (
    <details className="tw-plan-direction">
      <summary>{active || pending ? 'Adjust upcoming work' : 'Saved direction changes'}</summary>
      {active || pending ? (
        <>
          <p>Change the instructions before an assignment starts.</p>
          <label>
            Assignment
            <select
              value={key}
              disabled={busy || !!pending}
              onChange={(e) => choose(e.target.value)}
            >
              <option value="">Choose upcoming work</option>
              {execution.receipt.content.assignments
                .filter((a) => upcoming.includes(a) || a.key === key)
                .map((a) => (
                  <option key={a.key} value={a.key} disabled={!upcoming.includes(a)}>
                    {a.title}
                  </option>
                ))}
            </select>
          </label>
          {key && (
            <>
              <label>
                Updated instructions
                <textarea
                  rows={5}
                  maxLength={6000}
                  value={instructions}
                  disabled={busy || !!pending}
                  onChange={(e) => edit(e.target.value)}
                />
              </label>
              <p>
                <strong>Affects:</strong> {affected.map((a) => a.title).join(', ')}.
              </p>
              <p>
                {completed.length} completed{' '}
                {completed.length === 1 ? 'contribution stays' : 'contributions stay'} saved.
                Agents, tools and allowance stay the same.
              </p>
              {!pending && revision !== baseRevision && (
                <p>
                  The plan’s direction changed while you were editing. Your text is kept.{' '}
                  <button onClick={() => setBaseRevision(revision)}>
                    Use latest direction revision
                  </button>
                </p>
              )}
              <button
                className="cw-primary"
                disabled={
                  busy ||
                  !engine.isConnected ||
                  (!pending &&
                    (revision !== baseRevision ||
                      !upcoming.some((a) => a.key === key) ||
                      !instructions.trim() ||
                      new TextEncoder().encode(instructions.trim()).length > 6000))
                }
                onClick={() => void apply()}
              >
                {busy ? 'Saving…' : pending ? 'Check this change again' : 'Apply to upcoming work'}
              </button>
            </>
          )}
          {!upcoming.length && !pending && (
            <p>There are no unstarted assignments available to change.</p>
          )}
        </>
      ) : (
        <p>This plan has ended. Its saved direction changes are below.</p>
      )}
      {saved && <p role="status">{saved}</p>}
      {!active && !pending && instructions && (
        <details>
          <summary>Your unsaved instructions</summary>
          <p>{instructions}</p>
        </details>
      )}
      {error && <p role="alert">{error}</p>}
      {!!execution.directions?.length && (
        <details>
          <summary>Direction history · {execution.directions.length}</summary>
          {execution.directions.map((d) => (
            <div key={d.revision}>
              <strong>
                {
                  execution.receipt.content.assignments.find((a) => a.key === d.assignment_key)
                    ?.title
                }{' '}
                · revision {d.revision}
              </strong>
              <p>{d.instructions}</p>
            </div>
          ))}
        </details>
      )}
    </details>
  );
}
