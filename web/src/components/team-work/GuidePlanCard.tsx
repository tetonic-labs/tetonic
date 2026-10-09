import type { PlanView } from '../../engine/contracts';
import { stateLabels } from '../../engine/projections/records';
import { useLocalEngine } from '../../context/LocalEngineContext';

/** A conversation-sized receipt. Full review still owns consent and dispatch. */
export function GuidePlanCard({
  view,
  onReview,
  onTeamMap,
}: {
  view: PlanView;
  onReview: () => void;
  onTeamMap?: (id: string) => void;
}) {
  const { workspace, isConnected, approvals } = useLocalEngine();
  const execution = view.execution;
  const plan = view.plans[0];
  const content = execution?.receipt.content || plan?.content;
  const tasks = [...(execution?.root ? [execution.root] : []), ...(execution?.assignments || [])];
  const needsInput = tasks.some(
    (task) => task.state === 'waiting_human' && task.human_questions?.some((q) => !q.answer),
  );
  const workIds = new Set([
    execution?.receipt.root_work_id,
    ...(execution?.receipt.assignments.map((a) => a.work_id) || []),
  ]);
  const needsApproval = approvals?.pending_approvals.some(
    (approval) => !!approval.work_id && workIds.has(approval.work_id),
  );
  const failed = !!execution && ['failed', 'recovery_required'].includes(execution.state);
  const result =
    execution?.state === 'completed' &&
    execution.root?.messages.some((m) => m.role === 'assistant' && m.content.trim());
  const blocked =
    !execution && (view.readiness.length > 0 || plan?.brief_revision !== view.brief_revision);
  const running = execution && ['starting', 'running', 'canceling'].includes(execution.state);
  const label = execution
    ? needsInput || needsApproval
      ? 'Needs you'
      : result
        ? 'Result ready'
        : execution.state === 'completed'
          ? 'Execution finished'
          : stateLabels[execution.state] || execution.state
    : plan?.status === 'drafting'
      ? 'Preparing the proposal'
      : blocked
        ? 'Before we start'
        : 'Ready for your review';
  const completed = execution?.assignments.filter((t) => t.state === 'completed').length || 0;
  const agents = [...new Set(content?.assignments.map((a) => a.agent_key))].map(
    (key) => workspace?.agents.find((a) => a.key === key)?.name || key,
  );
  return (
    <section
      className="tw-guide-plan-card"
      aria-label="Team work summary"
      data-attention={failed || needsInput || needsApproval || blocked}
    >
      <div className="tw-guide-plan-status" role="status">
        <span
          className="tw-guide-plan-dot"
          data-running={!!running && isConnected}
          aria-hidden="true"
        />
        {!isConnected && 'Last seen · '}
        {label}
      </div>
      <h3>{content?.title || 'Putting the next steps together'}</h3>
      {content && (
        <p className="tw-guide-plan-meta">
          {execution
            ? `${completed} of ${content.assignments.length} contributions ready`
            : `${content.assignments.length} ${content.assignments.length === 1 ? 'assignment' : 'assignments'} · ${agents.length} ${agents.length === 1 ? 'agent' : 'agents'}`}{' '}
          · {agents.slice(0, 3).join(', ')}
          {agents.length > 3 ? ` +${agents.length - 3}` : ''}
        </p>
      )}
      {execution?.error || view.generation?.error ? (
        <p className="tw-guide-plan-issue">{execution?.error || view.generation?.error}</p>
      ) : (
        <p>
          {execution
            ? needsInput || needsApproval
              ? 'Your team needs a decision to continue.'
              : failed
                ? 'Work stopped. Review what happened before continuing.'
                : running
                  ? 'Follow the assignments on the map. You can keep talking here.'
                  : result
                    ? 'Open the result and its supporting contributions.'
                    : 'The recorded work is saved.'
            : blocked
              ? 'Review the proposal and resolve what the team needs.'
              : content?.summary || 'You can keep talking while the Guide prepares it.'}
        </p>
      )}
      <div className="tw-guide-plan-actions">
        <button type="button" className="cw-primary" onClick={onReview}>
          {execution
            ? needsInput || needsApproval
              ? 'Respond to team'
              : failed
                ? 'Review issue'
                : result
                  ? 'Read result'
                  : 'Work details'
            : 'Review proposal'}
        </button>
        {execution && onTeamMap && (
          <button type="button" onClick={() => onTeamMap(execution.receipt.root_work_id)}>
            View on map
          </button>
        )}
      </div>
      {!execution && content && <small>Nothing starts until you review and approve.</small>}
    </section>
  );
}
