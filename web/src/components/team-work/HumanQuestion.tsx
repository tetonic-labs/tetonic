import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import {
  connectionDraftScope,
  EngineRequestError,
  type AnswerPlanQuestion,
  type EngineTask,
  type WorkHumanQuestion,
} from '../../lib/localEngine';

export function HumanQuestion({
  task,
  question,
  refresh,
}: {
  task: EngineTask;
  question: WorkHumanQuestion;
  refresh: () => Promise<void>;
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
    if (gate.current || !engine.isConnected) return;
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
      setConfirmed(receipt);
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
      <details className="tw-human-answer">
        <summary>Your answer to {task.agent_name}</summary>
        <p>{question.content.question}</p>
        <p>{confirmed?.answer ?? question.answer}</p>
      </details>
    );
  return (
    <section className="tw-human-question" aria-label={`${task.agent_name} needs your input`}>
      <span className="tw-small">{task.agent_name} needs your input</span>
      <h3>{question.content.question}</h3>
      <details className="tw-question-reason">
        <summary>Why it matters</summary>
        <p>{question.content.why}</p>
      </details>
      {waiting || pending ? (
        <>
          {!!question.content.options.length && (
            <div className="tw-human-choices">
              {question.content.options.map((option) => (
                <button
                  key={option}
                  type="button"
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
              {new Date(question.deadline * 1000).toLocaleTimeString([], {
                hour: '2-digit',
                minute: '2-digit',
                second: '2-digit',
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
    </section>
  );
}
