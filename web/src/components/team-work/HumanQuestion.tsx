import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../engine/connection';
import { EngineRequestError } from '../../engine/failure';
import {
  type AnswerPlanQuestion,
  type EngineTask,
  type WorkHumanQuestion,
} from '../../engine/contracts';

export function HumanQuestion({
  task,
  question,
  refresh,
  showHeading = true,
  onAnswered,
  workTitle,
  onWork,
}: {
  task: EngineTask;
  question: WorkHumanQuestion;
  refresh: () => Promise<void>;
  showHeading?: boolean;
  onAnswered?: (receipt: WorkHumanQuestion) => void;
  workTitle?: string;
  onWork?: () => void;
}) {
  const engine = useLocalEngine();
  const key = `tetonic_human_answer:${connectionDraftScope()}:${question.id}`;
  const read = (): AnswerPlanQuestion | null => {
    try {
      return JSON.parse(sessionStorage.getItem(key) || 'null');
    } catch {
      return null;
    }
  };
  const [pending, setPending] = useState(read);
  const [answer, setAnswer] = useState(() => {
    try {
      return read()?.answer || sessionStorage.getItem(`${key}:draft`) || '';
    } catch {
      return '';
    }
  });
  const [now, setNow] = useState(Date.now());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [confirmed, setConfirmed] = useState<WorkHumanQuestion | null>(null);
  const gate = useRef(false);
  const card = useRef<HTMLElement>(null);
  const receiptStatus = useRef<HTMLParagraphElement>(null);
  const focusReceipt = useRef(false);
  useEffect(() => {
    if (confirmed && focusReceipt.current) {
      receiptStatus.current?.focus();
      focusReceipt.current = false;
    }
  }, [confirmed]);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const waiting = task.state === 'waiting_human' && question.deadline * 1000 > now;
  function change(value: string) {
    setAnswer(value);
    try {
      sessionStorage.setItem(`${key}:draft`, value);
    } catch {
      /* Keep the current edit. */
    }
  }
  async function send() {
    if (
      gate.current ||
      !engine.isConnected ||
      confirmed ||
      question.answer !== null ||
      (!pending &&
        (!waiting ||
          question.deadline * 1000 <= Date.now() ||
          !answer.trim() ||
          new TextEncoder().encode(answer.trim()).length > 6000))
    )
      return;
    gate.current = true;
    setBusy(true);
    setError('');
    const command = read() ||
      pending || {
        request_id: crypto.randomUUID(),
        question_id: question.id,
        answer: answer.trim(),
      };
    try {
      // Save command identity before sending: an uncertain response must retry the same answer.
      sessionStorage.setItem(key, JSON.stringify(command));
      setPending(command);
      const receipt = await engine.client.answerPlanQuestion(task.id, command);
      if (
        receipt.id !== question.id ||
        receipt.response_id !== command.request_id ||
        receipt.answer !== command.answer
      )
        throw new Error('Your answer has not been confirmed. Check the same answer again.');
      focusReceipt.current = !!card.current?.contains(document.activeElement);
      setConfirmed(receipt);
      onAnswered?.(receipt);
      sessionStorage.removeItem(key);
      sessionStorage.removeItem(`${key}:draft`);
      setPending(null);
      await refresh();
      await engine.refresh();
    } catch (e) {
      if (e instanceof EngineRequestError && e.status >= 400 && e.status < 500) {
        sessionStorage.removeItem(key);
        setPending(null);
      }
      setError(e instanceof Error ? e.message : 'Your answer has not been confirmed.');
      await refresh().catch(() => {});
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  if (confirmed || question.answer !== null)
    return (
      <section className="tw-human-answer" aria-label={`Answer saved for ${task.agent_name}`}>
        <div className="tw-request-context">
          <span>{task.agent_name} · Answer saved</span>
          {workTitle && <p>{workTitle}</p>}
        </div>
        <p role="status" ref={receiptStatus} tabIndex={-1}>
          {!engine.isConnected
            ? 'Your answer was saved. Reconnect to check the work.'
            : task.state === 'running'
              ? `${task.agent_name} is working again.`
              : task.state === 'completed'
                ? 'This work has finished. Open the work to review what was recorded.'
                : ['failed', 'recovery_required'].includes(task.state)
                  ? 'The work needs a look before it can continue.'
                  : ['canceled', 'canceling'].includes(task.state)
                    ? 'The work has stopped or is stopping.'
                    : `Waiting for an update from ${task.agent_name}.`}
        </p>
        <details>
          <summary>Your answer to {task.agent_name}</summary>
          <p>{question.content.question}</p>
          <p>{confirmed?.answer ?? question.answer}</p>
        </details>
        {onWork && (
          <button className="operator-secondary" onClick={onWork}>
            Open related work
          </button>
        )}
      </section>
    );
  return (
    <section
      ref={card}
      className="tw-human-question"
      aria-label={`${task.agent_name} needs your input`}
    >
      {showHeading && (
        <>
          <div className="tw-request-context">
            <span>{task.agent_name} · Needs your input</span>
            {workTitle && <p>{workTitle}</p>}
          </div>
          <h3>{question.content.question}</h3>
        </>
      )}
      <p className="tw-question-context">{question.content.why}</p>
      {waiting || pending ? (
        <>
          {!!question.content.options.length && (
            <div className="tw-human-choices">
              {question.content.options.map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={answer === option}
                  disabled={busy || !!pending}
                  onClick={() => change(option)}
                >
                  {option}
                </button>
              ))}
            </div>
          )}
          <label>
            Your answer
            <textarea
              value={answer}
              disabled={busy || !!pending}
              onChange={(e) => change(e.target.value)}
              rows={3}
              maxLength={6000}
            />
          </label>
          <div className="tw-human-actions">
            <button
              className="cw-primary"
              disabled={
                busy ||
                !engine.isConnected ||
                (!pending &&
                  (!answer.trim() || new TextEncoder().encode(answer.trim()).length > 6000))
              }
              onClick={() => void send()}
            >
              {busy ? 'Sending…' : pending ? 'Check this answer again' : 'Send answer'}
            </button>
            <small>
              {!waiting && pending
                ? 'The wait ended; this checks whether your earlier answer was saved. '
                : ''}
              Reply by{' '}
              {new Date(question.deadline * 1000).toLocaleString([], {
                month: 'short',
                day: 'numeric',
                hour: '2-digit',
                minute: '2-digit',
              })}
              . The current time limit still applies.
            </small>
          </div>
        </>
      ) : (
        <p>
          This wait has ended. Review the saved work before starting more work. No automatic
          restart.
        </p>
      )}
      {!waiting && answer && (
        <details>
          <summary>Your saved draft</summary>
          <p>{answer}</p>
        </details>
      )}
      {error && <p role="alert">{error}</p>}
      {onWork && (
        <button className="operator-secondary tw-question-work" onClick={onWork}>
          Open related work
        </button>
      )}
    </section>
  );
}
