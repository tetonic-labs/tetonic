import type { ReactNode } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { toolDescription } from '../../lib/agentCapabilities';
import type { PlanContent, PlanView } from '../../lib/localEngine';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

/** Presents the agent's saved proposal. It never chooses work or grants access. */
export function PlanHandoff({ content, children }: { content: PlanContent; children?: ReactNode }) {
  const { workspace, catalog } = useLocalEngine();
  const first = content.assignments.filter((a) => !a.depends_on.length);
  const parallel = new Set(first.map((a) => a.agent_key)).size > 1;
  return (
    <div className="tw-handoff">
      <h3>{content.title}</h3>
      <div className="tw-plan-approach">
        <FormattedMarkdown text={content.summary} />
      </div>
      <p className="tw-handoff-sequence">
        <strong>
          {[
            ...new Set(
              content.assignments.map(
                (a) =>
                  workspace?.agents.find((agent) => agent.key === a.agent_key)?.name || a.agent_key,
              ),
            ),
          ].join(' · ')}
        </strong>
        <br />
        {parallel
          ? 'Independent assignments can run alongside each other.'
          : content.assignments.length > 1
            ? 'Assignments follow their dependencies and each agent’s availability.'
            : 'One agent will take this forward.'}
      </p>
      {children}
      <ul className="tw-handoff-assignments">
        {content.assignments.map((assignment) => (
          <li key={assignment.key}>
            <div className="tw-handoff-owner">
              <span className="tw-handoff-dot" aria-hidden="true" />
              <strong>
                {workspace?.agents.find((a) => a.key === assignment.agent_key)?.name ||
                  assignment.agent_key}
              </strong>
              <span>
                {assignment.depends_on.length
                  ? `After: ${assignment.depends_on.map((key) => content.assignments.find((a) => a.key === key)?.title || key).join(', ')}`
                  : 'First'}
              </span>
            </div>
            <h4>{assignment.title}</h4>
            <p>{assignment.deliverable}</p>
            <details>
              <summary>Assignment details</summary>
              <FormattedMarkdown text={assignment.instructions} />
              <small>
                {assignment.tools.length
                  ? `Requested access: ${toolDescription(assignment.tools, catalog)}`
                  : 'No tools requested.'}{' '}
                {assignment.token_budget.toLocaleString()} tokens proposed.
              </small>
            </details>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function PlanReadiness({
  view,
  onDiscuss,
  onAgentSettings,
  onTools,
  disabled = false,
}: {
  view: PlanView;
  onDiscuss?: (text: string) => void;
  onAgentSettings?: (key: string) => void;
  onTools?: () => void;
  disabled?: boolean;
}) {
  const { workspace, catalog, isConnected } = useLocalEngine();
  const plan = view.plans[0];
  const content = plan?.content;
  if (!content || view.execution) return null;
  const reasons = [...new Set(view.readiness)];
  const questions = content.open_questions;
  // Navigation hints from saved selections, not another admission policy.
  // The engine's readiness and start endpoint remain authoritative.
  const missing = content.assignments.flatMap((assignment) => {
    const agent = workspace?.agents.find((a) => a.key === assignment.agent_key);
    const tools = assignment.tools.filter(
      (tool) => tool !== 'finish' && !agent?.tools?.includes(tool),
    );
    return agent && tools.length ? [{ agent, tools }] : [];
  });
  const accessAgents = [...new Map(missing.map((m) => [m.agent.key, m.agent])).values()];
  const needsWorkspaceTools = missing.some((m) =>
    m.tools.some((t) => !catalog?.tools?.includes(t)),
  );
  if (!reasons.length && !questions.length) return null;
  return (
    <section className="tw-handoff-readiness" aria-label="Before the team starts">
      <h4>{reasons.length ? 'Let’s clear the way' : 'Questions in this proposal'}</h4>
      {!!reasons.length && <p>The team has not started. This proposal needs attention:</p>}
      {!!reasons.length && (
        <ul>
          {reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      )}
      {!!questions.length && (
        <ul>
          {questions.map((q, i) => (
            <li key={i}>{q}</li>
          ))}
        </ul>
      )}
      <div className="tw-handoff-actions">
        {view.setup_issues?.map(
          (issue) =>
            onAgentSettings && (
              <button
                key={issue.agent_key}
                disabled={disabled || !isConnected}
                onClick={() => onAgentSettings(issue.agent_key)}
              >
                Review agent setup
              </button>
            ),
        )}
        {onAgentSettings &&
          accessAgents.map((agent) => (
            <button
              key={agent.key}
              disabled={disabled || !isConnected}
              onClick={() => onAgentSettings(agent.key)}
            >
              Review {agent.name}’s access
            </button>
          ))}
        {needsWorkspaceTools && onTools && (
          <button disabled={disabled || !isConnected} onClick={onTools}>
            Add tools or a connection
          </button>
        )}
        {onDiscuss && (
          <button
            disabled={disabled || !isConnected}
            onClick={() =>
              onDiscuss(
                `Help me resolve the following for the saved proposal “${content.title}” (revision ${plan.revision}). Inspect the current plan before suggesting changes.\n\n${[...reasons, ...questions].map((r) => `• ${r}`).join('\n')}\n\nMy guidance: `,
              )
            }
          >
            Work through this with the Guide
          </button>
        )}
      </div>
    </section>
  );
}
