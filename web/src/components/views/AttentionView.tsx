import type { Agent, ApprovalRequest } from '../../types';
import type { WorkState } from '../../lib/workScene';
import { sampleTime } from '../../lib/workEvidence';
import { teammateName } from '../../lib/teammates';
import { ArrowUpRight } from 'lucide-react';

export function workExceptions(states: Map<string, WorkState>, _approvals: ApprovalRequest[]) {
  return [...states.values()]
    .flatMap((work) => [
      ...work.failures.map((failure) => ({
        id: failure.interaction.id,
        agentId: work.id,
        at: failure.at,
        title: failure.interaction.label,
        target: failure.interaction.targetName,
        state: failure.retryId ? 'Recovery in progress' : 'Unresolved failure',
        explanation: failure.retryId
          ? 'A linked retry is running. Resolution is not yet verified.'
          : 'No recovery is recorded. Inspect the failed operation before deciding what to do.',
      })),
    ])
    .sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
}

export function AttentionView({
  agents,
  approvals,
  states,
  onRequest,
  onWork,
  onHistory,
  workDecisions = [],
  onDecision,
}: {
  agents: Agent[];
  approvals: ApprovalRequest[];
  states: Map<string, WorkState>;
  onRequest: (id: string) => void;
  onWork: (id: string) => void;
  onHistory: () => void;
  workDecisions?: { id: string; title: string; context: string; summary: string }[];
  onDecision?: (id: string) => void;
}) {
  const pending = approvals.filter((a) => a.status === 'pending');
  const exceptions = workExceptions(states, approvals);
  const name = (id: string) => teammateName(id, agents.find((a) => a.id === id)?.name || id);
  return (
    <div className="attention-view">
      <h2>Needs attention</h2>
      <p className="dialog-intro">Across all teams · decisions and recorded work to review.</p>
      {workDecisions.length > 0 && (
        <section aria-label="Work decisions">
          <h3>Your judgment</h3>
          {workDecisions.map((item) => (
            <button className="attention-row" key={item.id} onClick={() => onDecision?.(item.id)}>
              <span>
                <small>{item.context} · work preview</small>
                <strong>{item.title}</strong>
                <span>{item.summary}</span>
              </span>
              <ArrowUpRight size={17} />
            </button>
          ))}
        </section>
      )}
      {pending.length > 0 && (
        <section aria-label="Approval requests">
          <h3>
            {pending.length} {pending.length === 1 ? 'decision' : 'decisions'} requested
          </h3>
          {pending.map((request) => (
            <button
              className="attention-row"
              key={request.id}
              onClick={() => onRequest(request.id)}
            >
              <span>
                <small>{name(request.agentId)} · approval</small>
                <strong>{request.title}</strong>
                <span>{request.reason}</span>
              </span>
              <ArrowUpRight size={17} />
            </button>
          ))}
        </section>
      )}
      {exceptions.length > 0 && (
        <section aria-label="Work to review">
          <h3>
            {exceptions.length} work {exceptions.length === 1 ? 'item' : 'items'} to review
          </h3>
          {exceptions.map((item) => (
            <button className="attention-row" key={item.id} onClick={() => onWork(item.agentId)}>
              <span>
                <small>
                  {name(item.agentId)} · {sampleTime(item.at)} sample
                </small>
                <strong>
                  {item.state}: {item.target}
                </strong>
                <span>{item.title}</span>
                <small>{item.explanation}</small>
              </span>
              <ArrowUpRight size={17} />
            </button>
          ))}
        </section>
      )}
      {!pending.length && !exceptions.length && !workDecisions.length && (
        <div className="attention-empty">
          <h3>No recorded items to review.</h3>
          <p>
            This covers the loaded approvals and current sample playback. It is not a live health
            check.
          </p>
        </div>
      )}
      <button className="text-action" onClick={onHistory}>
        Approval history <ArrowUpRight size={14} />
      </button>
    </div>
  );
}
