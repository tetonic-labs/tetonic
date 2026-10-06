import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { taskIsActive } from '../../lib/localEngine';
import { stateLabel, type WorkRecord } from '../../lib/workspaceRecords';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { WorkComposer } from './WorkComposer';
import { WorkUsageSummary } from './UsagePanel';
import { HumanQuestion } from './HumanQuestion';

export function WorkDetails({
  work,
  onAccepted,
}: {
  work: WorkRecord;
  onAccepted: (id: string) => void;
}) {
  const engine = useLocalEngine();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const latest = work.latest;
  async function act(retry: boolean) {
    if (!latest || !engine.isConnected || gate.current) return;
    gate.current = true;
    setBusy(true);
    setError('');
    try {
      if (retry)
        await engine.submitTask(
          latest.input,
          latest.agent_key,
          latest.parent_id || undefined,
          latest.id,
          latest.purpose,
        );
      else {
        const receipt = await engine.cancelTask(latest.id);
        if (!['canceled', 'canceling', 'completed', 'failed'].includes(receipt.state))
          throw new Error('Stop not confirmed. The request may still be running.');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'The action was not confirmed.');
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="tw-work-details" aria-label="Work details">
      <div className="tw-work-status">
        <span>
          {!engine.isConnected && 'Last seen: '}
          {stateLabel(work)}
        </span>
        {latest && taskIsActive(latest) && (
          <button
            disabled={!engine.isConnected || busy || latest.state === 'canceling'}
            onClick={() => void act(false)}
          >
            {busy || latest.state === 'canceling'
              ? 'Stopping…'
              : latest.plan
                ? 'Stop whole plan'
                : 'Stop this request'}
          </button>
        )}
      </div>
      {latest?.state === 'not_started' && !latest.plan && (
        <button disabled={!engine.isConnected || busy} onClick={() => void act(true)}>
          Retry saved request
        </button>
      )}
      {latest?.state === 'recovery_required' && (
        <p>This run was interrupted. Review its effects before starting new work.</p>
      )}
      {error && <p role="alert">{error}</p>}
      {latest?.human_questions?.map((question) => (
        <HumanQuestion
          key={question.id}
          task={latest}
          question={question}
          refresh={engine.refresh}
        />
      ))}
      {engine.workspace?.usage && (
        <details className="tw-work-usage">
          <summary>
            {latest?.plan ? 'Usage for this assignment' : 'Usage for this conversation'}
          </summary>
          {work.turns.map((turn) => {
            const usage = engine.workspace?.usage?.find((row) => row.work_id === turn.id);
            return usage ? (
              <div key={turn.id}>
                <strong>{turn.input.slice(0, 90)}</strong>
                <WorkUsageSummary usage={usage} stale={!engine.isConnected} />
              </div>
            ) : null;
          })}
        </details>
      )}
      {work.turns.length === 0 && (
        <p>Saved work with no recorded execution. No agent has been dispatched for this record.</p>
      )}
      <div className="tw-turns">
        {[...work.turns].reverse().map((turn, index) => {
          const content = (
            <article key={turn.id}>
              {turn.plan ? (
                <details className="px-shaping-human">
                  <summary>Shared context received by {turn.agent_name}</summary>
                  <pre>{turn.input}</pre>
                </details>
              ) : (
                <div className="px-shaping-human">
                  <strong>{turn.parent_id ? 'Your follow-up' : 'What you handed over'}</strong>
                  <p>{turn.input}</p>
                </div>
              )}
              {turn.error && <p role="alert">{turn.error}</p>}
              {turn.messages
                .filter((m) => m.role === 'assistant')
                .map((m) => (
                  <div className="px-shaping-answer" key={m.id}>
                    <strong>
                      {turn.agent_name}
                      {turn.state !== 'completed' && ' · partial response'}
                    </strong>
                    <FormattedMarkdown text={m.content} />
                  </div>
                ))}
              {!turn.messages.some((m) => m.role === 'assistant') && (
                <p>
                  {taskIsActive(turn)
                    ? 'Waiting for a recorded response…'
                    : 'No response recorded.'}
                </p>
              )}
              <details>
                <summary>Recorded activity and run details</summary>
                <p>
                  Request: {turn.id}
                  <br />
                  Run: {turn.run_id || 'Not started'}
                  <br />
                  State: {turn.state}
                </p>
                {turn.messages.map((m) => (
                  <details key={m.id}>
                    <summary>
                      {m.role === 'tool' ? 'Tool result' : 'Assistant response'} · record {m.id}
                    </summary>
                    <pre>{m.content}</pre>
                  </details>
                ))}
              </details>
            </article>
          );
          return index === 0 ? (
            content
          ) : (
            <details key={turn.id} className="tw-earlier-turn">
              <summary>Earlier · {turn.input.split('\n')[0].slice(0, 100)}</summary>
              {content}
            </details>
          );
        })}
      </div>
      {!!work.item?.notes?.length && (
        <details>
          <summary>Saved notes</summary>
          {work.item.notes.map((note, index) => (
            <p key={index}>{note}</p>
          ))}
        </details>
      )}
      <p className="tw-small">
        Responses appear when recorded. A completed run does not verify the answer.
      </p>
      {latest?.plan && (
        <a
          href={`#shape=${encodeURIComponent(latest.plan.source_work_id)}`}
          onClick={(event) => {
            if (!event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey) {
              event.preventDefault();
              onAccepted(latest.plan!.source_work_id);
            }
          }}
        >
          Open this team’s plan and contributions
        </a>
      )}
      {latest && !latest.plan && (
        <div className="tw-reply">
          <WorkComposer work={work} onAccepted={onAccepted} />
        </div>
      )}
    </section>
  );
}
