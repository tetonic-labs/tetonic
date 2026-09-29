import { useEffect, useRef, useState } from 'react';
import { X, ArrowUpRight, RefreshCw } from 'lucide-react';
import type { Agent, ApprovalRequest } from '../../types';
import { teammate } from '../../lib/teammates';
import { sampleTime, workStatus } from '../../lib/workEvidence';
import type { OrganizationActivity } from './useOrganizationActivity';

interface Props {
  group?: { id: string; name: string; agentIds: string[] };
  agents: Agent[];
  approvals: ApprovalRequest[];
  activity: OrganizationActivity;
  onClose: () => void;
  onFocus: (id: string) => void;
}
type Topic = 'status' | 'attention' | 'changes' | 'other';
export function TeamInspection({ group, agents, approvals, activity, onClose, onFocus }: Props) {
  const capture = () => ({
    time: activity.elapsed,
    people: agents
      .filter((a) => group?.agentIds.includes(a.id))
      .map((a) => ({
        id: a.id,
        name: teammate(a).name,
        work: activity.states.get(a.id),
        request: approvals.some((r) => r.agentId === a.id && r.status === 'pending'),
      })),
    records: activity.records
      .filter((r) => group?.agentIds.includes(r.interaction.agentId))
      .slice(-4),
  });
  const [snapshot, setSnapshot] = useState(capture);
  const [draft, setDraft] = useState('');
  const [messages, setMessages] = useState<{ question: string; topic: Topic }[]>([
    { question: 'What is this team doing?', topic: 'status' },
  ]);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    input.current?.focus({ preventScroll: true });
    return () => {
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, []);
  const working = snapshot.people.filter((p) => p.work?.interaction && !p.work.waiting);
  const waiting = snapshot.people.filter((p) => p.work?.waiting);
  const failures = snapshot.people.filter((p) => p.work?.failure);
  const requests = snapshot.people.filter((p) => p.request);
  function ask(question: string, topic: Topic) {
    setMessages((old) => [...old, { question, topic }].slice(-3));
    setDraft('');
  }
  return (
    <aside
      className="work-focus team-inspection"
      aria-label={`Asking about ${group?.name || 'team'}`}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="team-inspection-header">
        <button className="work-focus-close" onClick={onClose} aria-label="Close team inspection">
          <X size={15} />
        </button>
        <div className="work-focus-heading">
          <div>
            <strong>{group?.name}</strong>
            <small>Recorded sample · {sampleTime(snapshot.time)}</small>
          </div>
        </div>
      </div>
      <button className="text-action" onClick={() => setSnapshot(capture())}>
        <RefreshCw size={12} /> Refresh snapshot
      </button>
      <div className="team-answers" aria-live="polite">
        {messages.map((m, i) => (
          <section key={i}>
            <h3>{m.question}</h3>
            {m.topic === 'status' && (
              <>
                <p>
                  {working.length} working, {waiting.length} waiting,{' '}
                  {snapshot.people.length - working.length - waiting.length} with no current sample
                  interaction.
                </p>
                {working.slice(0, 3).map((p) => (
                  <p key={p.id}>
                    <button className="text-action" onClick={() => onFocus(p.id)}>
                      {p.name} <ArrowUpRight size={12} />
                    </button>
                    {p.work?.interaction?.label || workStatus(p.work)}
                  </p>
                ))}
              </>
            )}
            {m.topic === 'attention' && (
              <>
                <p>
                  {requests.length
                    ? `${requests.length} explicit request${requests.length === 1 ? '' : 's'} for human input.`
                    : 'No pending human request is recorded.'}{' '}
                  {failures.length
                    ? `${failures.length} agent${failures.length === 1 ? ' has' : 's have'} unresolved failed work.`
                    : 'No unresolved failure is recorded.'}
                </p>
                {[...new Map([...requests, ...failures].map((p) => [p.id, p])).values()].map(
                  (p) => (
                    <button key={p.id} className="text-action" onClick={() => onFocus(p.id)}>
                      {p.name} · {p.request ? 'needs input' : 'failed work'}{' '}
                      <ArrowUpRight size={12} />
                    </button>
                  ),
                )}
                {waiting.length > 0 && (
                  <p>
                    {waiting.length} waiting on work. A wait alone is not a request for your
                    intervention.
                  </p>
                )}
              </>
            )}
            {m.topic === 'changes' &&
              (snapshot.records.length ? (
                <ol>
                  {snapshot.records.map((r) => (
                    <li key={r.id}>
                      {sampleTime(r.at)} · {r.state}
                      <span>
                        {r.interaction.label} / {r.interaction.targetName}
                      </span>
                    </li>
                  ))}
                </ol>
              ) : (
                <p>No activity has been played yet in this sample.</p>
              ))}
            {m.topic === 'other' && (
              <p>
                I can summarize recorded progress, waits, failures, requests, and recent changes
                here. Open the agent’s work for more evidence. A conversational model is not
                connected in this preview.
              </p>
            )}
          </section>
        ))}
      </div>
      <div className="team-prompts">
        <button onClick={() => ask('What needs my attention?', 'attention')}>Needs me?</button>
        <button onClick={() => ask('What changed recently?', 'changes')}>What changed?</button>
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const q = draft.trim();
          if (q)
            ask(
              q,
              /block|fail|help|need|wait|attention/i.test(q)
                ? 'attention'
                : /chang|recent|happen/i.test(q)
                  ? 'changes'
                  : /doing|status|work|progress|up to/i.test(q)
                    ? 'status'
                    : 'other',
            );
        }}
      >
        <input
          ref={input}
          maxLength={350}
          aria-label={`Ask about ${group?.name}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Ask about this team…"
        />
        <button type="submit" disabled={!draft.trim()} aria-label="Ask about recorded activity">
          <ArrowUpRight size={17} />
        </button>
      </form>
      <small className="team-inspection-note">
        Inspection only. Nothing is sent to the team. The map continues at its current playback
        time.
      </small>
    </aside>
  );
}
