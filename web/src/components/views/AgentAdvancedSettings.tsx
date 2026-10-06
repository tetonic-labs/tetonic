import { ChevronDown, SlidersHorizontal } from 'lucide-react';
import type { AgentConfiguration, PermissionRule } from '../../lib/agentConfiguration';

export function AgentAdvancedSettings({
  value,
  onChange,
  hasTeam,
  enforcedLimits,
}: {
  value: AgentConfiguration;
  onChange: (value: AgentConfiguration) => void;
  hasTeam: boolean;
  enforcedLimits?: AgentConfiguration['limits'];
}) {
  const scope = (change: Partial<AgentConfiguration['scope']>) =>
    onChange({ ...value, scope: { ...value.scope, ...change } });
  return (
    <details className="agent-advanced">
      <summary>
        <SlidersHorizontal size={17} aria-hidden="true" />
        <span>
          Advanced settings
          <small>
            {enforcedLimits ? 'Local execution & run limits' : 'Permissions, scope & run limits'}
          </small>
        </span>
        <ChevronDown size={17} className="agent-disclosure" aria-hidden="true" />
      </summary>
      <div className="agent-advanced-body">
        <div className="agent-host-note">
          <strong>This workstation</strong>
          <span>Execution stays on the agent owner’s machine.</span>
        </div>
        {enforcedLimits ? (
          <p className="agent-field-note">
            Only you can request work through this local connection. Records stay in your Personal
            workspace. Selected file tools use the engine’s configured folder. Shell, web and
            shared-human access are unavailable.
          </p>
        ) : (
          <>
            <fieldset>
              <legend>Permissions</legend>
              <p className="agent-field-note">
                Tools stay subject to the owner’s execution policy.
              </p>
              <div className="agent-form-grid three-columns">
                {(
                  [
                    ['fileChanges', 'File changes', 'Within working folder'],
                    ['shell', 'Shell commands', 'Within working folder'],
                    ['network', 'Network access', 'Selected connections only'],
                  ] as const
                ).map(([key, label, scoped]) => (
                  <label key={key}>
                    {label}
                    <select
                      value={value.permissions[key]}
                      onChange={(event) =>
                        onChange({
                          ...value,
                          permissions: {
                            ...value.permissions,
                            [key]: event.target.value as PermissionRule,
                          },
                        })
                      }
                    >
                      <option value="ask">Ask first</option>
                      <option value="deny">Blocked</option>
                      <option value="scoped">{scoped}</option>
                    </select>
                  </label>
                ))}
              </div>
            </fieldset>
            <fieldset>
              <legend>Scope</legend>
              <label>
                Working folder
                <input
                  value={value.scope.workspacePath}
                  onChange={(event) => scope({ workspacePath: event.target.value })}
                  maxLength={512}
                  placeholder="Choose when work starts"
                  aria-describedby="agent-folder-help"
                />
              </label>
              <p className="agent-field-note" id="agent-folder-help">
                Optional. File access and scoped commands stay within this folder.
              </p>
              <div className="agent-form-grid">
                <label>
                  Context access
                  <select
                    value={value.scope.context}
                    onChange={(event) => scope({ context: event.target.value as 'task' | 'team' })}
                  >
                    <option value="task">Current task only</option>
                    <option value="team" disabled={!hasTeam}>
                      Task + shared team context
                    </option>
                  </select>
                </label>
                <label>
                  Who can request work?
                  <select
                    value={value.scope.requests}
                    onChange={(event) =>
                      scope({ requests: event.target.value as 'owner' | 'team_approval' })
                    }
                  >
                    <option value="owner">Only me</option>
                    <option value="team_approval" disabled={!hasTeam}>
                      Team, with my approval
                    </option>
                  </select>
                </label>
              </div>
              {!hasTeam && (
                <p className="agent-field-note">
                  Choose a team above to use shared context or accept team requests.
                </p>
              )}
            </fieldset>
          </>
        )}
        <fieldset>
          <legend>Per-run limits</legend>
          <div className="agent-form-grid three-columns">
            {(
              [
                ['maxSteps', 'Steps', 1, 1000],
                ['maxSeconds', 'Time (seconds)', 10, 86400],
                ['maxTokens', 'Token budget', 256, 1000000],
              ] as const
            ).map(([key, label, min, max]) => (
              <label key={key}>
                {label}
                <input
                  type="number"
                  required
                  min={min}
                  max={enforcedLimits?.[key] ?? max}
                  step={1}
                  value={value.limits[key] || ''}
                  onChange={(event) =>
                    onChange({
                      ...value,
                      limits: { ...value.limits, [key]: Number(event.target.value) },
                    })
                  }
                />
              </label>
            ))}
          </div>
        </fieldset>
      </div>
    </details>
  );
}
