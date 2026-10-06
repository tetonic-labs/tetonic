import type { Agent } from '../../types';
import type { WorkspaceResource } from '../../lib/toolLibrary';
import { agentTools, harnesses, permissionLabels } from '../../lib/agentConfiguration';

export function AgentConfigurationSummary({
  agent,
  resources,
}: {
  agent: Agent;
  resources: WorkspaceResource[];
}) {
  const setup = agent.configuration;
  if (!setup) return null;
  const toolNames = setup.toolIds.map(
    (id) => agentTools.find((tool) => tool.id === id)?.name || id,
  );
  const resourceNames = setup.resourceIds.map(
    (id) => resources.find((resource) => resource.id === id)?.name || 'Removed resource',
  );
  return (
    <section className="agent-config-summary" aria-label="Agent configuration">
      <div className="agent-config-runtime">
        <span>{agent.model}</span>
        <span>{harnesses.find((harness) => harness.id === setup.harness)?.name} harness</span>
      </div>
      <p>{[...toolNames, ...resourceNames].join(' · ') || 'No tools selected'}</p>
      <details>
        <summary>Permissions & scope</summary>
        <dl>
          <div>
            <dt>Execution</dt>
            <dd>This workstation</dd>
          </div>
          <div>
            <dt>Working folder</dt>
            <dd>{setup.scope.workspacePath || 'Choose when work starts'}</dd>
          </div>
          <div>
            <dt>Context</dt>
            <dd>
              {setup.scope.context === 'task' ? 'Current task only' : 'Task + shared team context'}
            </dd>
          </div>
          <div>
            <dt>Work requests</dt>
            <dd>{setup.scope.requests === 'owner' ? 'Only me' : 'Team, with my approval'}</dd>
          </div>
          <div>
            <dt>File changes</dt>
            <dd>{permissionLabels[setup.permissions.fileChanges]}</dd>
          </div>
          <div>
            <dt>Shell commands</dt>
            <dd>{permissionLabels[setup.permissions.shell]}</dd>
          </div>
          <div>
            <dt>Network access</dt>
            <dd>{permissionLabels[setup.permissions.network]}</dd>
          </div>
          <div>
            <dt>Run limits</dt>
            <dd>
              {setup.limits.maxSteps} steps · {setup.limits.maxSeconds}s ·{' '}
              {setup.limits.maxTokens.toLocaleString()} tokens
            </dd>
          </div>
        </dl>
      </details>
      <small>Preview configuration · not a runtime permission grant.</small>
    </section>
  );
}
