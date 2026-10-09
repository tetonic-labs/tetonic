import { Check, CircleAlert } from 'lucide-react';
import { useId, useState } from 'react';
import { type GuideActivity as Activity } from '../../engine/contracts';

const labels: Record<Activity['operation'], { pending: string; done: string; failed: string }> = {
  resources: {
    pending: 'Checking agents, tools and budgets',
    done: 'Checked agents, tools and budgets',
    failed: 'Could not check workspace resources',
  },
  work: {
    pending: 'Checking work status and results',
    done: 'Checked work status and results',
    failed: 'Could not inspect work',
  },
  inspect: {
    pending: 'Reviewing the plan and its readiness',
    done: 'Reviewed the plan and its readiness',
    failed: 'Could not inspect this plan',
  },
  propose: {
    pending: 'Saving the team proposal',
    done: 'Saved a team proposal for review',
    failed: 'Could not save the team proposal',
  },
};

export function GuideActivity({
  activities,
  active,
  connected,
}: {
  activities: Activity[];
  active: boolean;
  connected: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const activityId = useId();
  const pending = activities.some((item) => item.state === 'requested');
  const current = activities.find((item) => item.state === 'requested') || activities.at(-1);
  const visible = expanded ? activities : current ? [current] : [];
  const issues = activities.filter((item) =>
    ['failed', 'interrupted', 'unconfirmed'].includes(item.state),
  ).length;
  return (
    <div className="guide-activity" aria-label="Guide activity">
      {!!activities.length && (
        <ul id={activityId}>
          {visible.map((item) => {
            const label = labels[item.operation];
            if (!label) return null;
            const working = item.state === 'requested' && active && connected;
            const complete = item.state === 'completed';
            const text = complete
              ? label.done
              : working
                ? `${label.pending}…`
                : item.state === 'failed'
                  ? label.failed
                  : item.state === 'requested' && !connected
                    ? 'Connection lost · check not confirmed'
                    : `${label.pending} · not confirmed`;
            return (
              <li
                key={item.id}
                data-state={working ? 'active' : complete ? 'completed' : 'unconfirmed'}
              >
                {working ? (
                  <span className="guide-activity-dots" aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </span>
                ) : complete ? (
                  <Check size={12} aria-hidden="true" />
                ) : (
                  <CircleAlert size={12} aria-hidden="true" />
                )}
                <span>{text}</span>
              </li>
            );
          })}
        </ul>
      )}
      {activities.length > 1 && (
        <button
          type="button"
          className="guide-activity-toggle"
          aria-expanded={expanded}
          aria-controls={activityId}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? 'Hide activity' : `Show ${activities.length} actions`}
          {!!issues && ` · ${issues} not confirmed`}
        </button>
      )}
      {active && (!pending || !connected) && (
        <p className="px-shaping-status" role="status">
          {connected && (
            <span className="guide-activity-dots" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          )}
          {connected ? 'Thinking it through…' : 'Connection lost. The reply may still be running.'}
        </p>
      )}
      {active && pending && connected && (
        <span className="sr-only" role="status">
          {labels[activities.find((item) => item.state === 'requested')!.operation]?.pending}
        </span>
      )}
    </div>
  );
}
